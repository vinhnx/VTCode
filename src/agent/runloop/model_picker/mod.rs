use anyhow::{Context, Result, anyhow};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use vtcode::interactive_list::SelectionInterrupted;
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

pub use selection::ModelSelectionResult;

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
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
                    config
                        .agent
                        .custom_api_keys
                        .insert(selection.provider.clone(), api_key.clone());
                } else {
                    config.agent.custom_api_keys.remove(&selection.provider);
                }
            } else {
                config.agent.api_key_env = String::new();
                config.agent.custom_api_keys.remove(&selection.provider);
            }
        } else {
            config.agent.api_key_env = selection.env_key.clone();
            if let Some(ref api_key) = selection.api_key {
                config
                    .agent
                    .custom_api_keys
                    .insert(selection.provider.clone(), api_key.clone());
            } else {
                config.agent.custom_api_keys.remove(&selection.provider);
            }
        }

        config.agent.default_model = selection.model.clone();
        config.agent.reasoning_effort = selection.reasoning;

        config.router.models.simple = selection.model.clone();
        config.router.models.standard = selection.model.clone();
        config.router.models.complex = selection.model.clone();
        config.router.models.codegen_heavy = selection.model.clone();
        config.router.models.retrieval_heavy = selection.model.clone();
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
                InlineListSelection::Session(_)
                | InlineListSelection::SlashCommand(_)
                | InlineListSelection::ToolApproval(_)
                | InlineListSelection::ToolApprovalSession
                | InlineListSelection::ToolApprovalPermanent => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitReasoning => match choice {
                InlineListSelection::Reasoning(level) => {
                    renderer.close_modal();
                    self.apply_reasoning_choice(renderer, level)
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
                InlineListSelection::Session(_) | InlineListSelection::SlashCommand(_) => {
                    Ok(ModelPickerProgress::InProgress)
                }
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

    fn handle_reasoning(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        if self.selection.is_none() {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        }

        let normalized = input.to_ascii_lowercase();
        if matches!(normalized.as_str(), "off" | "disable" | "none") {
            return self.apply_reasoning_off_choice(renderer);
        }

        let level = match normalized.as_str() {
            "easy" | "low" => Some(ReasoningEffortLevel::Low),
            "medium" => Some(ReasoningEffortLevel::Medium),
            "hard" | "high" => Some(ReasoningEffortLevel::High),
            "skip" => Some(self.current_reasoning),
            _ => None,
        };

        let Some(selected) = level else {
            renderer.line(
                MessageStyle::Error,
                "Unknown reasoning option. Use easy, medium, hard, skip, or off.",
            )?;
            if let Some(progress) = self.prompt_reasoning_step(renderer)? {
                return Ok(progress);
            }
            return Ok(ModelPickerProgress::InProgress);
        };

        self.apply_reasoning_choice(renderer, selected)
    }

    fn prompt_reasoning_step(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<Option<ModelPickerProgress>> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };
        if self.inline_enabled {
            render_reasoning_inline(renderer, selection, self.current_reasoning)?;
            return Ok(None);
        }

        match select_reasoning_with_ratatui(selection, self.current_reasoning) {
            Ok(Some(ReasoningChoice::Level(level))) => {
                self.apply_reasoning_choice(renderer, level).map(Some)
            }
            Ok(Some(ReasoningChoice::Disable)) => {
                self.apply_reasoning_off_choice(renderer).map(Some)
            }
            Ok(None) => {
                prompt_reasoning_plain(renderer, selection, self.current_reasoning)?;
                Ok(None)
            }
            Err(err) => {
                if err.is::<SelectionInterrupted>() {
                    return Err(err);
                }
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Interactive reasoning selector unavailable ({}). Falling back to manual input.",
                        err
                    ),
                )?;
                prompt_reasoning_plain(renderer, selection, self.current_reasoning)?;
                Ok(None)
            }
        }
    }

    fn prompt_api_key_step(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("API key requested before selecting a model"));
        };
        if self.inline_enabled {
            renderer.close_modal();
            show_secure_api_modal(renderer, selection, self.workspace.as_deref());
        }
        prompt_api_key_plain(renderer, selection, self.workspace.as_deref())
    }

    fn apply_reasoning_choice(
        &mut self,
        renderer: &mut AnsiRenderer,
        level: ReasoningEffortLevel,
    ) -> Result<ModelPickerProgress> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };
        self.selected_reasoning = Some(level);
        if selection.requires_api_key {
            self.step = PickerStep::AwaitApiKey;
            self.prompt_api_key_step(renderer)?;
            return Ok(ModelPickerProgress::InProgress);
        }
        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
    }

    fn apply_reasoning_off_choice(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<ModelPickerProgress> {
        let Some(current_selection) = self.selection.clone() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };

        let Some(target_model) = current_selection.reasoning_off_model else {
            renderer.line(
                MessageStyle::Error,
                "This model does not have a non-reasoning variant.",
            )?;
            if self.inline_enabled {
                render_reasoning_inline(renderer, &current_selection, self.current_reasoning)?;
            } else {
                prompt_reasoning_plain(renderer, &current_selection, self.current_reasoning)?;
            }
            return Ok(ModelPickerProgress::InProgress);
        };

        let Some(option) = self
            .options
            .iter()
            .find(|candidate| candidate.id.eq_ignore_ascii_case(target_model.as_str()))
        else {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Unable to locate the non-reasoning variant {}.",
                    target_model.as_str()
                ),
            )?;
            if self.inline_enabled {
                render_reasoning_inline(renderer, &current_selection, self.current_reasoning)?;
            } else {
                prompt_reasoning_plain(renderer, &current_selection, self.current_reasoning)?;
            }
            return Ok(ModelPickerProgress::InProgress);
        };

        self.selected_reasoning = None;
        let mut new_selection = selection_from_option(option);
        if new_selection.provider_label != current_selection.provider_label {
            new_selection.provider_label = current_selection.provider_label.clone();
        }
        let alt_display = new_selection.model_display.clone();
        let alt_id = new_selection.model_id.clone();

        let progress = self.process_model_selection(renderer, new_selection)?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Reasoning disabled by switching to {} ({}).",
                alt_display, alt_id
            ),
        )?;
        Ok(progress)
    }

    fn build_result(&self) -> Result<ModelSelectionResult> {
        let selection = self
            .selection
            .as_ref()
            .ok_or_else(|| anyhow!("Model selection missing"))?;
        let chosen_reasoning = self.selected_reasoning.unwrap_or(self.current_reasoning);
        let reasoning_changed = chosen_reasoning != self.current_reasoning;

        Ok(ModelSelectionResult {
            provider: selection.provider_key.clone(),
            provider_label: selection.provider_label.clone(),
            provider_enum: selection.provider_enum,
            model: selection.model_id.clone(),
            model_display: selection.model_display.clone(),
            known_model: selection.known_model,
            reasoning_supported: selection.reasoning_supported,
            reasoning: chosen_reasoning,
            reasoning_changed,
            api_key: self.pending_api_key.clone(),
            env_key: selection.env_key.clone(),
            requires_api_key: selection.requires_api_key,
        })
    }

    fn process_model_selection(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: SelectionDetail,
    ) -> Result<ModelPickerProgress> {
        let message = format!(
            "Selected {} ({}) from {}.",
            selection.model_display, selection.model_id, selection.provider_label
        );
        renderer.line(MessageStyle::Info, &message)?;

        self.pending_api_key = None;
        let mut selection = selection;
        if selection.requires_api_key {
            match self.find_existing_api_key(&selection.env_key) {
                Ok(Some(ExistingKey::Environment)) => {
                    selection.requires_api_key = false;
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Using existing environment variable {} for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                }
                Ok(Some(ExistingKey::WorkspaceDotenv(value))) => {
                    selection.requires_api_key = false;
                    unsafe {
                        std::env::set_var(&selection.env_key, &value);
                    }
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Loaded {} from workspace .env for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                }
                Ok(None) => {}
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Failed to inspect stored credentials for {}: {}",
                            selection.provider_label, err
                        ),
                    )?;
                }
            }
        }

        self.selection = Some(selection);
        if self
            .selection
            .as_ref()
            .map(|detail| detail.reasoning_supported)
            .unwrap_or(false)
        {
            self.step = PickerStep::AwaitReasoning;
            if let Some(progress) = self.prompt_reasoning_step(renderer)? {
                return Ok(progress);
            }
            return Ok(ModelPickerProgress::InProgress);
        }

        if self
            .selection
            .as_ref()
            .map(|detail| detail.requires_api_key)
            .unwrap_or(false)
        {
            self.step = PickerStep::AwaitApiKey;
            self.prompt_api_key_step(renderer)?;
            return Ok(ModelPickerProgress::InProgress);
        }

        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
    }

    fn handle_api_key(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("API key requested before selecting a model"));
        };
        if input.eq_ignore_ascii_case("skip") {
            match self.find_existing_api_key(&selection.env_key) {
                Ok(Some(ExistingKey::Environment)) => {
                    renderer.close_modal();
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Using existing environment variable {} for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                    self.pending_api_key = None;
                    if let Some(current) = self.selection.as_mut() {
                        current.requires_api_key = false;
                    }
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                Ok(Some(ExistingKey::WorkspaceDotenv(value))) => {
                    renderer.close_modal();
                    unsafe {
                        std::env::set_var(&selection.env_key, &value);
                    }
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Loaded {} from workspace .env for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                    self.pending_api_key = None;
                    if let Some(current) = self.selection.as_mut() {
                        current.requires_api_key = false;
                    }
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                Ok(None) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "No stored API key found under {}. Provide a key or update your workspace .env.",
                            selection.env_key
                        ),
                    )?;
                    prompt_api_key_plain(renderer, selection, self.workspace.as_deref())?;
                    return Ok(ModelPickerProgress::InProgress);
                }
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Failed to inspect stored credentials for {}: {}",
                            selection.provider_label, err
                        ),
                    )?;
                    prompt_api_key_plain(renderer, selection, self.workspace.as_deref())?;
                    return Ok(ModelPickerProgress::InProgress);
                }
            }
        }

        self.pending_api_key = Some(input.to_string());
        renderer.close_modal();
        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
    }

    fn find_existing_api_key(&self, env_key: &str) -> Result<Option<ExistingKey>> {
        if let Ok(value) = std::env::var(env_key)
            && !value.trim().is_empty()
        {
            return Ok(Some(ExistingKey::Environment));
        }

        if let Some(workspace) = self.workspace.as_deref()
            && let Some(value) = read_workspace_env(workspace, env_key)?
            && !value.trim().is_empty()
        {
            return Ok(Some(ExistingKey::WorkspaceDotenv(value)));
        }

        Ok(None)
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
mod tests {
    use super::*;
    use anyhow::Result;
    use std::fs;
    use tempfile::tempdir;
    use vtcode_core::config::models::ModelId;

    fn has_model(options: &[ModelOption], model: ModelId) -> bool {
        let id = model.as_str();
        let provider = model.provider();
        options
            .iter()
            .any(|option| option.id == id && option.provider == provider)
    }

    #[test]
    fn model_picker_lists_new_moonshot_models() {
        let options = MODEL_OPTIONS.as_slice();
        assert!(has_model(options, ModelId::MoonshotKimiK2TurboPreview));
        assert!(has_model(options, ModelId::MoonshotKimiK20905Preview));
        assert!(has_model(options, ModelId::MoonshotKimiK20711Preview));
        assert!(has_model(options, ModelId::MoonshotKimiLatest));
        assert!(has_model(options, ModelId::MoonshotKimiLatest8k));
        assert!(has_model(options, ModelId::MoonshotKimiLatest32k));
        assert!(has_model(options, ModelId::MoonshotKimiLatest128k));
        assert!(has_model(options, ModelId::OpenRouterMoonshotaiKimiK20905));
    }

    #[test]
    fn model_picker_lists_new_anthropic_models() {
        let options = MODEL_OPTIONS.as_slice();
        assert!(has_model(options, ModelId::ClaudeOpus41));
        assert!(has_model(options, ModelId::ClaudeSonnet45));
        assert!(has_model(options, ModelId::ClaudeHaiku45));
        assert!(has_model(options, ModelId::ClaudeSonnet4));
    }

    #[test]
    fn model_picker_lists_new_xai_models() {
        let options = MODEL_OPTIONS.as_slice();
        assert!(has_model(options, ModelId::XaiGrok4));
        assert!(has_model(options, ModelId::XaiGrok4Mini));
        assert!(has_model(options, ModelId::XaiGrok4Code));
        assert!(has_model(options, ModelId::XaiGrok4CodeLatest));
        assert!(has_model(options, ModelId::XaiGrok4Vision));
    }

    #[test]
    fn model_picker_lists_new_zai_models() {
        let options = MODEL_OPTIONS.as_slice();
        assert!(has_model(options, ModelId::ZaiGlm46));
        assert!(has_model(options, ModelId::ZaiGlm45));
        assert!(has_model(options, ModelId::ZaiGlm45Air));
        assert!(has_model(options, ModelId::ZaiGlm45X));
        assert!(has_model(options, ModelId::ZaiGlm45Airx));
        assert!(has_model(options, ModelId::ZaiGlm45Flash));
        assert!(has_model(options, ModelId::ZaiGlm432b0414128k));
    }

    #[test]
    fn model_picker_lists_new_ollama_cloud_models() {
        let options = MODEL_OPTIONS.as_slice();
        assert!(has_model(options, ModelId::OllamaGptOss20b));
        assert!(has_model(options, ModelId::OllamaGptOss120bCloud));
        assert!(has_model(options, ModelId::OllamaDeepseekV31_671bCloud));
        assert!(has_model(options, ModelId::OllamaKimiK21tCloud));
        assert!(has_model(options, ModelId::OllamaQwen3Coder480bCloud));
        assert!(has_model(options, ModelId::OllamaGlm46Cloud));
        assert!(has_model(options, ModelId::MinimaxM2));
    }

    #[test]
    fn read_workspace_env_returns_value_when_present() -> Result<()> {
        let dir = tempdir()?;
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "OPENAI_API_KEY=sk-test\n")?;
        let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
        assert_eq!(value, Some("sk-test".to_string()));
        Ok(())
    }

    #[test]
    fn read_workspace_env_returns_none_when_missing_file() -> Result<()> {
        let dir = tempdir()?;
        let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
        assert_eq!(value, None);
        Ok(())
    }

    #[test]
    fn read_workspace_env_returns_none_when_key_absent() -> Result<()> {
        let dir = tempdir()?;
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "OTHER_KEY=value\n")?;
        let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
        assert_eq!(value, None);
        Ok(())
    }
}
