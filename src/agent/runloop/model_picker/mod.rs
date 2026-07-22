//! Model picker — interactive provider/model/reasoning/service-tier/API-key selection.
//!
//! ## Module map (interface guard rails)
//!
//! | Module | Responsibility | Public surface |
//! |---|---|--|
//! | `options` | Static model catalog + override-aware option list | `ModelOption`, `MODEL_OPTIONS`, `build_filtered_options`, `find_option_index`, `option_indexes_for_provider` |
//! | `selection` | `SelectionDetail` construction from options, dynamic entries, custom providers, or raw input | `SelectionDetail`, `parse_model_selection`, `selection_from_option`, `selection_from_dynamic`, `selections_from_custom_provider` |
//! | `dynamic_models` | Dynamic model inventory (Ollama, LM Studio, LlamaCpp, Copilot) with cache | `DynamicModelRegistry` |
//! | `rendering` | Step-one list rendering (inline + plain) | `render_step_one_inline`, `render_step_one_plain`, `custom_provider_subtitle`, `static_model_subtitle`, `dynamic_model_subtitle` |
//! | `rendering/prompts` | Per-step prompt text (reasoning, API key, service tier, MiMo auth) | `render_reasoning_inline`, `prompt_reasoning_plain`, `show_secure_api_modal`, `prompt_api_key_plain`, `render_service_tier_inline`, `prompt_service_tier_plain`, `render_mimo_auth_method_inline`, `prompt_mimo_auth_method_plain` |
//! | `interaction` | Ratatui interactive list selection for reasoning, service tier, model list | `select_model_with_ratatui_list`, `select_reasoning_with_ratatui`, `select_service_tier_with_ratatui` |
//! | `state_machine` | Step-transition logic: model → reasoning → MiMo auth → service tier → API key | `process_model_selection`, `handle_reasoning`, `handle_mimo_auth_method`, `handle_service_tier`, `handle_api_key`, `build_result` |
//! | `config_persistence` | Persisting completed selection to `vtcode.toml` | `persist_selection` |
//! | `lightweight_palette` | Lightweight model palette view | `LightweightModelPaletteView`, `prepare_lightweight_model_palette_view` |
//!
//! ## State machine contract
//!
//! `ModelPickerState` is the single owner of picker state. Step transitions follow this order:
//!
//! ```text
//! AwaitModel -> AwaitReasoning -> AwaitMiMoAuthMethod -> AwaitServiceTier -> AwaitApiKey -> Completed
//! ```
//!
//! Each step returns `ModelPickerProgress::InProgress` to stay in the picker,
//! `Completed(result)` to finish, `Cancelled` to abort, or `NeedsRefresh` to reload dynamic models.
//!
//! ## Guard rails
//!
//! - `SelectionDetail` is the canonical selection representation — all entry points (static option, dynamic entry, custom provider, raw input) must produce it.
//! - `ModelSelectionResult` is the persisted output — it is built by `build_result()` from the current step state.
//! - Rendering functions are pure (no state mutation) and receive `&SelectionDetail` + current value.
//! - State machine functions mutate `ModelPickerState` and return `ModelPickerProgress`.

use anyhow::{Context, Result, anyhow};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_config::{MiMoAuthMethod, OpenAIServiceTier, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListSelection, OpenAIServiceTierChoice, reasoning_from_selection_string};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::ui::interactive_list::SelectionInterrupted;

use crate::agent::runloop::unified::state::CtrlCState;
use interaction::{
    ModelSelectionListOutcome, select_model_with_ratatui_list, select_reasoning_with_ratatui,
    select_service_tier_with_ratatui,
};
use options::{
    ModelOption, build_filtered_options, find_option_index, picker_provider_order, picker_provider_order_with_whitelist,
};
use rendering::{
    CLOSE_THEME_MESSAGE, prompt_api_key_plain, prompt_custom_model_entry, prompt_mimo_auth_method_plain,
    prompt_reasoning_plain, prompt_service_tier_plain, render_mimo_auth_method_inline, render_reasoning_inline,
    render_service_tier_inline, render_step_one_inline, render_step_one_plain, show_secure_api_modal,
};
#[cfg(test)]
use rendering::{dynamic_model_subtitle, model_search_value, static_model_search_terms, static_model_subtitle};
use selection::{
    ExistingKey, ReasoningChoice, SelectionDetail, ServiceTierChoice, is_cancel_command, parse_model_selection,
    selection_from_option, selections_from_custom_provider,
};
#[cfg(test)]
use selection::{
    reasoning_level_description, reasoning_level_label, service_tier_label, supports_gpt5_none_reasoning,
    supports_max_reasoning, supports_xhigh_reasoning,
};
#[cfg(test)]
use subagent::{
    SubagentModelTarget, normalized_subagent_reasoning, parseable_subagent_dynamic_indexes,
    preferred_subagent_model_selection, subagent_model_shortcuts, subagent_reasoning_levels,
};

mod config_persistence;
mod dynamic_models;
mod interaction;
mod lightweight_palette;
mod options;
mod rendering;
mod selection;
mod state_machine;
mod subagent;

pub(crate) use self::config_persistence::persist_lightweight_selection;
pub(crate) use self::dynamic_models::DynamicModelRegistry;
#[cfg(test)]
pub(crate) use self::lightweight_palette::build_lightweight_model_palette_view;
pub(crate) use self::lightweight_palette::{LightweightModelPaletteView, prepare_lightweight_model_palette_view};
pub(crate) use selection::ModelSelectionResult;
pub(super) use vtcode_config::read_workspace_env_value as read_workspace_env;

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::enum_variant_names)]
enum PickerStep {
    AwaitModel,
    AwaitReasoning,
    AwaitMiMoAuthMethod,
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

pub(crate) struct ModelPickerState {
    options: Cow<'static, [ModelOption]>,
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
    selected_mimo_auth: Option<MiMoAuthMethod>,
    pending_api_key: Option<String>,
    workspace: Option<PathBuf>,
    ctrl_c_state: Option<Arc<CtrlCState>>,
    ctrl_c_notify: Option<Arc<Notify>>,
    dynamic_models: DynamicModelRegistry,
    plain_mode_active: bool,
    provider_order: Vec<Provider>,
}

pub(crate) enum ModelPickerStart {
    Completed {
        state: ModelPickerState,
        selection: ModelSelectionResult,
    },
    InProgress(ModelPickerState),
    Exit,
}

impl ModelPickerState {
    #[expect(clippy::new_ret_no_self)]
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
        let options = build_filtered_options(vt_cfg.as_ref());
        let provider_order = vt_cfg
            .as_ref()
            .map(|cfg| picker_provider_order_with_whitelist(&cfg.providers_whitelist))
            .unwrap_or_else(|| picker_provider_order().to_vec());
        let inline_enabled = renderer.supports_inline_ui();
        let dynamic_models = DynamicModelRegistry::load(&options, workspace.as_deref(), vt_cfg.as_ref()).await;
        let custom_providers = vt_cfg
            .as_ref()
            .map(|cfg| {
                cfg.custom_providers
                    .iter()
                    .filter(|cp| {
                        cfg.providers_whitelist.is_empty()
                            || cfg.providers_whitelist.iter().any(|w| w.eq_ignore_ascii_case(&cp.name))
                    })
                    .flat_map(selections_from_custom_provider)
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
            selected_mimo_auth: None,
            pending_api_key: None,
            workspace,
            ctrl_c_state,
            ctrl_c_notify,
            dynamic_models,
            plain_mode_active: false,
            provider_order,
        };

        if inline_enabled {
            render_step_one_inline(
                renderer,
                &state.options,
                current_reasoning,
                &state.dynamic_models,
                state.preferred_model_selection(),
                &state.current_provider,
                &state.current_model,
                &state.custom_providers,
                &state.provider_order,
            )?;
        }

        if !inline_enabled {
            loop {
                match select_model_with_ratatui_list(
                    &state.options,
                    current_reasoning,
                    &state.dynamic_models,
                    &state.custom_providers,
                    &state.provider_order,
                ) {
                    Ok(ModelSelectionListOutcome::Predefined(detail)) => {
                        match state.process_model_selection(renderer, detail)? {
                            ModelPickerProgress::Completed(result) => {
                                return Ok(ModelPickerStart::Completed { state, selection: result });
                            }
                            ModelPickerProgress::InProgress => {
                                return Ok(ModelPickerStart::InProgress(state));
                            }
                            ModelPickerProgress::Cancelled => {
                                renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
                                return Ok(ModelPickerStart::InProgress(state));
                            }
                            ModelPickerProgress::Exit => {
                                renderer.line(MessageStyle::Info, "Exiting model picker.")?;
                                return Ok(ModelPickerStart::Exit);
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
                            &state.options,
                            &state.dynamic_models,
                            &state.custom_providers,
                            &state.current_provider,
                            &state.provider_order,
                        )?;
                        prompt_custom_model_entry(renderer)?;
                        break;
                    }
                    Ok(ModelSelectionListOutcome::Cancelled) => {
                        state.plain_mode_active = true;
                        render_step_one_plain(
                            renderer,
                            &state.options,
                            &state.dynamic_models,
                            &state.custom_providers,
                            &state.current_provider,
                            &state.provider_order,
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
                            &format!("Interactive model picker unavailable ({err}). Falling back to manual input."),
                        )?;
                        state.plain_mode_active = true;
                        render_step_one_plain(
                            renderer,
                            &state.options,
                            &state.dynamic_models,
                            &state.custom_providers,
                            &state.current_provider,
                            &state.provider_order,
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
        self.dynamic_models =
            DynamicModelRegistry::load(&self.options, self.workspace.as_deref(), self.vt_cfg.as_ref()).await;
        self.custom_providers = self
            .vt_cfg
            .as_ref()
            .map(|cfg| cfg.custom_providers.iter().flat_map(selections_from_custom_provider).collect())
            .unwrap_or_default();
        self.selection = None;
        self.selected_reasoning = None;
        self.selected_service_tier = None;
        self.selected_mimo_auth = None;
        self.pending_api_key = None;
        self.step = PickerStep::AwaitModel;
        if self.inline_enabled {
            render_step_one_inline(
                renderer,
                &self.options,
                self.current_reasoning,
                &self.dynamic_models,
                self.preferred_model_selection(),
                &self.current_provider,
                &self.current_model,
                &self.custom_providers,
                &self.provider_order,
            )?;
        } else if self.plain_mode_active {
            render_step_one_plain(
                renderer,
                &self.options,
                &self.dynamic_models,
                &self.custom_providers,
                &self.current_provider,
                &self.provider_order,
            )?;
            prompt_custom_model_entry(renderer)?;
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
            renderer.line(MessageStyle::Error, "Please enter a value or type 'cancel'.")?;
            return Ok(ModelPickerProgress::InProgress);
        }
        if is_cancel_command(trimmed) {
            if !self.inline_enabled {
                renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
            }
            return Ok(ModelPickerProgress::Cancelled);
        }

        if trimmed.eq_ignore_ascii_case("refresh") {
            return Ok(ModelPickerProgress::NeedsRefresh);
        }

        match self.step {
            PickerStep::AwaitModel => self.handle_model_selection(renderer, trimmed),
            PickerStep::AwaitReasoning => self.handle_reasoning(renderer, trimmed),
            PickerStep::AwaitMiMoAuthMethod => self.handle_mimo_auth_method(renderer, trimmed),
            PickerStep::AwaitServiceTier => self.handle_service_tier(renderer, trimmed),
            PickerStep::AwaitApiKey => self.handle_api_key(renderer, trimmed, url_guard).await,
        }
    }

    pub async fn persist_selection(&self, workspace: &Path, selection: &ModelSelectionResult) -> Result<VTCodeConfig> {
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
                        renderer.line(MessageStyle::Error, "Unable to locate the selected model option.")?;
                        return Ok(ModelPickerProgress::InProgress);
                    };
                    let detail = selection_from_option(option);
                    self.process_model_selection(renderer, detail)
                }
                InlineListSelection::DynamicModel(entry_index) => {
                    let Some(detail) = self.dynamic_models.dynamic_detail(entry_index) else {
                        renderer.line(MessageStyle::Error, "Unable to locate the selected dynamic model.")?;
                        return Ok(ModelPickerProgress::InProgress);
                    };
                    self.process_model_selection(renderer, detail)
                }
                InlineListSelection::CustomProvider(entry_index) => {
                    let Some(detail) = self.custom_providers.get(entry_index).cloned() else {
                        renderer.line(MessageStyle::Error, "Unable to locate the selected custom provider.")?;
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
                    renderer.line(MessageStyle::Error, "Select a model before configuring reasoning effort.")?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::DisableReasoning => {
                    renderer.line(MessageStyle::Error, "Select a model before disabling reasoning.")?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::OpenAIServiceTier(_) => {
                    renderer.line(MessageStyle::Error, "Select a model before choosing a service tier.")?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::Theme(_) => {
                    renderer.line(MessageStyle::Error, CLOSE_THEME_MESSAGE)?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::ConfigAction(_)
                | InlineListSelection::SlashCommand(_)
                | InlineListSelection::Session(_)
                | InlineListSelection::SessionForkMode { .. }
                | InlineListSelection::FileConflictReload
                | InlineListSelection::FileConflictViewDiff
                | InlineListSelection::FileConflictAbort
                | InlineListSelection::SessionLimitIncrease(_)
                | InlineListSelection::RewindCheckpoint(_)
                | InlineListSelection::RewindAction(_)
                | InlineListSelection::AskUserChoice { .. }
                | InlineListSelection::RequestUserInputAnswer { .. }
                | InlineListSelection::PlanApprovalExecute
                | InlineListSelection::PlanApprovalEditPlan
                | InlineListSelection::PlanApprovalAutoAccept
                | InlineListSelection::PlanApprovalSwitchBuild
                | InlineListSelection::PlanApprovalSwitchAuto
                | InlineListSelection::ToolApproval(_)
                | InlineListSelection::ToolApprovalDenyOnce
                | InlineListSelection::ToolApprovalSession
                | InlineListSelection::ToolApprovalPermanent
                | InlineListSelection::ToolApprovalEnable => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitReasoning => match choice {
                InlineListSelection::Reasoning(level) => {
                    self.apply_reasoning_choice(renderer, reasoning_from_selection_string(&level))
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
                InlineListSelection::ConfigAction(_)
                | InlineListSelection::SlashCommand(_)
                | InlineListSelection::Session(_)
                | InlineListSelection::SessionForkMode { .. }
                | InlineListSelection::FileConflictReload
                | InlineListSelection::FileConflictViewDiff
                | InlineListSelection::FileConflictAbort
                | InlineListSelection::SessionLimitIncrease(_)
                | InlineListSelection::RewindCheckpoint(_)
                | InlineListSelection::RewindAction(_)
                | InlineListSelection::AskUserChoice { .. }
                | InlineListSelection::RequestUserInputAnswer { .. }
                | InlineListSelection::PlanApprovalExecute
                | InlineListSelection::PlanApprovalEditPlan
                | InlineListSelection::PlanApprovalAutoAccept
                | InlineListSelection::PlanApprovalSwitchBuild
                | InlineListSelection::PlanApprovalSwitchAuto
                | InlineListSelection::ToolApprovalEnable => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitMiMoAuthMethod => match choice {
                InlineListSelection::ConfigAction(action) => {
                    if let Some(method_str) = action.strip_prefix("mimo-auth:") {
                        let auth_method = match method_str {
                            "token-plan" => MiMoAuthMethod::TokenPlan,
                            "pay-as-you-go" => MiMoAuthMethod::PayAsYouGo,
                            _ => {
                                renderer.line(MessageStyle::Error, "Unknown MiMo auth method selection.")?;
                                return Ok(ModelPickerProgress::InProgress);
                            }
                        };
                        self.selected_mimo_auth = Some(auth_method);
                        if let Some(ref mut selection) = self.selection {
                            selection.mimo_auth_method = Some(auth_method);
                            selection.env_key = auth_method.env_key().to_string();
                        }
                        self.finish_after_mimo_auth_method(renderer)
                    } else {
                        renderer.line(MessageStyle::Error, "Choose an auth method for MiMo or press Esc to cancel.")?;
                        Ok(ModelPickerProgress::InProgress)
                    }
                }
                InlineListSelection::Model(_)
                | InlineListSelection::DynamicModel(_)
                | InlineListSelection::CustomProvider(_)
                | InlineListSelection::RefreshDynamicModels
                | InlineListSelection::Reasoning(_)
                | InlineListSelection::DisableReasoning
                | InlineListSelection::OpenAIServiceTier(_)
                | InlineListSelection::CustomModel
                | InlineListSelection::Theme(_)
                | InlineListSelection::Session(_)
                | InlineListSelection::SessionForkMode { .. }
                | InlineListSelection::SlashCommand(_)
                | InlineListSelection::ToolApproval(_)
                | InlineListSelection::ToolApprovalDenyOnce
                | InlineListSelection::ToolApprovalSession
                | InlineListSelection::ToolApprovalPermanent
                | InlineListSelection::ToolApprovalEnable
                | InlineListSelection::FileConflictReload
                | InlineListSelection::FileConflictViewDiff
                | InlineListSelection::FileConflictAbort
                | InlineListSelection::SessionLimitIncrease(_)
                | InlineListSelection::RewindCheckpoint(_)
                | InlineListSelection::RewindAction(_)
                | InlineListSelection::AskUserChoice { .. }
                | InlineListSelection::RequestUserInputAnswer { .. }
                | InlineListSelection::PlanApprovalExecute
                | InlineListSelection::PlanApprovalEditPlan
                | InlineListSelection::PlanApprovalAutoAccept
                | InlineListSelection::PlanApprovalSwitchBuild
                | InlineListSelection::PlanApprovalSwitchAuto => {
                    renderer.line(MessageStyle::Error, "Choose an auth method for MiMo or press Esc to cancel.")?;
                    Ok(ModelPickerProgress::InProgress)
                }
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
                InlineListSelection::ConfigAction(_)
                | InlineListSelection::SlashCommand(_)
                | InlineListSelection::Session(_)
                | InlineListSelection::SessionForkMode { .. }
                | InlineListSelection::FileConflictReload
                | InlineListSelection::FileConflictViewDiff
                | InlineListSelection::FileConflictAbort
                | InlineListSelection::SessionLimitIncrease(_)
                | InlineListSelection::RewindCheckpoint(_)
                | InlineListSelection::RewindAction(_)
                | InlineListSelection::AskUserChoice { .. }
                | InlineListSelection::RequestUserInputAnswer { .. }
                | InlineListSelection::PlanApprovalExecute
                | InlineListSelection::PlanApprovalEditPlan
                | InlineListSelection::PlanApprovalAutoAccept
                | InlineListSelection::PlanApprovalSwitchBuild
                | InlineListSelection::PlanApprovalSwitchAuto
                | InlineListSelection::ToolApprovalEnable => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitApiKey => {
                renderer.line(MessageStyle::Info, "Enter the API key in the input field or type 'skip'.")?;
                Ok(ModelPickerProgress::InProgress)
            }
        }
    }

    fn handle_model_selection(&mut self, renderer: &mut AnsiRenderer, input: &str) -> Result<ModelPickerProgress> {
        let selection = match parse_model_selection(&self.options, input, self.vt_cfg.as_ref()) {
            Ok(detail) => detail,
            Err(err) => {
                renderer.line(MessageStyle::Error, &err.to_string())?;
                renderer.line(MessageStyle::Info, "Try again with '<provider> <model-id>'.")?;
                return Ok(ModelPickerProgress::InProgress);
            }
        };

        self.process_model_selection(renderer, selection)
    }

    fn preferred_model_selection(&self) -> Option<InlineListSelection> {
        let provider_key = self.current_provider.trim();
        let model_key = self.current_model.trim();
        if provider_key.is_empty() || model_key.is_empty() {
            return None;
        }

        if let Ok(provider) = Provider::from_str(provider_key) {
            if let Some(index) = find_option_index(provider, model_key, &self.options) {
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
            if detail.provider_key.eq_ignore_ascii_case(provider_key) && detail.model_id.eq_ignore_ascii_case(model_key)
            {
                return Some(InlineListSelection::CustomProvider(entry_index));
            }
        }

        None
    }
}

pub(crate) use self::subagent::{SubagentModelSelection, pick_subagent_model};

#[cfg(test)]
mod tests;
