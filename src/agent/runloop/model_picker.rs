use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::str::FromStr;

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::ReasoningEffortLevel;
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
    current_reasoning: ReasoningEffortLevel,
    selection: Option<SelectionDetail>,
    selected_reasoning: Option<ReasoningEffortLevel>,
    pending_api_key: Option<String>,
}

impl ModelPickerState {
    pub fn new(
        renderer: &mut AnsiRenderer,
        current_reasoning: ReasoningEffortLevel,
    ) -> Result<Self> {
        let options = MODEL_OPTIONS.as_slice();
        render_step_one(renderer, options)?;
        Ok(Self {
            options,
            step: PickerStep::AwaitModel,
            current_reasoning,
            selection: None,
            selected_reasoning: None,
            pending_api_key: None,
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

        let message = format!(
            "Selected {} ({}) from {}.",
            selection.model_display, selection.model_id, selection.provider_label
        );
        renderer.line(MessageStyle::Info, &message)?;

        self.selection = Some(selection);
        if self
            .selection
            .as_ref()
            .map(|detail| detail.reasoning_supported)
            .unwrap_or(false)
        {
            self.step = PickerStep::AwaitReasoning;
            prompt_reasoning(
                renderer,
                self.selection.as_ref().unwrap(),
                self.current_reasoning,
            )?;
            return Ok(ModelPickerProgress::InProgress);
        }

        if self
            .selection
            .as_ref()
            .map(|detail| detail.requires_api_key)
            .unwrap_or(false)
        {
            self.step = PickerStep::AwaitApiKey;
            prompt_api_key(renderer, self.selection.as_ref().unwrap())?;
            return Ok(ModelPickerProgress::InProgress);
        }

        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
    }

    fn handle_reasoning(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };

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
            prompt_reasoning(renderer, selection, self.current_reasoning)?;
            return Ok(ModelPickerProgress::InProgress);
        };

        self.selected_reasoning = Some(selected);
        if selection.requires_api_key {
            self.step = PickerStep::AwaitApiKey;
            prompt_api_key(renderer, selection)?;
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
            match std::env::var(&selection.env_key) {
                Ok(value) if !value.trim().is_empty() => {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Using existing environment variable {} for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                    self.pending_api_key = None;
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                _ => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Environment variable {} is not set. Please provide an API key.",
                            selection.env_key
                        ),
                    )?;
                    prompt_api_key(renderer, selection)?;
                    return Ok(ModelPickerProgress::InProgress);
                }
            }
        }

        self.pending_api_key = Some(input.to_string());
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

fn render_step_one(renderer: &mut AnsiRenderer, options: &[ModelOption]) -> Result<()> {
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

    for provider in Provider::all_providers() {
        let Some(list) = grouped.get(&provider) else {
            continue;
        };
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
    }

    Ok(())
}

fn prompt_reasoning(
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

fn prompt_api_key(renderer: &mut AnsiRenderer, selection: &SelectionDetail) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Step 3 – enter an API key for {} (env: {}).",
            selection.provider_label, selection.env_key
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        "Paste the API key now or type 'skip' to reuse the existing environment value.",
    )?;
    Ok(())
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
    let requires_api_key = match std::env::var(&env_key) {
        Ok(value) => value.trim().is_empty(),
        Err(_) => true,
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
    let requires_api_key = match std::env::var(&env_key) {
        Ok(value) => value.trim().is_empty(),
        Err(_) => true,
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
