use anyhow::Result;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::ui::tui::{InlineHandle, InlineListSelection};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::{
    ActivePalette, handle_palette_cancel, handle_palette_selection,
};
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
        model_picker_state: &'a mut Option<ModelPickerState>,
        palette_state: &'a mut Option<ActivePalette>,
        config: &'a mut CoreAgentConfig,
        vt_cfg: &'a mut Option<VTCodeConfig>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        session_bootstrap: &'a SessionBootstrap,
        full_auto: bool,
    ) -> Self {
        let model_picker = ModelPickerCoordinator {
            state: model_picker_state,
            config,
            vt_cfg,
            provider_client,
            session_bootstrap,
            handle,
            full_auto,
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
        match self.model_picker.handle_submit(renderer, selection).await? {
            ModelPickerOutcome::SkipPalette => Ok(InlineLoopAction::Continue),
            ModelPickerOutcome::ForwardToPalette(selection) => {
                self.palette.handle_submit(renderer, selection).await?;
                Ok(InlineLoopAction::Continue)
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
}

struct PaletteCoordinator<'a> {
    state: &'a mut Option<ActivePalette>,
    handle: &'a InlineHandle,
}

impl<'a> PaletteCoordinator<'a> {
    async fn handle_submit(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: InlineListSelection,
    ) -> Result<()> {
        if let Some(active) = self.state.take() {
            let restore =
                handle_palette_selection(active, selection, renderer, self.handle).await?;
            if let Some(state) = restore {
                *self.state = Some(state);
            }
        }

        Ok(())
    }

    fn handle_cancel(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        if let Some(active) = self.state.take() {
            handle_palette_cancel(active, renderer)?;
        }

        Ok(())
    }
}

struct ModelPickerCoordinator<'a> {
    state: &'a mut Option<ModelPickerState>,
    config: &'a mut CoreAgentConfig,
    vt_cfg: &'a mut Option<VTCodeConfig>,
    provider_client: &'a mut Box<dyn uni::LLMProvider>,
    session_bootstrap: &'a SessionBootstrap,
    handle: &'a InlineHandle,
    full_auto: bool,
}

impl<'a> ModelPickerCoordinator<'a> {
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
                renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
            }
            ModelPickerProgress::Completed(selection) => {
                let picker_state = self.state.take().unwrap();
                if let Err(err) = finalize_model_selection(
                    renderer,
                    &picker_state,
                    selection,
                    self.config,
                    self.vt_cfg,
                    self.provider_client,
                    self.session_bootstrap,
                    self.handle,
                    self.full_auto,
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
            renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
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
