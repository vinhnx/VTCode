use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task;
use vtcode_config::{OpenAIServiceTier, VTCodeConfig};
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListSelection, OpenAIServiceTierChoice, from_tui_reasoning};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineSession, TransientSubmission,
    WizardModalMode, WizardStep,
};
use vtcode_tui::ui::interactive_list::SelectionInterrupted;

use crate::agent::runloop::unified::overlay_prompt::{
    OverlayWaitOutcome, wait_for_overlay_submission,
};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};
use interaction::{
    ModelSelectionListOutcome, select_model_with_ratatui_list, select_reasoning_with_ratatui,
    select_service_tier_with_ratatui,
};
use options::{MODEL_OPTIONS, ModelOption, find_option_index};
use rendering::{
    CLOSE_THEME_MESSAGE, dynamic_model_subtitle, model_search_value, prompt_api_key_plain,
    prompt_custom_model_entry, prompt_reasoning_plain, prompt_service_tier_plain,
    render_reasoning_inline, render_service_tier_inline, render_step_one_inline,
    render_step_one_plain, show_secure_api_modal, static_model_search_terms, static_model_subtitle,
};
use selection::{
    ExistingKey, ReasoningChoice, SelectionDetail, ServiceTierChoice, is_cancel_command,
    parse_model_selection, reasoning_level_description, reasoning_level_label,
    selection_from_custom_provider, selection_from_option, supports_xhigh_reasoning,
};

mod config_persistence;
mod dynamic_models;
mod interaction;
mod lightweight_palette;
mod options;
mod rendering;
mod selection;
mod state_machine;

pub(crate) use self::config_persistence::persist_lightweight_selection;
pub(crate) use self::dynamic_models::DynamicModelRegistry;
#[cfg(test)]
pub(crate) use self::lightweight_palette::build_lightweight_model_palette_view;
pub(crate) use self::lightweight_palette::{
    LightweightModelPaletteView, prepare_lightweight_model_palette_view,
};
pub(crate) use selection::ModelSelectionResult;
pub(super) use vtcode_config::read_workspace_env_value as read_workspace_env;

const SUBAGENT_MODEL_ACTION_PREFIX: &str = "subagent-model:";
const SUBAGENT_REASONING_ACTION_PREFIX: &str = "subagent-reasoning:";
const SUBAGENT_MODEL_PROMPT_ID: &str = "subagent-model-id";
const SUBAGENT_SHORTCUTS: [(&str, &str); 5] = [
    ("inherit", "Use the parent session model and configuration."),
    (
        "small",
        "Use VT Code's lightweight delegated-model shortcut.",
    ),
    (
        "haiku",
        "Use the Anthropic Haiku shortcut alias for delegated work.",
    ),
    (
        "sonnet",
        "Use the Anthropic Sonnet shortcut alias for delegated work.",
    ),
    (
        "opus",
        "Use the Anthropic Opus shortcut alias for delegated work.",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum PickerStep {
    AwaitModel,
    AwaitReasoning,
    AwaitServiceTier,
    AwaitApiKey,
}

pub(crate) enum ModelPickerProgress {
    InProgress,
    NeedsRefresh,
    Completed(ModelSelectionResult),
    Cancelled,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SubagentModelSelection {
    pub(crate) model: String,
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Clone, Debug)]
enum SubagentModelTarget {
    Shortcut { model: String },
    Concrete(SelectionDetail),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubagentReasoningChoice {
    KeepCurrent,
    Unset,
    Explicit(ReasoningEffortLevel),
}

pub(crate) struct ModelPickerState {
    options: &'static [ModelOption],
    step: PickerStep,
    inline_enabled: bool,
    vt_cfg: Option<VTCodeConfig>,
    current_reasoning: ReasoningEffortLevel,
    current_service_tier: Option<OpenAIServiceTier>,
    current_provider: String,
    current_model: String,
    selection: Option<SelectionDetail>,
    custom_providers: Vec<SelectionDetail>,
    selected_reasoning: Option<ReasoningEffortLevel>,
    selected_service_tier: Option<Option<OpenAIServiceTier>>,
    pending_api_key: Option<String>,
    workspace: Option<PathBuf>,
    ctrl_c_state: Option<Arc<CtrlCState>>,
    ctrl_c_notify: Option<Arc<Notify>>,
    dynamic_models: DynamicModelRegistry,
    plain_mode_active: bool,
}

pub(crate) enum ModelPickerStart {
    Completed {
        state: ModelPickerState,
        selection: ModelSelectionResult,
    },
    InProgress(ModelPickerState),
}

impl ModelPickerState {
    #[allow(clippy::new_ret_no_self)]
    pub(crate) async fn new(
        renderer: &mut AnsiRenderer,
        vt_cfg: Option<VTCodeConfig>,
        current_reasoning: ReasoningEffortLevel,
        current_service_tier: Option<OpenAIServiceTier>,
        workspace: Option<PathBuf>,
        current_provider: String,
        current_model: String,
        ctrl_c_state: Option<Arc<CtrlCState>>,
        ctrl_c_notify: Option<Arc<Notify>>,
    ) -> Result<ModelPickerStart> {
        let options = MODEL_OPTIONS.as_slice();
        let inline_enabled = renderer.supports_inline_ui();
        let dynamic_models =
            DynamicModelRegistry::load(options, workspace.as_deref(), vt_cfg.as_ref()).await;
        let custom_providers = vt_cfg
            .as_ref()
            .map(|cfg| {
                cfg.custom_providers
                    .iter()
                    .map(selection_from_custom_provider)
                    .collect()
            })
            .unwrap_or_default();

        let mut state = Self {
            options,
            step: PickerStep::AwaitModel,
            inline_enabled,
            vt_cfg,
            current_reasoning,
            current_service_tier,
            current_provider,
            current_model,
            selection: None,
            custom_providers,
            selected_reasoning: None,
            selected_service_tier: None,
            pending_api_key: None,
            workspace,
            ctrl_c_state,
            ctrl_c_notify,
            dynamic_models,
            plain_mode_active: false,
        };

        if inline_enabled {
            render_step_one_inline(
                renderer,
                options,
                current_reasoning,
                &state.dynamic_models,
                state.preferred_model_selection(),
                &state.current_provider,
                &state.current_model,
                &state.custom_providers,
            )?;
        }

        if !inline_enabled {
            loop {
                match select_model_with_ratatui_list(
                    options,
                    current_reasoning,
                    &state.dynamic_models,
                    &state.custom_providers,
                ) {
                    Ok(ModelSelectionListOutcome::Predefined(detail)) => {
                        match state.process_model_selection(renderer, detail)? {
                            ModelPickerProgress::Completed(result) => {
                                return Ok(ModelPickerStart::Completed {
                                    state,
                                    selection: result,
                                });
                            }
                            ModelPickerProgress::InProgress => {
                                return Ok(ModelPickerStart::InProgress(state));
                            }
                            ModelPickerProgress::Cancelled => {
                                renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                                return Ok(ModelPickerStart::InProgress(state));
                            }
                            ModelPickerProgress::Exit => {
                                renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                                return Ok(ModelPickerStart::InProgress(state));
                            }
                            ModelPickerProgress::NeedsRefresh => {
                                state
                                    .refresh_dynamic_models(renderer)
                                    .await
                                    .context("Failed to refresh local models")?;
                                continue;
                            }
                        }
                    }
                    Ok(ModelSelectionListOutcome::Manual) => {
                        state.plain_mode_active = true;
                        render_step_one_plain(
                            renderer,
                            options,
                            &state.dynamic_models,
                            &state.custom_providers,
                        )?;
                        prompt_custom_model_entry(renderer)?;
                        break;
                    }
                    Ok(ModelSelectionListOutcome::Cancelled) => {
                        state.plain_mode_active = true;
                        render_step_one_plain(
                            renderer,
                            options,
                            &state.dynamic_models,
                            &state.custom_providers,
                        )?;
                        prompt_custom_model_entry(renderer)?;
                        break;
                    }
                    Ok(ModelSelectionListOutcome::Refresh) => {
                        state
                            .refresh_dynamic_models(renderer)
                            .await
                            .context("Failed to refresh local models")?;
                        continue;
                    }
                    Err(err) => {
                        if err.is::<SelectionInterrupted>() {
                            return Err(err);
                        }
                        renderer.line(
                            MessageStyle::Info,
                            &format!(
                                "Interactive model picker unavailable ({}). Falling back to manual input.",
                                err
                            ),
                        )?;
                        state.plain_mode_active = true;
                        render_step_one_plain(
                            renderer,
                            options,
                            &state.dynamic_models,
                            &state.custom_providers,
                        )?;
                        prompt_custom_model_entry(renderer)?;
                        break;
                    }
                }
            }
        }

        Ok(ModelPickerStart::InProgress(state))
    }

    pub async fn refresh_dynamic_models(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        renderer.line(MessageStyle::Info, "Refreshing local model inventory...")?;
        self.dynamic_models = DynamicModelRegistry::load(
            self.options,
            self.workspace.as_deref(),
            self.vt_cfg.as_ref(),
        )
        .await;
        self.custom_providers = self
            .vt_cfg
            .as_ref()
            .map(|cfg| {
                cfg.custom_providers
                    .iter()
                    .map(selection_from_custom_provider)
                    .collect()
            })
            .unwrap_or_default();
        self.selection = None;
        self.selected_reasoning = None;
        self.selected_service_tier = None;
        self.pending_api_key = None;
        self.step = PickerStep::AwaitModel;
        if self.inline_enabled {
            render_step_one_inline(
                renderer,
                self.options,
                self.current_reasoning,
                &self.dynamic_models,
                self.preferred_model_selection(),
                &self.current_provider,
                &self.current_model,
                &self.custom_providers,
            )?;
        } else if self.plain_mode_active {
            render_step_one_plain(
                renderer,
                self.options,
                &self.dynamic_models,
                &self.custom_providers,
            )?;
            if matches!(self.step, PickerStep::AwaitModel) {
                prompt_custom_model_entry(renderer)?;
            }
        }
        Ok(())
    }

    pub async fn handle_input(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
        url_guard: crate::agent::runloop::unified::external_url_guard::ExternalUrlGuardContext<'_>,
    ) -> Result<ModelPickerProgress> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            renderer.line(
                MessageStyle::Error,
                "Please enter a value or type 'cancel'.",
            )?;
            return Ok(ModelPickerProgress::InProgress);
        }
        if is_cancel_command(trimmed) {
            if !self.inline_enabled {
                renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
            }
            return Ok(ModelPickerProgress::Cancelled);
        }

        if matches!(self.step, PickerStep::AwaitModel) && trimmed.eq_ignore_ascii_case("refresh") {
            return Ok(ModelPickerProgress::NeedsRefresh);
        }

        match self.step {
            PickerStep::AwaitModel => self.handle_model_selection(renderer, trimmed),
            PickerStep::AwaitReasoning => self.handle_reasoning(renderer, trimmed),
            PickerStep::AwaitServiceTier => self.handle_service_tier(renderer, trimmed),
            PickerStep::AwaitApiKey => self.handle_api_key(renderer, trimmed, url_guard).await,
        }
    }

    pub async fn persist_selection(
        &self,
        workspace: &std::path::Path,
        selection: &ModelSelectionResult,
    ) -> Result<VTCodeConfig> {
        config_persistence::persist_selection(workspace, selection).await
    }

    pub fn handle_list_selection(
        &mut self,
        renderer: &mut AnsiRenderer,
        choice: InlineListSelection,
    ) -> Result<ModelPickerProgress> {
        match self.step {
            PickerStep::AwaitModel => match choice {
                InlineListSelection::Model(index) => {
                    let Some(option) = self.options.get(index) else {
                        renderer.line(
                            MessageStyle::Error,
                            "Unable to locate the selected model option.",
                        )?;
                        return Ok(ModelPickerProgress::InProgress);
                    };
                    let detail = selection_from_option(option);
                    self.process_model_selection(renderer, detail)
                }
                InlineListSelection::DynamicModel(entry_index) => {
                    let Some(detail) = self.dynamic_models.dynamic_detail(entry_index) else {
                        renderer.line(
                            MessageStyle::Error,
                            "Unable to locate the selected dynamic model.",
                        )?;
                        return Ok(ModelPickerProgress::InProgress);
                    };
                    self.process_model_selection(renderer, detail)
                }
                InlineListSelection::CustomProvider(entry_index) => {
                    let Some(detail) = self.custom_providers.get(entry_index).cloned() else {
                        renderer.line(
                            MessageStyle::Error,
                            "Unable to locate the selected custom provider.",
                        )?;
                        return Ok(ModelPickerProgress::InProgress);
                    };
                    self.process_model_selection(renderer, detail)
                }
                InlineListSelection::RefreshDynamicModels => Ok(ModelPickerProgress::NeedsRefresh),
                InlineListSelection::CustomModel => {
                    prompt_custom_model_entry(renderer)?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::Reasoning(_) => {
                    renderer.line(
                        MessageStyle::Error,
                        "Select a model before configuring reasoning effort.",
                    )?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::DisableReasoning => {
                    renderer.line(
                        MessageStyle::Error,
                        "Select a model before disabling reasoning.",
                    )?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::OpenAIServiceTier(_) => {
                    renderer.line(
                        MessageStyle::Error,
                        "Select a model before choosing a service tier.",
                    )?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::Theme(_) => {
                    renderer.line(MessageStyle::Error, CLOSE_THEME_MESSAGE)?;
                    Ok(ModelPickerProgress::InProgress)
                }
                _ => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitReasoning => match choice {
                InlineListSelection::Reasoning(level) => {
                    self.apply_reasoning_choice(renderer, from_tui_reasoning(level))
                }
                InlineListSelection::DisableReasoning => self.apply_reasoning_off_choice(renderer),
                InlineListSelection::OpenAIServiceTier(_) => {
                    renderer.line(
                        MessageStyle::Error,
                        "Reasoning selection is active. Choose a reasoning level or press Esc to cancel.",
                    )?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::CustomModel
                | InlineListSelection::Model(_)
                | InlineListSelection::RefreshDynamicModels
                | InlineListSelection::DynamicModel(_)
                | InlineListSelection::CustomProvider(_)
                | InlineListSelection::ToolApproval(_)
                | InlineListSelection::ToolApprovalDenyOnce
                | InlineListSelection::ToolApprovalSession
                | InlineListSelection::ToolApprovalPermanent => {
                    renderer.line(
                        MessageStyle::Error,
                        "Reasoning selection is active. Choose a reasoning level or press Esc to cancel.",
                    )?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::Theme(_) => {
                    renderer.line(MessageStyle::Error, CLOSE_THEME_MESSAGE)?;
                    Ok(ModelPickerProgress::InProgress)
                }
                _ => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitServiceTier => match choice {
                InlineListSelection::OpenAIServiceTier(choice) => {
                    let service_tier = match choice {
                        OpenAIServiceTierChoice::ProjectDefault => None,
                        OpenAIServiceTierChoice::Flex => Some(OpenAIServiceTier::Flex),
                        OpenAIServiceTierChoice::Priority => Some(OpenAIServiceTier::Priority),
                    };
                    self.apply_service_tier_choice(renderer, service_tier)
                }
                InlineListSelection::CustomModel
                | InlineListSelection::Model(_)
                | InlineListSelection::RefreshDynamicModels
                | InlineListSelection::DynamicModel(_)
                | InlineListSelection::CustomProvider(_)
                | InlineListSelection::Reasoning(_)
                | InlineListSelection::DisableReasoning
                | InlineListSelection::ToolApproval(_)
                | InlineListSelection::ToolApprovalDenyOnce
                | InlineListSelection::ToolApprovalSession
                | InlineListSelection::ToolApprovalPermanent => {
                    renderer.line(
                        MessageStyle::Error,
                        "Service tier selection is active. Choose a value or press Esc to cancel.",
                    )?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::Theme(_) => {
                    renderer.line(MessageStyle::Error, CLOSE_THEME_MESSAGE)?;
                    Ok(ModelPickerProgress::InProgress)
                }
                _ => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitApiKey => {
                renderer.line(
                    MessageStyle::Info,
                    "Enter the API key in the input field or type 'skip'.",
                )?;
                Ok(ModelPickerProgress::InProgress)
            }
        }
    }

    fn handle_model_selection(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        let selection = match parse_model_selection(self.options, input, self.vt_cfg.as_ref()) {
            Ok(detail) => detail,
            Err(err) => {
                renderer.line(MessageStyle::Error, &err.to_string())?;
                renderer.line(
                    MessageStyle::Info,
                    "Try again with '<provider> <model-id>'.",
                )?;
                return Ok(ModelPickerProgress::InProgress);
            }
        };

        self.process_model_selection(renderer, selection)
    }

    fn preferred_model_selection(&self) -> Option<InlineListSelection> {
        let provider_key = self.current_provider.trim().to_ascii_lowercase();
        let model_key = self.current_model.trim();
        if provider_key.is_empty() || model_key.is_empty() {
            return None;
        }

        if let Ok(provider) = Provider::from_str(provider_key.as_str()) {
            if let Some(index) = find_option_index(provider, model_key) {
                return Some(InlineListSelection::Model(index));
            }
            for entry_index in self.dynamic_models.indexes_for(provider) {
                if let Some(detail) = self.dynamic_models.detail(*entry_index)
                    && detail.model_id.eq_ignore_ascii_case(model_key)
                {
                    return Some(InlineListSelection::DynamicModel(*entry_index));
                }
            }
        }

        for (entry_index, detail) in self.custom_providers.iter().enumerate() {
            if detail.provider_key.eq_ignore_ascii_case(&provider_key)
                && detail.model_id.eq_ignore_ascii_case(model_key)
            {
                return Some(InlineListSelection::CustomProvider(entry_index));
            }
        }

        None
    }
}

pub(crate) async fn pick_subagent_model(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace: Option<&Path>,
    current_model: &str,
    current_reasoning_effort: Option<&str>,
) -> Result<Option<SubagentModelSelection>> {
    if !renderer.supports_inline_ui() {
        renderer.line(
            MessageStyle::Info,
            "Interactive subagent model selection requires inline UI.",
        )?;
        return Ok(None);
    }

    let mut dynamic_models =
        DynamicModelRegistry::load(MODEL_OPTIONS.as_slice(), workspace, vt_cfg).await;
    loop {
        let Some(target) = select_subagent_model_target(
            handle,
            session,
            ctrl_c_state,
            ctrl_c_notify,
            &dynamic_models,
            current_model,
        )
        .await?
        else {
            return Ok(None);
        };

        let target = match target {
            SubagentModelChoice::Target(target) => target,
            SubagentModelChoice::Refresh => {
                renderer.line(MessageStyle::Info, "Refreshing local model inventory...")?;
                dynamic_models =
                    DynamicModelRegistry::load(MODEL_OPTIONS.as_slice(), workspace, vt_cfg).await;
                continue;
            }
            SubagentModelChoice::Manual => {
                let Some(target) = prompt_subagent_model_id(
                    renderer,
                    handle,
                    session,
                    ctrl_c_state,
                    ctrl_c_notify,
                )
                .await?
                else {
                    return Ok(None);
                };
                target
            }
        };

        let Some(selection) = select_subagent_reasoning(
            handle,
            session,
            ctrl_c_state,
            ctrl_c_notify,
            target,
            current_reasoning_effort,
        )
        .await?
        else {
            return Ok(None);
        };

        return Ok(Some(selection));
    }
}

#[derive(Clone, Debug)]
enum SubagentModelChoice {
    Target(SubagentModelTarget),
    Refresh,
    Manual,
}

async fn select_subagent_model_target(
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    dynamic_models: &DynamicModelRegistry,
    current_model: &str,
) -> Result<Option<SubagentModelChoice>> {
    let mut items = Vec::new();
    for (shortcut, description) in subagent_model_shortcuts() {
        items.push(InlineListItem {
            title: (*shortcut).to_string(),
            subtitle: Some((*description).to_string()),
            badge: Some("Shortcut".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{SUBAGENT_MODEL_ACTION_PREFIX}shortcut:{shortcut}"
            ))),
            search_value: Some(format!(
                "{shortcut} shortcut alias delegated model {description}"
            )),
        });
    }

    for (index, option) in MODEL_OPTIONS.iter().enumerate() {
        let current_provider = if current_model.eq_ignore_ascii_case(option.id) {
            option.provider.as_ref()
        } else {
            ""
        };
        items.push(InlineListItem {
            title: option.display.to_string(),
            subtitle: Some(format!(
                "{} • {}",
                option.provider.label(),
                static_model_subtitle(option, current_provider, current_model)
            )),
            badge: Some(option.provider.label().to_string()),
            indent: 0,
            selection: Some(InlineListSelection::Model(index)),
            search_value: Some(model_search_value(
                option.provider,
                option.display,
                option.id,
                Some(option.description),
                &static_model_search_terms(option.model, option.supports_reasoning),
            )),
        });
    }

    for entry_index in parseable_subagent_dynamic_indexes(dynamic_models) {
        let Some(detail) = dynamic_models.detail(entry_index) else {
            continue;
        };
        let Some(provider) = detail.provider_enum else {
            continue;
        };
        let current_provider = if current_model.eq_ignore_ascii_case(&detail.model_id) {
            provider.as_ref()
        } else {
            ""
        };
        items.push(InlineListItem {
            title: detail.model_display.clone(),
            subtitle: Some(format!(
                "{} • {}",
                provider.label(),
                dynamic_model_subtitle(
                    provider,
                    &detail.model_id,
                    detail.reasoning_supported,
                    current_provider,
                    current_model,
                )
            )),
            badge: Some(provider.label().to_string()),
            indent: 0,
            selection: Some(InlineListSelection::DynamicModel(entry_index)),
            search_value: Some(model_search_value(
                provider,
                &detail.model_display,
                &detail.model_id,
                None,
                &[provider.label().to_string(), "dynamic".to_string()],
            )),
        });
    }

    items.push(InlineListItem {
        title: "Refresh local models".to_string(),
        subtitle: Some(
            "Re-query dynamic model inventories without changing workspace config.".to_string(),
        ),
        badge: Some("Refresh".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::RefreshDynamicModels),
        search_value: Some("refresh dynamic local models".to_string()),
    });
    items.push(InlineListItem {
        title: "Enter exact model id".to_string(),
        subtitle: Some(
            "Provide a concrete VT Code model id such as `gpt-5.4` or `claude-sonnet-4-6`."
                .to_string(),
        ),
        badge: Some("Manual".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::CustomModel),
        search_value: Some("manual exact model id".to_string()),
    });

    let selected = preferred_subagent_model_selection(dynamic_models, current_model)
        .or_else(|| items.first().and_then(|item| item.selection.clone()));
    handle.show_list_modal(
        "Subagent model".to_string(),
        vec![
            "Choose a shortcut alias or a concrete VT Code model id for this subagent.".to_string(),
            "This picker only stores `model` and `reasoning_effort`; it never prompts for API keys or service tier.".to_string(),
        ],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search subagent models".to_string(),
            placeholder: Some("shortcut, provider, model id".to_string()),
        }),
    );

    let Some(selection) =
        wait_for_inline_list_selection(handle, session, ctrl_c_state, ctrl_c_notify).await?
    else {
        return Ok(None);
    };

    let choice = match selection {
        InlineListSelection::ConfigAction(action) => {
            if let Some(shortcut) =
                action.strip_prefix(&format!("{SUBAGENT_MODEL_ACTION_PREFIX}shortcut:"))
            {
                SubagentModelChoice::Target(SubagentModelTarget::Shortcut {
                    model: shortcut.to_string(),
                })
            } else {
                return Ok(None);
            }
        }
        InlineListSelection::Model(index) => {
            let option = MODEL_OPTIONS
                .get(index)
                .ok_or_else(|| anyhow!("Unable to locate the selected model option"))?;
            SubagentModelChoice::Target(SubagentModelTarget::Concrete(selection_from_option(
                option,
            )))
        }
        InlineListSelection::DynamicModel(index) => {
            let detail = dynamic_models
                .dynamic_detail(index)
                .ok_or_else(|| anyhow!("Unable to locate the selected dynamic model"))?;
            SubagentModelChoice::Target(SubagentModelTarget::Concrete(detail))
        }
        InlineListSelection::RefreshDynamicModels => SubagentModelChoice::Refresh,
        InlineListSelection::CustomModel => SubagentModelChoice::Manual,
        _ => return Ok(None),
    };

    Ok(Some(choice))
}

async fn select_subagent_reasoning(
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    target: SubagentModelTarget,
    current_reasoning_effort: Option<&str>,
) -> Result<Option<SubagentModelSelection>> {
    let model = target.model().to_string();
    if !target.supports_reasoning() {
        return Ok(Some(SubagentModelSelection {
            model,
            reasoning_effort: None,
        }));
    }

    let current_reasoning_effort = normalized_subagent_reasoning(&target, current_reasoning_effort);
    let current_label = current_reasoning_effort
        .as_deref()
        .and_then(ReasoningEffortLevel::parse)
        .map(reasoning_level_label)
        .unwrap_or("unset");
    let mut items = vec![
        InlineListItem {
            title: format!("Keep current ({current_label})"),
            subtitle: Some("Retain the current reasoning override for this subagent.".to_string()),
            badge: Some("Current".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{SUBAGENT_REASONING_ACTION_PREFIX}keep"
            ))),
            search_value: Some("keep current reasoning".to_string()),
        },
        InlineListItem {
            title: "Unset reasoning override".to_string(),
            subtitle: Some(
                "Do not store a `reasoning_effort` override for this subagent.".to_string(),
            ),
            badge: Some("Unset".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{SUBAGENT_REASONING_ACTION_PREFIX}unset"
            ))),
            search_value: Some("unset clear reasoning".to_string()),
        },
    ];

    for level in subagent_reasoning_levels(target.model(), target.supports_reasoning()) {
        items.push(InlineListItem {
            title: reasoning_level_label(level).to_string(),
            subtitle: Some(reasoning_level_description(level).to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{SUBAGENT_REASONING_ACTION_PREFIX}{}",
                level.as_str()
            ))),
            search_value: Some(format!(
                "{} {}",
                level.as_str(),
                reasoning_level_label(level)
            )),
        });
    }

    handle.show_list_modal(
        "Subagent reasoning".to_string(),
        vec![format!(
            "Choose the reasoning override for `{}`. Esc cancels the picker.",
            target.model()
        )],
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{SUBAGENT_REASONING_ACTION_PREFIX}keep"
        ))),
        Some(InlineListSearchConfig {
            label: "Search reasoning".to_string(),
            placeholder: Some("keep, unset, high".to_string()),
        }),
    );

    let Some(selection) =
        wait_for_inline_list_selection(handle, session, ctrl_c_state, ctrl_c_notify).await?
    else {
        return Ok(None);
    };

    let reasoning_choice = match selection {
        InlineListSelection::ConfigAction(action)
            if action == format!("{SUBAGENT_REASONING_ACTION_PREFIX}keep") =>
        {
            SubagentReasoningChoice::KeepCurrent
        }
        InlineListSelection::ConfigAction(action)
            if action == format!("{SUBAGENT_REASONING_ACTION_PREFIX}unset") =>
        {
            SubagentReasoningChoice::Unset
        }
        InlineListSelection::ConfigAction(action) => {
            let level_key = action
                .strip_prefix(SUBAGENT_REASONING_ACTION_PREFIX)
                .ok_or_else(|| anyhow!("Unknown subagent reasoning selection"))?;
            let level = ReasoningEffortLevel::parse(level_key)
                .ok_or_else(|| anyhow!("Unknown reasoning effort level `{level_key}`"))?;
            SubagentReasoningChoice::Explicit(level)
        }
        _ => return Ok(None),
    };

    let reasoning_effort = match reasoning_choice {
        SubagentReasoningChoice::KeepCurrent => current_reasoning_effort,
        SubagentReasoningChoice::Unset => None,
        SubagentReasoningChoice::Explicit(level) => Some(level.as_str().to_string()),
    };

    Ok(Some(SubagentModelSelection {
        model,
        reasoning_effort,
    }))
}

async fn prompt_subagent_model_id(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<Option<SubagentModelTarget>> {
    loop {
        let outcome = show_wizard_modal_and_wait(
            handle,
            session,
            "Subagent model id".to_string(),
            vec![WizardStep {
                title: "Model id".to_string(),
                question: "Enter a concrete VT Code model id. Shortcut aliases such as `inherit` and `small` are available in the list view.".to_string(),
                items: vec![InlineListItem {
                    title: "Enter a model id".to_string(),
                    subtitle: Some(
                        "Press Tab to type inline, then Enter to confirm the model id."
                            .to_string(),
                    ),
                    badge: Some("Input".to_string()),
                    indent: 0,
                    selection: Some(InlineListSelection::RequestUserInputAnswer {
                        question_id: SUBAGENT_MODEL_PROMPT_ID.to_string(),
                        selected: vec![],
                        other: Some(String::new()),
                    }),
                    search_value: Some("manual model id".to_string()),
                }],
                completed: false,
                answer: None,
                allow_freeform: true,
                freeform_label: Some("Model id".to_string()),
                freeform_placeholder: Some("gpt-5.4".to_string()),
                freeform_default: None,
            }],
            0,
            None,
            WizardModalMode::MultiStep,
            ctrl_c_state,
            ctrl_c_notify,
        )
        .await?;

        let Some(selection) = (match outcome {
            WizardModalOutcome::Submitted(selections) => selections.into_iter().next(),
            WizardModalOutcome::Cancelled { .. } => None,
        }) else {
            return Ok(None);
        };

        let InlineListSelection::RequestUserInputAnswer {
            other, selected, ..
        } = selection
        else {
            return Ok(None);
        };
        let raw_value = other
            .or_else(|| selected.first().cloned())
            .unwrap_or_default();
        let trimmed = raw_value.trim();
        if trimmed.is_empty() {
            continue;
        }

        let model_id = match trimmed.parse::<ModelId>() {
            Ok(model_id) => model_id,
            Err(_) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("`{trimmed}` is not a recognized VT Code model id."),
                )?;
                continue;
            }
        };
        let detail = parse_model_selection(
            MODEL_OPTIONS.as_slice(),
            &format!("{} {}", model_id.provider(), model_id.as_str()),
            None,
        )?;
        return Ok(Some(SubagentModelTarget::Concrete(detail)));
    }
}

fn preferred_subagent_model_selection(
    dynamic_models: &DynamicModelRegistry,
    current_model: &str,
) -> Option<InlineListSelection> {
    let current_trimmed = current_model.trim();
    if current_trimmed.is_empty() {
        return None;
    }
    if let Some(shortcut) = canonical_subagent_shortcut(current_trimmed) {
        return Some(InlineListSelection::ConfigAction(format!(
            "{SUBAGENT_MODEL_ACTION_PREFIX}shortcut:{shortcut}"
        )));
    }
    if let Ok(model_id) = current_trimmed.parse::<ModelId>()
        && let Some(index) = find_option_index(model_id.provider(), model_id.as_str())
    {
        return Some(InlineListSelection::Model(index));
    }
    parseable_subagent_dynamic_indexes(dynamic_models)
        .into_iter()
        .find_map(|index| {
            dynamic_models
                .detail(index)
                .filter(|detail| detail.model_id.eq_ignore_ascii_case(current_trimmed))
                .map(|_| InlineListSelection::DynamicModel(index))
        })
}

fn normalized_subagent_reasoning(
    target: &SubagentModelTarget,
    current_reasoning_effort: Option<&str>,
) -> Option<String> {
    let level = parse_subagent_reasoning_effort(current_reasoning_effort)?;
    subagent_supports_reasoning_level(target, level).then(|| level.as_str().to_string())
}

fn is_subagent_shortcut(model: &str) -> bool {
    canonical_subagent_shortcut(model).is_some()
}

fn canonical_subagent_shortcut(model: &str) -> Option<&'static str> {
    subagent_model_shortcuts()
        .iter()
        .find(|(shortcut, _)| shortcut.eq_ignore_ascii_case(model.trim()))
        .map(|(shortcut, _)| *shortcut)
}

fn subagent_model_shortcuts() -> &'static [(&'static str, &'static str)] {
    &SUBAGENT_SHORTCUTS
}

fn parse_subagent_reasoning_effort(
    current_reasoning_effort: Option<&str>,
) -> Option<ReasoningEffortLevel> {
    current_reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(ReasoningEffortLevel::parse)
}

fn parseable_subagent_dynamic_indexes(dynamic_models: &DynamicModelRegistry) -> Vec<usize> {
    dynamic_models
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, detail)| detail.model_id.parse::<ModelId>().ok().map(|_| index))
        .collect()
}

fn subagent_reasoning_levels(model: &str, supports_reasoning: bool) -> Vec<ReasoningEffortLevel> {
    if !supports_reasoning {
        return Vec::new();
    }

    let mut levels = vec![
        ReasoningEffortLevel::None,
        ReasoningEffortLevel::Minimal,
        ReasoningEffortLevel::Low,
        ReasoningEffortLevel::Medium,
        ReasoningEffortLevel::High,
    ];
    if !is_subagent_shortcut(model) && supports_xhigh_reasoning(model) {
        levels.push(ReasoningEffortLevel::XHigh);
    }
    levels
}

fn subagent_supports_reasoning_level(
    target: &SubagentModelTarget,
    level: ReasoningEffortLevel,
) -> bool {
    if !target.supports_reasoning() {
        return false;
    }

    match level {
        ReasoningEffortLevel::None
        | ReasoningEffortLevel::Minimal
        | ReasoningEffortLevel::Low
        | ReasoningEffortLevel::Medium
        | ReasoningEffortLevel::High => true,
        ReasoningEffortLevel::XHigh => {
            !is_subagent_shortcut(target.model()) && supports_xhigh_reasoning(target.model())
        }
    }
}

async fn wait_for_inline_list_selection(
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<Option<InlineListSelection>> {
    let outcome =
        wait_for_overlay_submission(handle, session, ctrl_c_state, ctrl_c_notify, |submission| {
            match submission {
                TransientSubmission::Selection(selection) => Some(selection),
                _ => None,
            }
        })
        .await?;

    handle.close_modal();
    handle.force_redraw();
    task::yield_now().await;

    Ok(match outcome {
        OverlayWaitOutcome::Submitted(selection) => Some(selection),
        OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted
        | OverlayWaitOutcome::Exit => None,
    })
}

impl SubagentModelTarget {
    fn model(&self) -> &str {
        match self {
            Self::Shortcut { model } => model,
            Self::Concrete(detail) => &detail.model_id,
        }
    }

    fn supports_reasoning(&self) -> bool {
        match self {
            Self::Shortcut { .. } => true,
            Self::Concrete(detail) => detail.reasoning_supported,
        }
    }
}

#[cfg(test)]
mod tests;
