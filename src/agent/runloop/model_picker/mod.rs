use anyhow::{Context, Result, anyhow};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::agent::runloop::tui_compat::from_tui_reasoning;
use vtcode::interactive_list::SelectionInterrupted;
use vtcode_config::auth::CustomApiKeyStorage;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::InlineListSelection;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::update_model_preference;

use dynamic_models::DynamicModelRegistry;
use interaction::{
    ModelSelectionListOutcome, select_model_with_ratatui_list, select_reasoning_with_ratatui,
};
use options::{MODEL_OPTIONS, ModelOption};
use rendering::{
    CLOSE_THEME_MESSAGE, prompt_api_key_plain, prompt_custom_model_entry, prompt_reasoning_plain,
    render_reasoning_inline, render_step_one_inline, render_step_one_plain, show_secure_api_modal,
};
use selection::{
    ExistingKey, ReasoningChoice, SelectionDetail, is_cancel_command, parse_model_selection,
    selection_from_option,
};

mod dynamic_models;
mod interaction;
mod options;
mod rendering;
mod selection;
mod state_machine;

pub use selection::ModelSelectionResult;

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
enum PickerStep {
    AwaitModel,
    AwaitReasoning,
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
    selection: Option<SelectionDetail>,
    selected_reasoning: Option<ReasoningEffortLevel>,
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
        workspace: Option<PathBuf>,
    ) -> Result<ModelPickerStart> {
        let options = MODEL_OPTIONS.as_slice();
        let inline_enabled = renderer.supports_inline_ui();
        let dynamic_models = DynamicModelRegistry::load(options, workspace.as_deref()).await;

        let mut state = Self {
            options,
            step: PickerStep::AwaitModel,
            inline_enabled,
            current_reasoning,
            selection: None,
            selected_reasoning: None,
            pending_api_key: None,
            workspace,
            dynamic_models,
            plain_mode_active: false,
        };

        if inline_enabled {
            render_step_one_inline(renderer, options, current_reasoning, &state.dynamic_models)?;
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
        self.pending_api_key = None;
        self.step = PickerStep::AwaitModel;
        if self.inline_enabled {
            renderer.line(MessageStyle::Info, "Refreshing local model inventory...")?;
            render_step_one_inline(
                renderer,
                self.options,
                self.current_reasoning,
                &self.dynamic_models,
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
            renderer.line(MessageStyle::Info, "Model picker cancelled.")?;
            return Ok(ModelPickerProgress::Cancelled);
        }

        if matches!(self.step, PickerStep::AwaitModel) && trimmed.eq_ignore_ascii_case("refresh") {
            return Ok(ModelPickerProgress::NeedsRefresh);
        }

        match self.step {
            PickerStep::AwaitModel => self.handle_model_selection(renderer, trimmed),
            PickerStep::AwaitReasoning => self.handle_reasoning(renderer, trimmed),
            PickerStep::AwaitApiKey => self.handle_api_key(renderer, trimmed),
        }
    }

    pub async fn persist_selection(
        &self,
        workspace: &std::path::Path,
        selection: &ModelSelectionResult,
    ) -> Result<VTCodeConfig> {
        let mut manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
            format!(
                "Failed to load vtcode configuration for workspace {}",
                workspace.display()
            )
        })?;
        let mut config = manager.config().clone();
        config.agent.provider = selection.provider.clone();

        if selection.provider_enum == Some(Provider::Ollama) {
            let is_cloud_model =
                selection.model.contains(":cloud") || selection.model.contains("-cloud");
            if is_cloud_model {
                config.agent.api_key_env = selection.env_key.clone();
                if let Some(ref api_key) = selection.api_key {
                    // Store API key in secure storage (keyring)
                    let storage_mode = config.agent.credential_storage_mode;
                    let key_storage = CustomApiKeyStorage::new(&selection.provider);
                    if let Err(e) = key_storage.store(api_key, storage_mode) {
                        tracing::warn!(
                            "Failed to store API key for provider '{}' securely: {}",
                            selection.provider,
                            e
                        );
                    }
                    // Track provider (key not serialized, just for UI/migration)
                    config
                        .agent
                        .custom_api_keys
                        .insert(selection.provider.clone(), String::new());
                } else {
                    config.agent.custom_api_keys.remove(&selection.provider);
                    // Clear any previously stored key
                    let storage_mode = config.agent.credential_storage_mode;
                    let key_storage = CustomApiKeyStorage::new(&selection.provider);
                    let _ = key_storage.clear(storage_mode);
                }
            } else {
                config.agent.api_key_env = String::new();
                config.agent.custom_api_keys.remove(&selection.provider);
                let storage_mode = config.agent.credential_storage_mode;
                let key_storage = CustomApiKeyStorage::new(&selection.provider);
                let _ = key_storage.clear(storage_mode);
            }
        } else {
            config.agent.api_key_env = selection.env_key.clone();
            if let Some(ref api_key) = selection.api_key {
                // Store API key in secure storage (keyring)
                let storage_mode = config.agent.credential_storage_mode;
                let key_storage = CustomApiKeyStorage::new(&selection.provider);
                if let Err(e) = key_storage.store(api_key, storage_mode) {
                    tracing::warn!(
                        "Failed to store API key for provider '{}' securely: {}",
                        selection.provider,
                        e
                    );
                }
                // Track provider (key not serialized, just for UI/migration)
                config
                    .agent
                    .custom_api_keys
                    .insert(selection.provider.clone(), String::new());
            } else {
                config.agent.custom_api_keys.remove(&selection.provider);
                // Clear any previously stored key
                let storage_mode = config.agent.credential_storage_mode;
                let key_storage = CustomApiKeyStorage::new(&selection.provider);
                let _ = key_storage.clear(storage_mode);
            }
        }

        config.agent.default_model = selection.model.clone();
        config.agent.reasoning_effort = selection.reasoning;

        manager.save_config(&config)?;
        update_model_preference(&selection.provider, &selection.model)
            .await
            .ok();
        Ok(config)
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
                    renderer.close_modal();
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
                InlineListSelection::Theme(_) => {
                    renderer.line(MessageStyle::Error, CLOSE_THEME_MESSAGE)?;
                    Ok(ModelPickerProgress::InProgress)
                }
                _ => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitReasoning => match choice {
                InlineListSelection::Reasoning(level) => {
                    renderer.close_modal();
                    self.apply_reasoning_choice(renderer, from_tui_reasoning(level))
                }
                InlineListSelection::DisableReasoning => {
                    renderer.close_modal();
                    self.apply_reasoning_off_choice(renderer)
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
}

fn read_workspace_env(workspace: &Path, env_key: &str) -> Result<Option<String>> {
    let env_path = workspace.join(".env");
    let iter = match dotenvy::from_path_iter(&env_path) {
        Ok(iter) => iter,
        Err(dotenvy::Error::Io(err)) if err.kind() == ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(err) => {
            return Err(anyhow!(err).context(format!("Failed to read {}", env_path.display())));
        }
    };

    for item in iter {
        let (key, value) = item
            .map_err(|err: dotenvy::Error| anyhow!(err))
            .with_context(|| format!("Failed to parse {}", env_path.display()))?;
        if key == env_key {
            if value.trim().is_empty() {
                return Ok(None);
            }
            return Ok(Some(value));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests;
