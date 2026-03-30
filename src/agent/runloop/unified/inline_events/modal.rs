use anyhow::Result;

use tracing::warn;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::{InlineHandle, InlineHeaderContext, InlineListSelection};

use crate::agent::runloop::model_picker::{
    ModelPickerProgress, ModelPickerStart, ModelPickerState,
};
use crate::agent::runloop::slash_commands::SessionPaletteMode;
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::{
    ActivePalette, LIGHTWEIGHT_MODEL_ACTION_PREFIX, MODEL_TARGET_ACTION_LIGHTWEIGHT,
    MODEL_TARGET_ACTION_MAIN, handle_palette_cancel, handle_palette_preview,
    handle_palette_selection, show_lightweight_model_palette,
};
use crate::agent::runloop::unified::settings_interactive::{
    ACTION_PICK_LIGHTWEIGHT_MODEL, ACTION_PICK_MAIN_MODEL,
};
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
use crate::agent::runloop::welcome::SessionBootstrap;

use super::action::InlineLoopAction;

pub(crate) struct InlineModalProcessor<'a> {
    model_picker: ModelPickerCoordinator<'a>,
    palette: PaletteCoordinator<'a>,
}

impl<'a> InlineModalProcessor<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        handle: &'a InlineHandle,
        header_context: &'a mut InlineHeaderContext,
        model_picker_state: &'a mut Option<ModelPickerState>,
        palette_state: &'a mut Option<ActivePalette>,
        config: &'a mut CoreAgentConfig,
        vt_cfg: &'a mut Option<VTCodeConfig>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        session_bootstrap: &'a SessionBootstrap,
        full_auto: bool,
        conversation_history_len: usize,
    ) -> Self {
        let model_picker = ModelPickerCoordinator {
            state: model_picker_state,
            header_context,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            handle,
            full_auto,
            conversation_history_len,
        };
        let palette = PaletteCoordinator {
            state: palette_state,
            handle,
        };

        Self {
            model_picker,
            palette,
        }
    }

    pub(crate) async fn handle_submit(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: InlineListSelection,
    ) -> Result<InlineLoopAction> {
        // Handle plan approval selections (Claude Code style HITL)
        match &selection {
            InlineListSelection::PlanApprovalExecute => {
                return Ok(InlineLoopAction::PlanApproved { auto_accept: false });
            }
            InlineListSelection::PlanApprovalAutoAccept => {
                return Ok(InlineLoopAction::PlanApproved { auto_accept: true });
            }
            InlineListSelection::PlanApprovalEditPlan => {
                return Ok(InlineLoopAction::PlanEditRequested);
            }
            _ => {}
        }

        match self.model_picker.handle_submit(renderer, selection).await? {
            ModelPickerOutcome::SkipPalette => Ok(InlineLoopAction::Continue),
            ModelPickerOutcome::ForwardToPalette(selection) => {
                if self.handle_palette_redirect(renderer, &selection).await? {
                    return Ok(InlineLoopAction::Continue);
                }
                self.palette
                    .handle_submit(
                        renderer,
                        selection,
                        self.model_picker.config,
                        self.model_picker.vt_cfg,
                        self.model_picker.provider_client.as_ref(),
                        self.model_picker.session_bootstrap,
                        self.model_picker.full_auto,
                    )
                    .await
            }
            ModelPickerOutcome::Continue => Ok(InlineLoopAction::Continue),
        }
    }

    pub(crate) fn handle_cancel(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<InlineLoopAction> {
        if self.model_picker.handle_cancel(renderer)? {
            return Ok(InlineLoopAction::Continue);
        }

        self.palette.handle_cancel(renderer)?;
        Ok(InlineLoopAction::Continue)
    }

    pub(crate) fn handle_preview(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: InlineListSelection,
    ) -> Result<InlineLoopAction> {
        self.palette.handle_preview(renderer, selection)
    }

    async fn handle_palette_redirect(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: &InlineListSelection,
    ) -> Result<bool> {
        let Some(active) = self.palette.state.as_ref() else {
            return Ok(false);
        };

        match (active, selection) {
            (ActivePalette::ModelTarget, InlineListSelection::ConfigAction(action))
                if action == MODEL_TARGET_ACTION_MAIN =>
            {
                self.palette.state.take();
                self.model_picker.start_picker(renderer).await?;
                Ok(true)
            }
            (ActivePalette::ModelTarget, InlineListSelection::ConfigAction(action))
                if action == MODEL_TARGET_ACTION_LIGHTWEIGHT =>
            {
                self.palette.state.take();
                if show_lightweight_model_palette(
                    renderer,
                    self.model_picker.config,
                    self.model_picker.vt_cfg.as_ref(),
                )? {
                    *self.palette.state = Some(ActivePalette::LightweightModel);
                }
                Ok(true)
            }
            (ActivePalette::Settings { .. }, InlineListSelection::ConfigAction(action))
                if action == ACTION_PICK_MAIN_MODEL =>
            {
                self.palette.state.take();
                self.model_picker.start_picker(renderer).await?;
                Ok(true)
            }
            (ActivePalette::Settings { .. }, InlineListSelection::ConfigAction(action))
                if action == ACTION_PICK_LIGHTWEIGHT_MODEL =>
            {
                self.palette.state.take();
                if show_lightweight_model_palette(
                    renderer,
                    self.model_picker.config,
                    self.model_picker.vt_cfg.as_ref(),
                )? {
                    *self.palette.state = Some(ActivePalette::LightweightModel);
                }
                Ok(true)
            }
            (ActivePalette::LightweightModel, InlineListSelection::ConfigAction(action))
                if action.starts_with(LIGHTWEIGHT_MODEL_ACTION_PREFIX) =>
            {
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

struct PaletteCoordinator<'a> {
    state: &'a mut Option<ActivePalette>,
    handle: &'a InlineHandle,
}

impl<'a> PaletteCoordinator<'a> {
    #[allow(clippy::too_many_arguments)]
    async fn handle_submit(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: InlineListSelection,
        config: &mut CoreAgentConfig,
        vt_cfg: &mut Option<VTCodeConfig>,
        provider_client: &dyn uni::LLMProvider,
        session_bootstrap: &SessionBootstrap,
        full_auto: bool,
    ) -> Result<InlineLoopAction> {
        if let Some(active) = self.state.take() {
            match (&active, &selection) {
                (
                    ActivePalette::Sessions {
                        mode: SessionPaletteMode::Resume,
                        ..
                    },
                    InlineListSelection::Session(session_id),
                ) => {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Resuming session: {}", session_id),
                    )?;
                    return Ok(InlineLoopAction::ResumeSession(session_id.clone()));
                }
                (
                    ActivePalette::Sessions {
                        mode: SessionPaletteMode::Fork,
                        listings,
                        limit,
                        show_all,
                    },
                    InlineListSelection::Session(session_id),
                ) => {
                    crate::agent::runloop::unified::palettes::show_fork_mode_palette(
                        renderer, session_id,
                    )?;
                    *self.state = Some(ActivePalette::ForkMode {
                        session_id: session_id.clone(),
                        listings: listings.clone(),
                        limit: *limit,
                        show_all: *show_all,
                    });
                    return Ok(InlineLoopAction::Continue);
                }
                (
                    ActivePalette::ForkMode { .. },
                    InlineListSelection::SessionForkMode {
                        session_id,
                        summarize,
                    },
                ) => {
                    let mode_label = if *summarize {
                        "summarized"
                    } else {
                        "full-copy"
                    };
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Forking session: {} ({mode_label})", session_id),
                    )?;
                    return Ok(InlineLoopAction::ForkSession {
                        session_id: session_id.clone(),
                        summarize: *summarize,
                    });
                }
                _ => {}
            }

            let restore = handle_palette_selection(
                active,
                selection,
                renderer,
                self.handle,
                config,
                vt_cfg,
                provider_client,
                session_bootstrap,
                full_auto,
            )
            .await?;
            if let Some(state) = restore {
                *self.state = Some(state);
            }
            return Ok(InlineLoopAction::Continue);
        }

        // Safety net: If palette state is missing, log and inform the user
        warn!(
            "Palette selection {:?} dropped because no active palette was tracked",
            selection
        );
        renderer.line(
            MessageStyle::Error,
            "Selection dismissed because the palette is no longer active. Please try again.",
        )?;

        Ok(InlineLoopAction::Continue)
    }

    fn handle_cancel(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if let Some(active) = self.state.take() {
            let restore = handle_palette_cancel(active, renderer, self.handle)?;
            if let Some(state) = restore {
                *self.state = Some(state);
            }
        }

        Ok(())
    }

    fn handle_preview(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: InlineListSelection,
    ) -> Result<InlineLoopAction> {
        if let Some(active) = self.state.take() {
            let restore = handle_palette_preview(active, selection, renderer, self.handle)?;
            if let Some(state) = restore {
                *self.state = Some(state);
            }
        }

        Ok(InlineLoopAction::Continue)
    }
}

struct ModelPickerCoordinator<'a> {
    state: &'a mut Option<ModelPickerState>,
    header_context: &'a mut InlineHeaderContext,
    config: &'a mut CoreAgentConfig,
    vt_cfg: &'a mut Option<VTCodeConfig>,
    provider_client: &'a mut Box<dyn uni::LLMProvider>,
    session_bootstrap: &'a SessionBootstrap,
    handle: &'a InlineHandle,
    full_auto: bool,
    conversation_history_len: usize,
}

impl<'a> ModelPickerCoordinator<'a> {
    async fn start_picker(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if self.state.is_some() {
            renderer.line(
                MessageStyle::Error,
                "A model picker session is already active. Complete or cancel it before starting another.",
            )?;
            return Ok(());
        }

        let reasoning = self
            .vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.reasoning_effort)
            .unwrap_or(self.config.reasoning_effort);
        let service_tier = self
            .vt_cfg
            .as_ref()
            .and_then(|cfg| cfg.provider.openai.service_tier);
        let workspace_hint = Some(self.config.workspace.clone());
        let picker_start = {
            let loading_spinner = if renderer.supports_inline_ui() {
                Some(PlaceholderSpinner::new(
                    self.handle,
                    Some(String::new()),
                    Some(String::new()),
                    "Loading model lists...",
                ))
            } else {
                renderer.line(MessageStyle::Info, "Loading model lists...")?;
                None
            };
            let result = ModelPickerState::new(
                renderer,
                self.vt_cfg.clone(),
                reasoning,
                service_tier,
                workspace_hint,
                self.config.provider.clone(),
                self.config.model.clone(),
            )
            .await;
            drop(loading_spinner);
            result
        };

        match picker_start {
            Ok(ModelPickerStart::InProgress(picker)) => {
                *self.state = Some(picker);
            }
            Ok(ModelPickerStart::Completed { state, selection }) => {
                if let Err(err) = finalize_model_selection(
                    renderer,
                    &state,
                    selection,
                    self.config,
                    self.vt_cfg,
                    self.provider_client,
                    self.session_bootstrap,
                    self.handle,
                    self.header_context,
                    self.full_auto,
                    self.conversation_history_len,
                )
                .await
                {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to apply model selection: {}", err),
                    )?;
                }
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to start model picker: {}", err),
                )?;
            }
        }

        Ok(())
    }

    async fn handle_submit(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: InlineListSelection,
    ) -> Result<ModelPickerOutcome> {
        let Some(picker) = self.state.as_mut() else {
            return Ok(ModelPickerOutcome::ForwardToPalette(selection));
        };

        let progress = picker.handle_list_selection(renderer, selection)?;
        match progress {
            ModelPickerProgress::InProgress => {}
            ModelPickerProgress::NeedsRefresh => {
                picker.refresh_dynamic_models(renderer).await?;
                return Ok(ModelPickerOutcome::SkipPalette);
            }
            ModelPickerProgress::Cancelled => {
                *self.state = None;
                if !renderer.supports_inline_ui() {
                    renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                }
            }
            ModelPickerProgress::Completed(selection) => {
                let Some(picker_state) = self.state.take() else {
                    warn!("Model picker completed but no state was available");
                    return Ok(ModelPickerOutcome::Continue);
                };
                if let Err(err) = finalize_model_selection(
                    renderer,
                    &picker_state,
                    selection,
                    self.config,
                    self.vt_cfg,
                    self.provider_client,
                    self.session_bootstrap,
                    self.handle,
                    self.header_context,
                    self.full_auto,
                    self.conversation_history_len,
                )
                .await
                {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to apply model selection: {}", err),
                    )?;
                }
            }
        }

        Ok(ModelPickerOutcome::Continue)
    }

    fn handle_cancel(&mut self, renderer: &mut AnsiRenderer) -> Result<bool> {
        if self.state.take().is_some() {
            if !renderer.supports_inline_ui() {
                renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
            }
            return Ok(true);
        }

        Ok(false)
    }
}

enum ModelPickerOutcome {
    Continue,
    ForwardToPalette(InlineListSelection),
    SkipPalette,
}
