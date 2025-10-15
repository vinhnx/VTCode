use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use vtcode_core::config::constants::{reasoning, ui};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListItem, InlineListSearchConfig, InlineListSelection};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::update_model_preference;

#[derive(Clone, Copy)]
struct ModelOption {
    index: usize,
    provider: Provider,
    id: &'static str,
    display: &'static str,
    description: &'static str,
    supports_reasoning: bool,
}

static MODEL_OPTIONS: Lazy<Vec<ModelOption>> = Lazy::new(|| {
    let mut index = 1usize;
    let mut options = Vec::new();
    for model in ModelId::all_models() {
        options.push(ModelOption {
            index,
            provider: model.provider(),
            id: model.as_str(),
            display: model.display_name(),
            description: model.description(),
            supports_reasoning: model.supports_reasoning_effort(),
        });
        index += 1;
    }
    options
});

const STEP_ONE_TITLE: &str = "Model picker – Step 1";
const STEP_TWO_TITLE: &str = "Model picker – Step 2";
const STEP_ONE_NAVIGATION_HINT: &str = "Use ↑/↓ to navigate, Enter to select, or Esc to cancel.";
const STEP_TWO_NAVIGATION_HINT: &str = "Use ↑/↓ to navigate, Enter to choose, or Esc to cancel.";
const CUSTOM_PROVIDER_TITLE: &str = "Custom provider + model";
const CUSTOM_PROVIDER_SUBTITLE: &str = "Provide the provider name and model identifier manually.";
const CUSTOM_PROVIDER_BADGE: &str = "Manual";
const REASONING_BADGE: &str = "Reasoning";
const CURRENT_BADGE: &str = "Current";
const CURRENT_REASONING_PREFIX: &str = "Current reasoning effort: ";
const KEEP_CURRENT_DESCRIPTION: &str = "Retain the existing reasoning configuration.";

#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerStep {
    AwaitModel,
    AwaitReasoning,
    AwaitApiKey,
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
}

impl ModelPickerState {
    pub fn new(
        renderer: &mut AnsiRenderer,
        current_reasoning: ReasoningEffortLevel,
        workspace: Option<PathBuf>,
    ) -> Result<Self> {
        let options = MODEL_OPTIONS.as_slice();
        let inline_enabled = renderer.supports_inline_ui();
        if inline_enabled {
            render_step_one_inline(renderer, options, current_reasoning)?;
        } else {
            render_step_one_plain(renderer, options)?;
        }
        Ok(Self {
            options,
            step: PickerStep::AwaitModel,
            inline_enabled,
            current_reasoning,
            selection: None,
            selected_reasoning: None,
            pending_api_key: None,
            workspace,
        })
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

        match self.step {
            PickerStep::AwaitModel => self.handle_model_selection(renderer, trimmed),
            PickerStep::AwaitReasoning => self.handle_reasoning(renderer, trimmed),
            PickerStep::AwaitApiKey => self.handle_api_key(renderer, trimmed),
        }
    }

    pub fn persist_selection(
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
        config.agent.api_key_env = selection.env_key.clone();
        config.agent.default_model = selection.model.clone();
        config.agent.reasoning_effort = selection.reasoning;
        if let Some(ref api_key) = selection.api_key {
            config
                .agent
                .custom_api_keys
                .insert(selection.provider.clone(), api_key.clone());
        } else {
            config.agent.custom_api_keys.remove(&selection.provider);
        }
        config.router.models.simple = selection.model.clone();
        config.router.models.standard = selection.model.clone();
        config.router.models.complex = selection.model.clone();
        config.router.models.codegen_heavy = selection.model.clone();
        config.router.models.retrieval_heavy = selection.model.clone();
        manager.save_config(&config)?;
        update_model_preference(&selection.provider, &selection.model).ok();
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
                    let Some(option) = self.options.iter().find(|item| item.index == index) else {
                        renderer.line(
                            MessageStyle::Error,
                            "Unable to locate the selected model option.",
                        )?;
                        return Ok(ModelPickerProgress::InProgress);
                    };
                    let detail = selection_from_option(option);
                    self.process_model_selection(renderer, detail)
                }
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
                InlineListSelection::Theme(_)
                | InlineListSelection::Session(_)
                | InlineListSelection::SlashCommand(_) => Ok(ModelPickerProgress::InProgress),
            },
            PickerStep::AwaitReasoning => match choice {
                InlineListSelection::Reasoning(level) => {
                    renderer.close_modal();
                    self.apply_reasoning_choice(renderer, level)
                }
                InlineListSelection::CustomModel | InlineListSelection::Model(_) => {
                    renderer.line(
                        MessageStyle::Error,
                        "Reasoning selection is active. Choose a reasoning level or press Esc to cancel.",
                    )?;
                    Ok(ModelPickerProgress::InProgress)
                }
                InlineListSelection::Theme(_)
                | InlineListSelection::Session(_)
                | InlineListSelection::SlashCommand(_) => Ok(ModelPickerProgress::InProgress),
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
                    "Try again with a model number or '<provider> <model-id>'.",
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
                "Unknown reasoning level. Use easy, medium, hard, or skip.",
            )?;
            self.prompt_reasoning_step(renderer)?;
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
            self.prompt_reasoning_step(renderer)?;
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

    fn prompt_reasoning_step(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };
        if self.inline_enabled {
            render_reasoning_inline(renderer, selection, self.current_reasoning)?;
        } else {
            prompt_reasoning_plain(renderer, selection, self.current_reasoning)?;
        }
        Ok(())
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

fn render_step_one_inline(
    renderer: &mut AnsiRenderer,
    options: &[ModelOption],
    current_reasoning: ReasoningEffortLevel,
) -> Result<()> {
    let mut items = Vec::new();
    let mut first_section = true;
    for provider in Provider::all_providers() {
        let provider_models: Vec<&ModelOption> = options
            .iter()
            .filter(|candidate| candidate.provider == provider)
            .collect();
        if provider_models.is_empty() {
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
        for option in provider_models {
            let badge = option
                .supports_reasoning
                .then(|| REASONING_BADGE.to_string());
            items.push(InlineListItem {
                title: option.display.to_string(),
                subtitle: Some(option.description.to_string()),
                badge,
                indent: 2,
                selection: Some(InlineListSelection::Model(option.index)),
                search_value: Some(format!(
                    "{} {} {}",
                    provider.label(),
                    option.display,
                    option.id
                )),
            });
        }

        // Add custom Ollama model option when in the Ollama provider section
        if provider == Provider::Ollama {
            items.push(InlineListItem {
                title: "Custom Ollama model".to_string(),
                subtitle: Some(
                    "Enter a custom Ollama model ID (e.g., qwen3:1.7b, llama3:8b, etc.)"
                        .to_string(),
                ),
                badge: Some("Local".to_string()),
                indent: 2,
                selection: Some(InlineListSelection::CustomModel),
                search_value: Some("ollama custom".to_string()),
            });
        }
    }

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

fn render_step_one_plain(renderer: &mut AnsiRenderer, options: &[ModelOption]) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Model picker – Step 1: select the model you want to use.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Enter the number next to a model or type '<provider> <model-id>' for custom entries.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'cancel' to exit the picker at any time.",
    )?;

    let mut grouped: HashMap<Provider, Vec<&ModelOption>> = HashMap::new();
    for option in options {
        grouped.entry(option.provider).or_default().push(option);
    }

    let mut first_section = true;
    for provider in Provider::all_providers() {
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
                &format!(
                    "  ({}) {} • {}{}",
                    option.index, option.display, option.id, reasoning_marker
                ),
            )?;
            renderer.line(MessageStyle::Info, &format!("      {}", option.description))?;
        }

        // Add custom Ollama model option when in the Ollama provider section
        if provider == Provider::Ollama {
            renderer.line(
                MessageStyle::Info,
                "  (custom-ollama) Custom Ollama model • Enter any Ollama model ID",
            )?;
            renderer.line(
                MessageStyle::Info,
                "      Enter a custom Ollama model ID (e.g., qwen3:1.7b, llama3:8b, etc.)",
            )?;
        }
    }

    Ok(())
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
    } else {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} (easy/medium/hard). Current: {}.",
                selection.model_display, current
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
    let lines = vec![
        format!(
            "Step 2 – select reasoning effort for {}.",
            selection.model_display
        ),
        STEP_TWO_NAVIGATION_HINT.to_string(),
    ];
    renderer.show_list_modal(
        STEP_TWO_TITLE,
        lines,
        items,
        Some(InlineListSelection::Reasoning(current)),
        None,
    );
    Ok(())
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

fn prompt_custom_model_entry(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Enter a provider and model identifier (examples: 'openai gpt-4o-mini', 'ollama qwen3:1.7b').",
    )?;
    renderer.line(
        MessageStyle::Info,
        "For Ollama, you can use any locally available model like 'llama3:8b', 'mistral:7b', etc.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'cancel' to exit the picker at any time.",
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
        if let Some(option) = options.iter().find(|candidate| candidate.index == index) {
            return Ok(selection_from_option(option));
        }
        return Err(anyhow!("No model with number {}", index));
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
    let requires_api_key = if provider_enum == Some(Provider::Ollama) {
        false
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
        requires_api_key,
        env_key,
    })
}

fn selection_from_option(option: &ModelOption) -> SelectionDetail {
    let env_key = option.provider.default_api_key_env().to_string();
    let requires_api_key = if option.provider == Provider::Ollama {
        false
    } else {
        match std::env::var(&env_key) {
            Ok(value) => value.trim().is_empty(),
            Err(_) => true,
        }
    };
    SelectionDetail {
        provider_key: option.provider.to_string(),
        provider_label: option.provider.label().to_string(),
        provider_enum: Some(option.provider),
        model_id: option.id.to_string(),
        model_display: option.display.to_string(),
        known_model: true,
        reasoning_supported: option.supports_reasoning,
        reasoning_optional: false,
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
