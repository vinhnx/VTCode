use anyhow::{Result, anyhow};
use std::str::FromStr;

use vtcode_config::OpenAIServiceTier;
use vtcode_core::config::constants::reasoning;
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::ReasoningEffortLevel;

use super::options::{ModelOption, find_option_index};

#[derive(Clone)]
pub(super) struct SelectionDetail {
    pub(super) provider_key: String,
    pub(super) provider_label: String,
    pub(super) provider_enum: Option<Provider>,
    pub(super) model_id: String,
    pub(super) model_display: String,
    pub(super) known_model: bool,
    pub(super) reasoning_supported: bool,
    pub(super) reasoning_optional: bool,
    pub(super) reasoning_off_model: Option<ModelId>,
    pub(super) service_tier_supported: bool,
    pub(super) requires_api_key: bool,
    pub(super) uses_chatgpt_auth: bool,
    pub(super) env_key: String,
}

#[derive(Clone, Copy)]
pub(super) enum ReasoningChoice {
    Level(ReasoningEffortLevel),
    Disable,
}

#[derive(Clone, Copy)]
pub(super) enum ServiceTierChoice {
    ProjectDefault,
    Priority,
}

pub(super) enum ExistingKey {
    Environment,
    WorkspaceDotenv,
    /// OAuth token from encrypted storage (for OpenRouter)
    OAuthToken,
}

#[derive(Clone)]
pub(crate) struct ModelSelectionResult {
    pub(crate) provider: String,
    pub(crate) provider_label: String,
    pub(crate) provider_enum: Option<Provider>,
    pub(crate) model: String,
    pub(crate) model_display: String,
    pub(crate) known_model: bool,
    pub(crate) reasoning_supported: bool,
    pub(crate) reasoning: ReasoningEffortLevel,
    pub(crate) reasoning_changed: bool,
    pub(crate) service_tier_supported: bool,
    pub(crate) service_tier: Option<OpenAIServiceTier>,
    pub(crate) service_tier_changed: bool,
    pub(crate) api_key: Option<String>,
    pub(crate) env_key: String,
    pub(crate) requires_api_key: bool,
    pub(crate) uses_chatgpt_auth: bool,
}

pub(super) fn parse_model_selection(
    options: &[ModelOption],
    input: &str,
) -> Result<SelectionDetail> {
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

    if let Some(provider) = provider_enum
        && let Some(option_index) = find_option_index(provider, model_token.trim())
        && let Some(option) = options.get(option_index)
    {
        return Ok(selection_from_option(option));
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
    let service_tier_supported = provider_enum
        .map(|provider| provider.supports_service_tier(model_token.trim()))
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
        service_tier_supported,
        requires_api_key,
        uses_chatgpt_auth: false,
        env_key,
    })
}

pub(super) fn selection_from_option(option: &ModelOption) -> SelectionDetail {
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
        service_tier_supported: option.provider.supports_service_tier(option.id),
        requires_api_key,
        uses_chatgpt_auth: false,
        env_key,
    }
}

pub(super) fn selection_from_dynamic(provider: Provider, model_id: &str) -> SelectionDetail {
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
        service_tier_supported: provider.supports_service_tier(model_id),
        requires_api_key,
        uses_chatgpt_auth: false,
        env_key,
    }
}

pub(super) fn reasoning_level_label(level: ReasoningEffortLevel) -> &'static str {
    match level {
        ReasoningEffortLevel::None => "None (Fast)",
        ReasoningEffortLevel::Minimal => "Minimal (Fastest)",
        ReasoningEffortLevel::Low => reasoning::LABEL_LOW,
        ReasoningEffortLevel::Medium => reasoning::LABEL_MEDIUM,
        ReasoningEffortLevel::High => reasoning::LABEL_HIGH,
        ReasoningEffortLevel::XHigh => "Extra High",
    }
}

pub(super) fn supports_gpt5_none_reasoning(model_id: &str) -> bool {
    matches!(model_id, "gpt-5.2" | "gpt-5.4" | "gpt-5.4-pro")
        || matches!(model_id, "gpt-5.2-codex" | "gpt-5.3-codex")
}

pub(super) fn supports_xhigh_reasoning(model_id: &str) -> bool {
    matches!(
        model_id,
        "gpt-5.2" | "gpt-5.2-codex" | "gpt-5.4" | "gpt-5.4-pro" | "gpt-5.3-codex"
    )
}

pub(super) fn reasoning_level_description(level: ReasoningEffortLevel) -> &'static str {
    match level {
        ReasoningEffortLevel::None => "No reasoning overhead - fastest responses",
        ReasoningEffortLevel::Minimal => "Minimal reasoning overhead - very fast responses",
        ReasoningEffortLevel::Low => reasoning::DESCRIPTION_LOW,
        ReasoningEffortLevel::Medium => reasoning::DESCRIPTION_MEDIUM,
        ReasoningEffortLevel::High => reasoning::DESCRIPTION_HIGH,
        ReasoningEffortLevel::XHigh => {
            "Maximum reasoning for hardest long-running tasks (GPT-5.3+/GPT-5.4 family only)"
        }
    }
}

pub(super) fn service_tier_label(service_tier: Option<OpenAIServiceTier>) -> &'static str {
    match service_tier {
        Some(OpenAIServiceTier::Priority) => "Priority",
        None => "Project default",
    }
}

pub(super) fn is_cancel_command(input: &str) -> bool {
    matches!(
        input.to_ascii_lowercase().as_str(),
        "cancel" | "/cancel" | "abort" | "quit"
    )
}

pub(super) fn derive_env_key(provider: &str) -> String {
    let mut key = String::new();
    for ch in provider.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_uppercase());
        } else if !key.ends_with('_') {
            key.push('_');
        }
    }
    if !key.ends_with("_API_KEY") {
        if !key.ends_with('_') {
            key.push('_');
        }
        key.push_str("API_KEY");
    }
    key
}

pub(super) fn provider_requires_api_key(provider: Provider, model_id: &str, env_key: &str) -> bool {
    if provider == Provider::Ollama {
        let is_cloud_model = model_id.contains(":cloud") || model_id.contains("-cloud");
        if !is_cloud_model {
            return false;
        }
    }

    // OAuth-backed credentials can satisfy provider auth without a pasted API key.
    if provider == Provider::OpenRouter
        && let Ok(Some(_token)) = vtcode_config::auth::load_oauth_token()
    {
        return false;
    }

    if provider == Provider::OpenAI
        && let Ok(Some(_session)) = vtcode_config::auth::load_openai_chatgpt_session()
    {
        return false;
    }

    match std::env::var(env_key) {
        Ok(value) => value.trim().is_empty(),
        Err(_) => true,
    }
}

pub(super) fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut result = String::new();
    result.push(first.to_ascii_uppercase());
    result.push_str(&chars.as_str().to_ascii_lowercase());
    result
}
