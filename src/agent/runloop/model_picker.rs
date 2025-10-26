use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json;
use vtcode_core::config::constants::{env_vars, reasoning, ui, urls};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListItem, InlineListSearchConfig, InlineListSelection};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::{
    DotConfig, get_dot_manager, load_user_config, update_model_preference,
};

use vtcode::interactive_list::{SelectionEntry, SelectionInterrupted, run_interactive_selection};
use vtcode_core::llm::providers::lmstudio::fetch_lmstudio_models;
use vtcode_core::llm::providers::ollama::fetch_ollama_models;

use tokio::fs;

#[derive(Clone, Copy)]
struct ModelOption {
    provider: Provider,
    id: &'static str,
    display: &'static str,
    description: &'static str,
    supports_reasoning: bool,
    reasoning_alternative: Option<ModelId>,
}

static MODEL_OPTIONS: Lazy<Vec<ModelOption>> = Lazy::new(|| {
    let mut options = Vec::new();
    for provider in Provider::all_providers() {
        for model in ModelId::models_for_provider(provider) {
            options.push(ModelOption {
                provider,
                id: model.as_str(),
                display: model.display_name(),
                description: model.description(),
                supports_reasoning: model.supports_reasoning_effort(),
                reasoning_alternative: model.non_reasoning_variant(),
            });
        }
    }
    options
});

fn picker_provider_order() -> Vec<Provider> {
    let mut providers: Vec<Provider> = Provider::all_providers()
        .into_iter()
        .filter(|provider| !matches!(provider, Provider::LmStudio | Provider::Ollama))
        .collect();
    providers.push(Provider::LmStudio);
    providers.push(Provider::Ollama);
    providers
}

const DYNAMIC_MODEL_CACHE_FILENAME: &str = "dynamic_local_models.json";
const DYNAMIC_MODEL_CACHE_TTL_SECS: u64 = 300;

const STEP_ONE_TITLE: &str = "Model picker – Step 1";
const STEP_TWO_TITLE: &str = "Model picker – Step 2";
const STEP_ONE_NAVIGATION_HINT: &str = "Use ↑/↓ to navigate, Enter to select, or Esc to cancel.";
const STEP_TWO_NAVIGATION_HINT: &str = "Use ↑/↓ to navigate, Enter to choose, or Esc to cancel.";
const CUSTOM_PROVIDER_TITLE: &str = "Custom provider + model";
const CUSTOM_PROVIDER_SUBTITLE: &str = "Provide the provider name and model identifier manually.";
const CUSTOM_PROVIDER_BADGE: &str = "Manual";
const REASONING_BADGE: &str = "Reasoning";
const REASONING_OFF_BADGE: &str = "No reasoning";
const CURRENT_BADGE: &str = "Current";
const CURRENT_REASONING_PREFIX: &str = "Current reasoning effort: ";
const KEEP_CURRENT_DESCRIPTION: &str = "Retain the existing reasoning configuration.";
const CLOSE_THEME_MESSAGE: &str = "Close the active model picker before selecting a theme.";

#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerStep {
    AwaitModel,
    AwaitReasoning,
    AwaitApiKey,
}

#[derive(Clone, Copy)]
enum ReasoningChoice {
    Level(ReasoningEffortLevel),
    Disable,
}

#[derive(Clone)]
struct SelectionDetail {
    provider_key: String,
    provider_label: String,
    provider_enum: Option<Provider>,
    model_id: String,
    model_display: String,
    known_model: bool,
    reasoning_supported: bool,
    reasoning_optional: bool,
    reasoning_off_model: Option<ModelId>,
    requires_api_key: bool,
    env_key: String,
}

enum ExistingKey {
    Environment,
    WorkspaceDotenv(String),
}

pub struct ModelSelectionResult {
    pub provider: String,
    pub provider_label: String,
    pub provider_enum: Option<Provider>,
    pub model: String,
    pub model_display: String,
    pub known_model: bool,
    pub reasoning_supported: bool,
    pub reasoning: ReasoningEffortLevel,
    pub reasoning_changed: bool,
    pub api_key: Option<String>,
    pub env_key: String,
    pub requires_api_key: bool,
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

#[derive(Clone, Default)]
struct DynamicModelRegistry {
    entries: Vec<SelectionDetail>,
    provider_models: HashMap<Provider, Vec<usize>>,
    provider_errors: HashMap<Provider, String>,
    provider_warnings: HashMap<Provider, String>,
}

struct ProviderEndpointConfig {
    lmstudio: Option<String>,
    ollama: Option<String>,
}

type StaticModelIndex = HashMap<Provider, HashSet<String>>;
type CacheEntries = HashMap<String, CachedDynamicModelEntry>;

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

        // For local Ollama models, do not store API key environment variable since they don't require them
        // For cloud Ollama models, store the environment variable reference
        if selection.provider_enum == Some(Provider::Ollama) {
            let is_cloud_model =
                selection.model.contains(":cloud") || selection.model.contains("-cloud");
            if is_cloud_model {
                // Cloud Ollama models should keep the API key environment variable
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
                // Local Ollama models don't need API key environment variables
                config.agent.api_key_env = String::new(); // Clear the API key environment variable
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
                        // SAFETY: Keys are derived from known providers or sanitized user input.
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
                        // SAFETY: Keys are derived from known providers or sanitized user input.
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
        // Preserve manual selection label when switching providers with identical display names.
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
}

// Helper function that returns LMStudio setup instructions
fn get_lmstudio_setup_instructions() -> String {
    "LM Studio server is not running. To start:\n  1. Download and install LM Studio from https://lmstudio.ai\n  2. Launch LM Studio\n  3. Click the 'Local Server' toggle to start the server\n  4. Select and load a model in the 'Local Server' tab\n  5. Make sure the server runs on port 1234 (default)"
        .to_string()
}

// Helper function that returns Ollama setup instructions
fn get_ollama_setup_instructions() -> String {
    "Ollama server is not running. To start:\n  1. Install Ollama from https://ollama.com\n  2. Run 'ollama serve' in a terminal\n  3. Pull models using 'ollama pull <model-name>' (e.g., 'ollama pull llama3:8b')"
        .to_string()
}

fn render_step_one_inline(
    renderer: &mut AnsiRenderer,
    options: &[ModelOption],
    current_reasoning: ReasoningEffortLevel,
    dynamic_models: &DynamicModelRegistry,
) -> Result<()> {
    let mut items = Vec::new();
    let mut first_section = true;
    for provider in picker_provider_order() {
        let provider_models: Vec<(usize, &ModelOption)> = options
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.provider == provider)
            .collect();
        let dynamic_indexes = dynamic_models.indexes_for(provider);
        let has_error = dynamic_models.error_for(provider).is_some();
        let has_warning = dynamic_models.warning_for(provider).is_some();

        if provider_models.is_empty() && dynamic_indexes.is_empty() && !has_error && !has_warning {
            continue;
        }

        if !first_section {
            items.push(provider_group_divider_item());
        }
        first_section = false;
        items.push(InlineListItem {
            title: provider.label().to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: None,
            search_value: Some(provider.label().to_string()),
        });

        for (idx, option) in &provider_models {
            let badge = option
                .supports_reasoning
                .then(|| REASONING_BADGE.to_string());
            items.push(InlineListItem {
                title: option.display.to_string(),
                subtitle: Some(option.description.to_string()),
                badge,
                indent: 2,
                selection: Some(InlineListSelection::Model(*idx)),
                search_value: Some(format!("{} {}", provider.label(), option.display)),
            });
        }

        if matches!(provider, Provider::LmStudio | Provider::Ollama) {
            let subtitle = if provider == Provider::LmStudio {
                "Locally available LM Studio model"
            } else {
                "Locally available Ollama model"
            };
            for entry_index in &dynamic_indexes {
                if let Some(detail) = dynamic_models.detail(*entry_index) {
                    items.push(InlineListItem {
                        title: detail.model_display.clone(),
                        subtitle: Some(subtitle.to_string()),
                        badge: Some("Local".to_string()),
                        indent: 2,
                        selection: Some(InlineListSelection::DynamicModel(*entry_index)),
                        search_value: Some(format!(
                            "{} {}",
                            provider.label(),
                            detail.model_display
                        )),
                    });
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                items.push(InlineListItem {
                    title: format!("{} cache notice", provider.label()),
                    subtitle: Some(warning.to_string()),
                    badge: Some("Info".to_string()),
                    indent: 2,
                    selection: Some(InlineListSelection::RefreshDynamicModels),
                    search_value: Some(format!("{} cache", provider.label())),
                });
            }

            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    let instructions = if provider == Provider::LmStudio {
                        get_lmstudio_setup_instructions()
                    } else {
                        get_ollama_setup_instructions()
                    };
                    items.push(InlineListItem {
                        title: format!("{} server unreachable", provider.label()),
                        subtitle: Some(format!("{error}\n{instructions}")),
                        badge: Some("Info".to_string()),
                        indent: 2,
                        selection: Some(InlineListSelection::CustomModel),
                        search_value: Some(format!(
                            "{} setup",
                            provider.label().to_ascii_lowercase()
                        )),
                    });
                }
            }
        }
    }

    items.push(InlineListItem {
        title: "Refresh local LM Studio/Ollama models".to_string(),
        subtitle: Some(
            "Re-query LM Studio and Ollama servers without closing the picker.".to_string(),
        ),
        badge: Some("Action".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::RefreshDynamicModels),
        search_value: Some("refresh local models".to_string()),
    });

    items.push(InlineListItem {
        title: CUSTOM_PROVIDER_TITLE.to_string(),
        subtitle: Some(CUSTOM_PROVIDER_SUBTITLE.to_string()),
        badge: Some(CUSTOM_PROVIDER_BADGE.to_string()),
        indent: 0,
        selection: Some(InlineListSelection::CustomModel),
        search_value: Some(CUSTOM_PROVIDER_TITLE.to_string()),
    });

    let lines = vec![
        STEP_ONE_NAVIGATION_HINT.to_string(),
        format!("{CURRENT_REASONING_PREFIX}{current_reasoning}"),
    ];

    let search = InlineListSearchConfig {
        label: "Search models or providers".to_string(),
        placeholder: Some("Type to filter models".to_string()),
    };
    renderer.show_list_modal(STEP_ONE_TITLE, lines, items, None, Some(search));

    Ok(())
}

fn render_step_one_plain(
    renderer: &mut AnsiRenderer,
    options: &[ModelOption],
    dynamic_models: &DynamicModelRegistry,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Model picker – Step 1: select the model you want to use.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type '<provider> <model-id>' to select a model.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'cancel' to exit the picker at any time.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'refresh' to re-query LM Studio and Ollama servers.",
    )?;

    let mut grouped: HashMap<Provider, Vec<&ModelOption>> = HashMap::new();
    for option in options {
        grouped.entry(option.provider).or_default().push(option);
    }

    let mut first_section = true;
    for provider in picker_provider_order() {
        if provider == Provider::LmStudio {
            // Handle LM Studio specially by fetching dynamic models
            if !first_section {
                renderer.line(MessageStyle::Info, &provider_group_divider_line())?;
            }
            first_section = false;
            renderer.line(MessageStyle::Info, &format!("[{}]", provider.label()))?;
            if let Some(list) = grouped.get(&provider) {
                for option in list {
                    let reasoning_marker = if option.supports_reasoning {
                        " [reasoning]"
                    } else {
                        ""
                    };
                    renderer.line(
                        MessageStyle::Info,
                        &format!("  {} • {}{}", option.display, option.id, reasoning_marker),
                    )?;
                    renderer.line(MessageStyle::Info, &format!("      {}", option.description))?;
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                renderer.line(MessageStyle::Info, &format!("      note: {}", warning))?;
            }
            let dynamic_indexes = dynamic_models.indexes_for(provider);
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("LM Studio server not reachable ({error}) • Setup instructions:"),
                    )?;
                    for line in get_lmstudio_setup_instructions().lines() {
                        renderer.line(MessageStyle::Info, &format!("      {}", line))?;
                    }
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        renderer.line(
                            MessageStyle::Info,
                            &format!("  {} • {} (dynamic)", detail.model_display, detail.model_id),
                        )?;
                        renderer.line(
                            MessageStyle::Info,
                            "      Locally available LM Studio model",
                        )?;
                    }
                }
            }
        } else if provider == Provider::Ollama {
            // Handle Ollama specially by fetching dynamic local models
            if !first_section {
                renderer.line(MessageStyle::Info, &provider_group_divider_line())?;
            }
            first_section = false;
            renderer.line(MessageStyle::Info, &format!("[{}]", provider.label()))?;
            if let Some(list) = grouped.get(&provider) {
                for option in list {
                    let reasoning_marker = if option.supports_reasoning {
                        " [reasoning]"
                    } else {
                        ""
                    };
                    renderer.line(
                        MessageStyle::Info,
                        &format!("  {} • {}{}", option.display, option.id, reasoning_marker),
                    )?;
                    renderer.line(MessageStyle::Info, &format!("      {}", option.description))?;
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                renderer.line(MessageStyle::Info, &format!("      note: {}", warning))?;
            }
            let dynamic_indexes = dynamic_models.indexes_for(provider);
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Ollama server not reachable ({error}) • Setup instructions:"),
                    )?;
                    for line in get_ollama_setup_instructions().lines() {
                        renderer.line(MessageStyle::Info, &format!("      {}", line))?;
                    }
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        renderer.line(
                            MessageStyle::Info,
                            &format!("  {} • {} (local)", detail.model_display, detail.model_id),
                        )?;
                        renderer
                            .line(MessageStyle::Info, "      Locally available Ollama model")?;
                    }
                }
            }
        } else {
            // Handle other providers as before
            let Some(list) = grouped.get(&provider) else {
                continue;
            };
            if !first_section {
                renderer.line(MessageStyle::Info, &provider_group_divider_line())?;
            }
            first_section = false;
            renderer.line(MessageStyle::Info, &format!("[{}]", provider.label()))?;
            for option in list {
                let reasoning_marker = if option.supports_reasoning {
                    " [reasoning]"
                } else {
                    ""
                };
                renderer.line(
                    MessageStyle::Info,
                    &format!("  {} • {}{}", option.display, option.id, reasoning_marker),
                )?;
                renderer.line(MessageStyle::Info, &format!("      {}", option.description))?;
            }
        }
    }

    Ok(())
}

#[derive(Clone)]
struct ModelSelectionChoice {
    entry: SelectionEntry,
    outcome: ModelSelectionChoiceOutcome,
}

#[derive(Clone)]
enum ModelSelectionChoiceOutcome {
    Predefined(SelectionDetail),
    Manual,
    Refresh,
}

enum ModelSelectionListOutcome {
    Predefined(SelectionDetail),
    Manual,
    Refresh,
    Cancelled,
}

fn select_model_with_ratatui_list(
    options: &[ModelOption],
    current_reasoning: ReasoningEffortLevel,
    dynamic_models: &DynamicModelRegistry,
) -> Result<ModelSelectionListOutcome> {
    if options.is_empty() {
        return Err(anyhow!("No models available for selection"));
    }

    let mut choices = Vec::new();
    for provider in picker_provider_order() {
        if provider == Provider::LmStudio {
            let provider_models: Vec<&ModelOption> = options
                .iter()
                .filter(|option| option.provider == provider)
                .collect();

            // Add static LM Studio models first
            for option in &provider_models {
                let mut title = format!("{} • {}", provider.label(), option.display);
                if option.supports_reasoning {
                    title.push_str(" [Reasoning]");
                }
                let description = format!("ID: {} — {}", option.id, option.description);
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(title, Some(description)),
                    outcome: ModelSelectionChoiceOutcome::Predefined(selection_from_option(option)),
                });
            }
            let dynamic_indexes = dynamic_models.indexes_for(provider);
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    choices.push(ModelSelectionChoice {
                        entry: SelectionEntry::new(
                            "LM Studio server not running - Setup instructions",
                            Some(format!("{error}\n{}", get_lmstudio_setup_instructions())),
                        ),
                        outcome: ModelSelectionChoiceOutcome::Manual,
                    });
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        let title =
                            format!("{} • {} (dynamic)", provider.label(), detail.model_display);
                        let description = format!(
                            "ID: {} — Locally available LM Studio model",
                            detail.model_id
                        );
                        choices.push(ModelSelectionChoice {
                            entry: SelectionEntry::new(title, Some(description)),
                            outcome: ModelSelectionChoiceOutcome::Predefined(detail.clone()),
                        });
                    }
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(
                        format!("{} cache notice", provider.label()),
                        Some(format!(
                            "{warning} Select 'Refresh local models' to re-query."
                        )),
                    ),
                    outcome: ModelSelectionChoiceOutcome::Refresh,
                });
            }
        } else if provider == Provider::Ollama {
            let provider_models: Vec<&ModelOption> = options
                .iter()
                .filter(|option| option.provider == provider)
                .collect();

            // Add static Ollama models first
            for option in &provider_models {
                let mut title = format!("{} • {}", provider.label(), option.display);
                if option.supports_reasoning {
                    title.push_str(" [Reasoning]");
                }
                let description = format!("ID: {} — {}", option.id, option.description);
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(title, Some(description)),
                    outcome: ModelSelectionChoiceOutcome::Predefined(selection_from_option(option)),
                });
            }
            let dynamic_indexes = dynamic_models.indexes_for(provider);
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    choices.push(ModelSelectionChoice {
                        entry: SelectionEntry::new(
                            "Ollama server not running - Setup instructions",
                            Some(format!("{error}\n{}", get_ollama_setup_instructions())),
                        ),
                        outcome: ModelSelectionChoiceOutcome::Manual,
                    });
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        let title =
                            format!("{} • {} (local)", provider.label(), detail.model_display);
                        let description =
                            format!("ID: {} — Locally available Ollama model", detail.model_id);
                        choices.push(ModelSelectionChoice {
                            entry: SelectionEntry::new(title, Some(description)),
                            outcome: ModelSelectionChoiceOutcome::Predefined(detail.clone()),
                        });
                    }
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(
                        format!("{} cache notice", provider.label()),
                        Some(format!(
                            "{warning} Select 'Refresh local models' to re-query."
                        )),
                    ),
                    outcome: ModelSelectionChoiceOutcome::Refresh,
                });
            }
        } else {
            let provider_models: Vec<&ModelOption> = options
                .iter()
                .filter(|option| option.provider == provider)
                .collect();
            for option in &provider_models {
                let mut title = format!("{} • {}", provider.label(), option.display);
                if option.supports_reasoning {
                    title.push_str(" [Reasoning]");
                }
                let description = format!("ID: {} — {}", option.id, option.description);
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(title, Some(description)),
                    outcome: ModelSelectionChoiceOutcome::Predefined(selection_from_option(option)),
                });
            }
        }
    }

    choices.push(ModelSelectionChoice {
        entry: SelectionEntry::new(
            "Refresh local LM Studio/Ollama models",
            Some("Re-query local servers without closing the picker.".to_string()),
        ),
        outcome: ModelSelectionChoiceOutcome::Refresh,
    });

    choices.push(ModelSelectionChoice {
        entry: SelectionEntry::new(
            CUSTOM_PROVIDER_TITLE,
            Some(CUSTOM_PROVIDER_SUBTITLE.to_string()),
        ),
        outcome: ModelSelectionChoiceOutcome::Manual,
    });

    let entries: Vec<SelectionEntry> = choices.iter().map(|choice| choice.entry.clone()).collect();

    let instructions = format!(
        "Current reasoning effort: {}. Models marked with [Reasoning] support adjustable reasoning.",
        current_reasoning
    );

    let selection = run_interactive_selection("Models", &instructions, &entries, 0)?;
    let selected_index = match selection {
        Some(index) => index,
        None => return Ok(ModelSelectionListOutcome::Cancelled),
    };

    match &choices[selected_index].outcome {
        ModelSelectionChoiceOutcome::Predefined(detail) => {
            Ok(ModelSelectionListOutcome::Predefined(detail.clone()))
        }
        ModelSelectionChoiceOutcome::Manual => Ok(ModelSelectionListOutcome::Manual),
        ModelSelectionChoiceOutcome::Refresh => Ok(ModelSelectionListOutcome::Refresh),
    }
}

fn prompt_reasoning_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<()> {
    if selection.reasoning_optional {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – reasoning effort (current: {}). Choose easy/medium/hard or type 'skip' if the model does not expose configurable reasoning.",
                current
            ),
        )?
    } else if let Some(alternative) = selection.reasoning_off_model {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} (easy/medium/hard). Type 'skip' to keep {} or 'off' to use {} ({}).",
                selection.model_display,
                reasoning_level_label(current),
                alternative.display_name(),
                alternative.as_str()
            ),
        )?
    } else {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} (easy/medium/hard). Type 'skip' to keep {}. Current: {}.",
                selection.model_display,
                reasoning_level_label(current),
                current
            ),
        )?
    }
    Ok(())
}

fn render_reasoning_inline(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<()> {
    let mut items = Vec::new();
    items.push(InlineListItem {
        title: format!("Keep current ({})", reasoning_level_label(current)),
        subtitle: Some(KEEP_CURRENT_DESCRIPTION.to_string()),
        badge: Some(CURRENT_BADGE.to_string()),
        indent: 0,
        selection: Some(InlineListSelection::Reasoning(current)),
        search_value: None,
    });
    for level in [
        ReasoningEffortLevel::Low,
        ReasoningEffortLevel::Medium,
        ReasoningEffortLevel::High,
    ] {
        items.push(InlineListItem {
            title: reasoning_level_label(level).to_string(),
            subtitle: Some(reasoning_level_description(level).to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Reasoning(level)),
            search_value: None,
        });
    }
    if let Some(alternative) = selection.reasoning_off_model {
        items.push(InlineListItem {
            title: format!("Use {} (reasoning off)", alternative.display_name()),
            subtitle: Some(format!(
                "Switch to {} ({}) without enabling structured reasoning.",
                alternative.display_name(),
                alternative.as_str()
            )),
            badge: Some(REASONING_OFF_BADGE.to_string()),
            indent: 0,
            selection: Some(InlineListSelection::DisableReasoning),
            search_value: None,
        });
    }
    let mut lines = vec![
        format!(
            "Step 2 – select reasoning effort for {}.",
            selection.model_display
        ),
        STEP_TWO_NAVIGATION_HINT.to_string(),
    ];
    if let Some(alternative) = selection.reasoning_off_model {
        lines.push(format!(
            "Select \"Use {} (reasoning off)\" to switch to {}.",
            alternative.display_name(),
            alternative.as_str()
        ));
    }
    renderer.show_list_modal(
        STEP_TWO_TITLE,
        lines,
        items,
        Some(InlineListSelection::Reasoning(current)),
        None,
    );
    Ok(())
}

fn select_reasoning_with_ratatui(
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<Option<ReasoningChoice>> {
    let mut entries = vec![
        SelectionEntry::new(
            format!("Keep current ({})", reasoning_level_label(current)),
            Some(KEEP_CURRENT_DESCRIPTION.to_string()),
        ),
        SelectionEntry::new(
            reasoning_level_label(ReasoningEffortLevel::Low),
            Some(reasoning_level_description(ReasoningEffortLevel::Low).to_string()),
        ),
        SelectionEntry::new(
            reasoning_level_label(ReasoningEffortLevel::Medium),
            Some(reasoning_level_description(ReasoningEffortLevel::Medium).to_string()),
        ),
        SelectionEntry::new(
            reasoning_level_label(ReasoningEffortLevel::High),
            Some(reasoning_level_description(ReasoningEffortLevel::High).to_string()),
        ),
    ];

    let mut disable_index = None;
    if let Some(alternative) = selection.reasoning_off_model {
        entries.push(SelectionEntry::new(
            format!("Use {} (reasoning off)", alternative.display_name()),
            Some(format!(
                "Switch to {} ({}) without enabling structured reasoning.",
                alternative.display_name(),
                alternative.as_str()
            )),
        ));
        disable_index = Some(entries.len() - 1);
    }

    let mut instructions = format!(
        "Select reasoning effort for {}. Esc keeps the current level ({}).",
        selection.model_display,
        reasoning_level_label(current),
    );
    if let Some(alternative) = selection.reasoning_off_model {
        instructions.push(' ');
        instructions.push_str(&format!(
            "Choose \"Use {} (reasoning off)\" to switch to {}.",
            alternative.display_name(),
            alternative.as_str()
        ));
    }

    let selection_index =
        run_interactive_selection("Reasoning effort", &instructions, &entries, 0)?;

    let Some(index) = selection_index else {
        return Ok(None);
    };

    if disable_index == Some(index) {
        return Ok(Some(ReasoningChoice::Disable));
    }

    let choice = match index {
        0 => current,
        1 => ReasoningEffortLevel::Low,
        2 => ReasoningEffortLevel::Medium,
        3 => ReasoningEffortLevel::High,
        _ => current,
    };
    Ok(Some(ReasoningChoice::Level(choice)))
}

fn prompt_api_key_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    workspace: Option<&Path>,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Step 3 – enter an API key for {} (env: {}).",
            selection.provider_label, selection.env_key
        ),
    )?;
    if let Some(root) = workspace {
        let env_path = root.join(".env");
        renderer.line(
            MessageStyle::Info,
            &format!("The key will be stored in {}.", env_path.display()),
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            "The key will be stored in your workspace .env file.",
        )?;
    }
    renderer.line(
        MessageStyle::Info,
        "Paste the API key now or type 'skip' to reuse a stored credential.",
    )?;
    Ok(())
}

fn show_secure_api_modal(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    workspace: Option<&Path>,
) {
    let storage_line = workspace
        .map(|root| {
            let env_path = root.join(".env");
            format!("Stored in {}.", env_path.display())
        })
        .unwrap_or_else(|| "Stored in workspace .env file.".to_string());
    let mask_preview = "●●●●●●";
    let lines = vec![
        format!(
            "Bring your own key (BYOK) for {}.",
            selection.provider_label
        ),
        format!("Secure display hint: {}", mask_preview),
        storage_line,
        "Paste the key and press Enter when ready.".to_string(),
    ];
    let prompt_label = format!("{} API key", selection.provider_label);
    renderer.show_secure_prompt_modal("Secure API key setup", lines, prompt_label);
}

fn provider_group_divider_item() -> InlineListItem {
    InlineListItem {
        title: provider_group_divider_line(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
    }
}

fn provider_group_divider_line() -> String {
    let modal_width = usize::from(ui::MODAL_MIN_WIDTH);
    let title_width = STEP_ONE_TITLE.chars().count();
    let divider_width = modal_width.max(title_width);
    ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(divider_width)
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

impl ModelPickerState {
    fn find_existing_api_key(&self, env_key: &str) -> Result<Option<ExistingKey>> {
        if let Ok(value) = std::env::var(env_key) {
            if !value.trim().is_empty() {
                return Ok(Some(ExistingKey::Environment));
            }
        }

        if let Some(workspace) = self.workspace.as_deref() {
            if let Some(value) = read_workspace_env(workspace, env_key)? {
                if !value.trim().is_empty() {
                    return Ok(Some(ExistingKey::WorkspaceDotenv(value)));
                }
            }
        }

        Ok(None)
    }
}

impl DynamicModelRegistry {
    async fn load(options: &[ModelOption], workspace: Option<&Path>) -> Self {
        let endpoints = ProviderEndpointConfig::gather(workspace).await;
        let static_index = build_static_model_index(options);
        let mut cache_store = CachedDynamicModelStore::load().await;
        let (lmstudio_result, lmstudio_warning) = cache_store
            .fetch_with_cache(
                Provider::LmStudio,
                endpoints.lmstudio.clone(),
                fetch_lmstudio_models,
            )
            .await;
        let (ollama_result, ollama_warning) = cache_store
            .fetch_with_cache(
                Provider::Ollama,
                endpoints.ollama.clone(),
                fetch_ollama_models,
            )
            .await;
        let _ = cache_store.persist().await;
        let mut registry = Self::default();
        registry.process_fetch(
            Provider::LmStudio,
            lmstudio_result,
            endpoints
                .lmstudio
                .clone()
                .unwrap_or_else(|| urls::LMSTUDIO_API_BASE.to_string()),
            &static_index,
        );
        if let Some(warning) = lmstudio_warning {
            registry.record_warning(Provider::LmStudio, warning);
        }
        registry.process_fetch(
            Provider::Ollama,
            ollama_result,
            endpoints
                .ollama
                .clone()
                .unwrap_or_else(|| urls::OLLAMA_API_BASE.to_string()),
            &static_index,
        );
        if let Some(warning) = ollama_warning {
            registry.record_warning(Provider::Ollama, warning);
        }
        registry
    }

    fn indexes_for(&self, provider: Provider) -> Vec<usize> {
        self.provider_models
            .get(&provider)
            .cloned()
            .unwrap_or_default()
    }

    fn detail(&self, index: usize) -> Option<&SelectionDetail> {
        self.entries.get(index)
    }

    fn dynamic_detail(&self, index: usize) -> Option<SelectionDetail> {
        self.entries.get(index).cloned()
    }

    fn error_for(&self, provider: Provider) -> Option<&str> {
        self.provider_errors.get(&provider).map(|msg| msg.as_str())
    }

    fn warning_for(&self, provider: Provider) -> Option<&str> {
        self.provider_warnings
            .get(&provider)
            .map(|msg| msg.as_str())
    }

    fn process_fetch(
        &mut self,
        provider: Provider,
        result: Result<Vec<String>>,
        base_url: String,
        static_index: &StaticModelIndex,
    ) {
        match result {
            Ok(models) => self.register_provider_models(provider, models, static_index),
            Err(err) => {
                self.record_error(
                    provider,
                    format!(
                        "Failed to query {} at {} ({})",
                        provider.label(),
                        base_url,
                        err
                    ),
                );
            }
        }
    }

    fn register_provider_models(
        &mut self,
        provider: Provider,
        models: Vec<String>,
        static_index: &StaticModelIndex,
    ) {
        if !models.is_empty() {
            self.provider_errors.remove(&provider);
            self.provider_warnings.remove(&provider);
        }
        for model_id in models {
            let trimmed = model_id.trim();
            if trimmed.is_empty() {
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            if static_index
                .get(&provider)
                .map_or(false, |set| set.contains(&lower))
            {
                continue;
            }
            if self.has_model(provider, trimmed) {
                continue;
            }
            if provider == Provider::Ollama
                && (trimmed.contains(":cloud") || trimmed.contains("-cloud"))
            {
                continue;
            }
            let detail = selection_from_dynamic(provider, trimmed);
            self.register_model(provider, detail);
        }
    }

    fn register_model(&mut self, provider: Provider, detail: SelectionDetail) {
        let index = self.entries.len();
        self.entries.push(detail);
        self.provider_models
            .entry(provider)
            .or_default()
            .push(index);
    }

    fn has_model(&self, provider: Provider, candidate: &str) -> bool {
        if let Some(indexes) = self.provider_models.get(&provider) {
            for index in indexes {
                if let Some(entry) = self.entries.get(*index) {
                    if entry.model_id.eq_ignore_ascii_case(candidate) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn record_error(&mut self, provider: Provider, message: String) {
        self.provider_errors.insert(provider, message);
        self.provider_warnings.remove(&provider);
    }

    fn record_warning(&mut self, provider: Provider, message: String) {
        self.provider_warnings.insert(provider, message);
    }
}

#[derive(Default)]
struct CachedDynamicModelStore {
    path: Option<PathBuf>,
    entries: CacheEntries,
}

#[derive(Clone, Serialize, Deserialize)]
struct CachedDynamicModelEntry {
    provider: String,
    base_url: String,
    fetched_at: u64,
    models: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct CachedDynamicModelFile {
    version: u32,
    entries: Vec<CachedDynamicModelEntry>,
}

impl CachedDynamicModelStore {
    async fn load() -> Self {
        let path = dynamic_model_cache_path();
        let mut store = Self {
            path: path.clone(),
            entries: HashMap::new(),
        };
        if let Some(path) = path {
            if let Ok(bytes) = fs::read(&path).await {
                if let Ok(file) = serde_json::from_slice::<CachedDynamicModelFile>(&bytes) {
                    for entry in file.entries {
                        let key = format!("{}::{}", entry.provider, entry.base_url);
                        store.entries.insert(key, entry);
                    }
                }
            }
        }
        store
    }

    async fn persist(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.ok();
        }
        let file = CachedDynamicModelFile {
            version: 1,
            entries: self.entries.values().cloned().collect(),
        };
        let payload = serde_json::to_vec_pretty(&file)?;
        fs::write(path, payload).await?;
        Ok(())
    }

    async fn fetch_with_cache<F, Fut>(
        &mut self,
        provider: Provider,
        mut base_url: Option<String>,
        fetch_fn: F,
    ) -> (Result<Vec<String>>, Option<String>)
    where
        F: Fn(Option<String>) -> Fut,
        Fut: Future<Output = Result<Vec<String>, anyhow::Error>>,
    {
        if let Some(value) = base_url.take() {
            let trimmed = value.trim().trim_end_matches('/').to_string();
            if trimmed.is_empty() {
                base_url = None;
            } else {
                base_url = Some(trimmed);
            }
        }

        let resolved_base = base_url
            .clone()
            .unwrap_or_else(|| default_provider_base(provider).to_string());
        let key = Self::cache_key(provider, &resolved_base);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(entry) = self.entries.get(&key) {
            if now.saturating_sub(entry.fetched_at) <= DYNAMIC_MODEL_CACHE_TTL_SECS {
                return (Ok(entry.models.clone()), None);
            }
        }

        match fetch_fn(base_url.clone()).await {
            Ok(models) => {
                self.entries.insert(
                    key,
                    CachedDynamicModelEntry {
                        provider: provider.to_string(),
                        base_url: resolved_base,
                        fetched_at: now,
                        models: models.clone(),
                    },
                );
                (Ok(models), None)
            }
            Err(err) => {
                if let Some(entry) = self.entries.get(&key) {
                    let warning = format!(
                        "Using cached {} models fetched {}s ago because {} was unreachable ({}).",
                        provider.label(),
                        now.saturating_sub(entry.fetched_at),
                        resolved_base,
                        err
                    );
                    return (Ok(entry.models.clone()), Some(warning));
                }
                (Err(err), None)
            }
        }
    }

    fn cache_key(provider: Provider, base_url: &str) -> String {
        format!("{}::{}", provider, base_url)
    }
}

fn dynamic_model_cache_path() -> Option<PathBuf> {
    let manager = get_dot_manager().lock().ok()?.clone();
    Some(
        manager
            .cache_dir("models")
            .join(DYNAMIC_MODEL_CACHE_FILENAME),
    )
}

fn default_provider_base(provider: Provider) -> &'static str {
    match provider {
        Provider::LmStudio => urls::LMSTUDIO_API_BASE,
        Provider::Ollama => urls::OLLAMA_API_BASE,
        _ => "",
    }
}

impl ProviderEndpointConfig {
    async fn gather(workspace: Option<&Path>) -> Self {
        let _ = workspace;
        let dot_config = load_user_config().await.ok();
        Self {
            lmstudio: Self::extract_base_url(Provider::LmStudio, dot_config.as_ref()),
            ollama: Self::extract_base_url(Provider::Ollama, dot_config.as_ref()),
        }
    }

    fn extract_base_url(provider: Provider, dot_config: Option<&DotConfig>) -> Option<String> {
        let from_config = dot_config.and_then(|cfg| match provider {
            Provider::LmStudio => cfg
                .providers
                .lmstudio
                .as_ref()
                .and_then(|c| c.base_url.clone()),
            Provider::Ollama => cfg
                .providers
                .ollama
                .as_ref()
                .and_then(|c| c.base_url.clone()),
            _ => None,
        });

        from_config
            .and_then(Self::sanitize_owned)
            .or_else(|| Self::env_override(provider))
    }

    fn sanitize_owned(value: String) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn env_override(provider: Provider) -> Option<String> {
        let key = match provider {
            Provider::LmStudio => env_vars::LMSTUDIO_BASE_URL,
            Provider::Ollama => env_vars::OLLAMA_BASE_URL,
            _ => return None,
        };
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

fn build_static_model_index(options: &[ModelOption]) -> StaticModelIndex {
    let mut index = HashMap::new();
    for option in options {
        index
            .entry(option.provider)
            .or_insert_with(HashSet::new)
            .insert(option.id.to_ascii_lowercase());
    }
    index
}

fn prompt_custom_model_entry(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Enter a provider and model identifier (examples: 'openai gpt-5-nano', 'ollama qwen3:1.7b').",
    )?;
    renderer.line(
        MessageStyle::Info,
        "For Ollama, you can use any locally available model like 'llama3:8b', 'mistral:7b', etc.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'cancel' to exit the picker at any time.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'refresh' to reload LM Studio and Ollama model lists.",
    )?;
    Ok(())
}

fn reasoning_level_label(level: ReasoningEffortLevel) -> &'static str {
    match level {
        ReasoningEffortLevel::Low => reasoning::LABEL_LOW,
        ReasoningEffortLevel::Medium => reasoning::LABEL_MEDIUM,
        ReasoningEffortLevel::High => reasoning::LABEL_HIGH,
    }
}

fn reasoning_level_description(level: ReasoningEffortLevel) -> &'static str {
    match level {
        ReasoningEffortLevel::Low => reasoning::DESCRIPTION_LOW,
        ReasoningEffortLevel::Medium => reasoning::DESCRIPTION_MEDIUM,
        ReasoningEffortLevel::High => reasoning::DESCRIPTION_HIGH,
    }
}

fn parse_model_selection(options: &[ModelOption], input: &str) -> Result<SelectionDetail> {
    if let Ok(index) = input.parse::<usize>() {
        if let Some(option) = options.get(index) {
            return Ok(selection_from_option(option));
        }
        return Err(anyhow!(
            "Invalid model selection. Use provider and model name (e.g., 'openai gpt-5')"
        ));
    }

    let mut parts = input.split_whitespace();
    let Some(provider_token) = parts.next() else {
        return Err(anyhow!("Please provide a provider and model identifier."));
    };
    let model_token = parts.collect::<Vec<&str>>().join(" ");
    if model_token.trim().is_empty() {
        return Err(anyhow!(
            "Provide both provider and model. Example: 'openai gpt-5'"
        ));
    }

    let provider_lower = provider_token.to_ascii_lowercase();
    let provider_enum = Provider::from_str(&provider_lower).ok();

    if let Some(option) = options
        .iter()
        .find(|candidate| candidate.id.eq_ignore_ascii_case(model_token.trim()))
    {
        if let Some(provider) = provider_enum {
            if provider == option.provider {
                return Ok(selection_from_option(option));
            }
        }
    }

    let provider_label = provider_enum
        .map(|provider| provider.label().to_string())
        .unwrap_or_else(|| title_case(&provider_lower));
    let env_key = provider_enum
        .map(|provider| provider.default_api_key_env().to_string())
        .unwrap_or_else(|| derive_env_key(&provider_lower));
    let reasoning_supported = provider_enum
        .map(|provider| provider.supports_reasoning_effort(model_token.trim()))
        .unwrap_or(false);
    let requires_api_key = if let Some(provider) = provider_enum {
        provider_requires_api_key(provider, model_token.trim(), &env_key)
    } else {
        match std::env::var(&env_key) {
            Ok(value) => value.trim().is_empty(),
            Err(_) => true,
        }
    };

    Ok(SelectionDetail {
        provider_key: provider_lower,
        provider_label,
        provider_enum,
        model_id: model_token.trim().to_string(),
        model_display: model_token.trim().to_string(),
        known_model: false,
        reasoning_supported,
        reasoning_optional: true,
        reasoning_off_model: None,
        requires_api_key,
        env_key,
    })
}

fn selection_from_option(option: &ModelOption) -> SelectionDetail {
    let env_key = option.provider.default_api_key_env().to_string();
    let requires_api_key = provider_requires_api_key(option.provider, option.id, &env_key);
    SelectionDetail {
        provider_key: option.provider.to_string(),
        provider_label: option.provider.label().to_string(),
        provider_enum: Some(option.provider),
        model_id: option.id.to_string(),
        model_display: option.display.to_string(),
        known_model: true,
        reasoning_supported: option.supports_reasoning,
        reasoning_optional: false,
        reasoning_off_model: option.reasoning_alternative,
        requires_api_key,
        env_key,
    }
}

fn selection_from_dynamic(provider: Provider, model_id: &str) -> SelectionDetail {
    let env_key = provider.default_api_key_env().to_string();
    let requires_api_key = provider_requires_api_key(provider, model_id, &env_key);
    SelectionDetail {
        provider_key: provider.to_string(),
        provider_label: provider.label().to_string(),
        provider_enum: Some(provider),
        model_id: model_id.to_string(),
        model_display: model_id.to_string(),
        known_model: false,
        reasoning_supported: provider.supports_reasoning_effort(model_id),
        reasoning_optional: true,
        reasoning_off_model: None,
        requires_api_key,
        env_key,
    }
}

fn is_cancel_command(input: &str) -> bool {
    matches!(
        input.to_ascii_lowercase().as_str(),
        "cancel" | "/cancel" | "abort" | "quit"
    )
}

fn derive_env_key(provider: &str) -> String {
    let mut key = String::new();
    for ch in provider.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_uppercase());
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }
    if key.is_empty() {
        key.push_str("LLM");
    }
    if !key.ends_with("_API_KEY") {
        if !key.ends_with('_') {
            key.push('_');
        }
        key.push_str("API_KEY");
    }
    key
}

fn provider_requires_api_key(provider: Provider, model_id: &str, env_key: &str) -> bool {
    if provider == Provider::Ollama {
        let is_cloud_model = model_id.contains(":cloud") || model_id.contains("-cloud");
        if !is_cloud_model {
            return false;
        }
    }
    if provider == Provider::LmStudio {
        return false;
    }

    match std::env::var(env_key) {
        Ok(value) => value.trim().is_empty(),
        Err(_) => true,
    }
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut result = String::new();
    result.push(first.to_ascii_uppercase());
    result.push_str(&chars.as_str().to_ascii_lowercase());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::fs;
    use tempfile::tempdir;

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
