use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;
use std::str::FromStr;

use vtcode_config::{OpenAIServiceTier, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListSelection, from_tui_reasoning};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::ui::interactive_list::SelectionInterrupted;

use dynamic_models::DynamicModelRegistry;
use interaction::{
    ModelSelectionListOutcome, select_model_with_ratatui_list, select_reasoning_with_ratatui,
    select_service_tier_with_ratatui,
};
use options::{MODEL_OPTIONS, ModelOption};
use rendering::{
    CLOSE_THEME_MESSAGE, prompt_api_key_plain, prompt_custom_model_entry, prompt_reasoning_plain,
    prompt_service_tier_plain, render_reasoning_inline, render_service_tier_inline,
    render_step_one_inline, render_step_one_plain, show_secure_api_modal,
};
use selection::{
    ExistingKey, ReasoningChoice, SelectionDetail, ServiceTierChoice, is_cancel_command,
    parse_model_selection, selection_from_option,
};

mod config_persistence;
mod dynamic_models;
mod interaction;
mod options;
mod rendering;
mod selection;
mod state_machine;

pub use selection::ModelSelectionResult;
pub(super) use vtcode_config::read_workspace_env_value as read_workspace_env;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum PickerStep {
    AwaitModel,
    AwaitReasoning,
    AwaitServiceTier,
    AwaitApiKey,
}

pub enum ModelPickerProgress {
    InProgress,
    NeedsRefresh,
    Completed(ModelSelectionResult),
    Cancelled,
}

pub struct ModelPickerState {
    options: &'static [ModelOption],
    step: PickerStep,
    inline_enabled: bool,
    current_reasoning: ReasoningEffortLevel,
    current_service_tier: Option<OpenAIServiceTier>,
    current_provider: String,
    current_model: String,
    selection: Option<SelectionDetail>,
    selected_reasoning: Option<ReasoningEffortLevel>,
    selected_service_tier: Option<bool>,
    pending_api_key: Option<String>,
    workspace: Option<PathBuf>,
    dynamic_models: DynamicModelRegistry,
    plain_mode_active: bool,
}

pub enum ModelPickerStart {
    Completed {
        state: ModelPickerState,
        selection: ModelSelectionResult,
    },
    InProgress(ModelPickerState),
}

impl ModelPickerState {
    #[allow(clippy::new_ret_no_self)]
    pub async fn new(
        renderer: &mut AnsiRenderer,
        current_reasoning: ReasoningEffortLevel,
        current_service_tier: Option<OpenAIServiceTier>,
        workspace: Option<PathBuf>,
        current_provider: String,
        current_model: String,
    ) -> Result<ModelPickerStart> {
        let options = MODEL_OPTIONS.as_slice();
        let inline_enabled = renderer.supports_inline_ui();
        let dynamic_models = DynamicModelRegistry::load(options, workspace.as_deref()).await;

        let mut state = Self {
            options,
            step: PickerStep::AwaitModel,
            inline_enabled,
            current_reasoning,
            current_service_tier,
            current_provider,
            current_model,
            selection: None,
            selected_reasoning: None,
            selected_service_tier: None,
            pending_api_key: None,
            workspace,
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
            )?;
        }

        if !inline_enabled {
            loop {
                match select_model_with_ratatui_list(
                    options,
                    current_reasoning,
                    &state.dynamic_models,
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
                        render_step_one_plain(renderer, options, &state.dynamic_models)?;
                        prompt_custom_model_entry(renderer)?;
                        break;
                    }
                    Ok(ModelSelectionListOutcome::Cancelled) => {
                        state.plain_mode_active = true;
                        render_step_one_plain(renderer, options, &state.dynamic_models)?;
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
                        render_step_one_plain(renderer, options, &state.dynamic_models)?;
                        prompt_custom_model_entry(renderer)?;
                        break;
                    }
                }
            }
        }

        Ok(ModelPickerStart::InProgress(state))
    }

    pub async fn refresh_dynamic_models(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        self.dynamic_models =
            DynamicModelRegistry::load(self.options, self.workspace.as_deref()).await;
        self.selection = None;
        self.selected_reasoning = None;
        self.selected_service_tier = None;
        self.pending_api_key = None;
        self.step = PickerStep::AwaitModel;
        if self.inline_enabled {
            renderer.line(MessageStyle::Info, "Refreshing local model inventory...")?;
            render_step_one_inline(
                renderer,
                self.options,
                self.current_reasoning,
                &self.dynamic_models,
                self.preferred_model_selection(),
                &self.current_provider,
                &self.current_model,
            )?;
        } else if self.plain_mode_active {
            renderer.line(MessageStyle::Info, "Refreshing local model inventory...")?;
            render_step_one_plain(renderer, self.options, &self.dynamic_models)?;
            if matches!(self.step, PickerStep::AwaitModel) {
                prompt_custom_model_entry(renderer)?;
            }
        }
        Ok(())
    }

    pub fn handle_input(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
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
            PickerStep::AwaitApiKey => self.handle_api_key(renderer, trimmed),
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
                InlineListSelection::OpenAIServiceTier(priority) => {
                    self.apply_service_tier_choice(renderer, priority)
                }
                InlineListSelection::CustomModel
                | InlineListSelection::Model(_)
                | InlineListSelection::RefreshDynamicModels
                | InlineListSelection::DynamicModel(_)
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
        let selection = match parse_model_selection(self.options, input) {
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

        if let Some((index, _)) = self.options.iter().enumerate().find(|(_, option)| {
            option.provider.to_string() == provider_key && option.id.eq_ignore_ascii_case(model_key)
        }) {
            return Some(InlineListSelection::Model(index));
        }

        let Ok(provider) = Provider::from_str(provider_key.as_str()) else {
            return None;
        };
        for entry_index in self.dynamic_models.indexes_for(provider) {
            if let Some(detail) = self.dynamic_models.detail(entry_index)
                && detail.model_id.eq_ignore_ascii_case(model_key)
            {
                return Some(InlineListSelection::DynamicModel(entry_index));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests;
