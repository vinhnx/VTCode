use crate::config::core::{PromptCachingConfig, ProviderPromptCachingConfig};
use crate::llm::error_display;
use crate::llm::provider::{FinishReason, LLMError, LLMRequest, Message, ToolCall};
use crate::llm::types as llm_types;
use serde_json::Value;

pub fn resolve_model(model: Option<String>, default_model: &str) -> String {
    model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_model.to_string())
}

/// Creates a default LLM request with a single user message.
/// Used by all providers for their LLMClient implementation.
#[inline]
pub fn make_default_request(prompt: &str, model: &str) -> LLMRequest {
    LLMRequest {
        messages: vec![Message::user(prompt.to_string())],
        system_prompt: None,
        tools: None,
        model: model.to_string(),
        max_tokens: None,
        temperature: None,
        stream: false,
        output_format: None,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
        verbosity: None,
    }
}

/// Parses a client prompt that may be a JSON chat request or plain text.
/// Returns a parsed LLMRequest from JSON if valid, or a default request with the prompt.
#[inline]
pub fn parse_client_prompt_common<F>(prompt: &str, model: &str, parse_json: F) -> LLMRequest
where
    F: FnOnce(&Value) -> Option<LLMRequest>,
{
    let trimmed = prompt.trim_start();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if let Some(request) = parse_json(&value) {
                return request;
            }
        }
    }
    make_default_request(prompt, model)
}

/// Converts provider Usage to llm_types::Usage.
/// Shared by all LLMClient implementations.
#[inline]
pub fn convert_usage_to_llm_types(usage: crate::llm::provider::Usage) -> llm_types::Usage {
    llm_types::Usage {
        prompt_tokens: usage.prompt_tokens as usize,
        completion_tokens: usage.completion_tokens as usize,
        total_tokens: usage.total_tokens as usize,
        cached_prompt_tokens: usage.cached_prompt_tokens.map(|v| v as usize),
        cache_creation_tokens: usage.cache_creation_tokens.map(|v| v as usize),
        cache_read_tokens: usage.cache_read_tokens.map(|v| v as usize),
    }
}

pub fn override_base_url(
    default_base_url: &str,
    base_url: Option<String>,
    env_var_name: Option<&str>,
) -> String {
    if let Some(url) = base_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Some(var_name) = env_var_name {
        if let Ok(value) = std::env::var(var_name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    default_base_url.to_string()
}

pub fn extract_prompt_cache_settings<T, SelectFn, EnabledFn>(
    prompt_cache: Option<PromptCachingConfig>,
    select_settings: SelectFn,
    enabled: EnabledFn,
) -> (bool, T)
where
    T: Clone + Default,
    SelectFn: Fn(&ProviderPromptCachingConfig) -> &T,
    EnabledFn: Fn(&PromptCachingConfig, &T) -> bool,
{
    if let Some(cfg) = prompt_cache {
        let provider_settings = select_settings(&cfg.providers).clone();
        let is_enabled = enabled(&cfg, &provider_settings);
        (is_enabled, provider_settings)
    } else {
        (false, T::default())
    }
}

pub fn forward_prompt_cache_with_state<PredicateFn>(
    prompt_cache: Option<PromptCachingConfig>,
    predicate: PredicateFn,
    default_enabled: bool,
) -> (bool, Option<PromptCachingConfig>)
where
    PredicateFn: Fn(&PromptCachingConfig) -> bool,
{
    match prompt_cache {
        Some(cfg) => {
            if predicate(&cfg) {
                (true, Some(cfg))
            } else {
                (false, None)
            }
        }
        None => (default_enabled, None),
    }
}

/// Parses a tool call from OpenAI-compatible JSON format.
/// Works for DeepSeek, ZAI, and other OpenAI-compatible providers.
#[inline]
pub fn parse_tool_call_openai_format(value: &Value) -> Option<ToolCall> {
    let id = value.get("id").and_then(|v| v.as_str())?;
    let function = value.get("function")?;
    let name = function.get("name").and_then(|v| v.as_str())?;
    let arguments = function.get("arguments").map(|arg| {
        if let Some(text) = arg.as_str() {
            text.to_string()
        } else {
            arg.to_string()
        }
    });

    Some(ToolCall::function(
        id.to_string(),
        name.to_string(),
        arguments.unwrap_or_else(|| "{}".to_string()),
    ))
}

/// Maps common finish reason strings to FinishReason enum.
/// Handles standard OpenAI-compatible finish reasons.
#[inline]
pub fn map_finish_reason_common(reason: &str) -> FinishReason {
    match reason {
        "stop" | "completed" | "done" | "finished" => FinishReason::Stop,
        "length" => FinishReason::Length,
        "tool_calls" => FinishReason::ToolCalls,
        "content_filter" | "sensitive" => FinishReason::ContentFilter,
        other => FinishReason::Error(other.to_string()),
    }
}

/// Validates an LLM request with common checks.
/// Checks for empty messages and validates each message for the given provider.
pub fn validate_request_common(
    request: &LLMRequest,
    provider_name: &str,
    validation_provider: &str,
    supported_models: Option<&[String]>,
) -> Result<(), LLMError> {
    if request.messages.is_empty() {
        let formatted =
            error_display::format_llm_error(provider_name, "Messages cannot be empty");
        return Err(LLMError::InvalidRequest(formatted));
    }

    if let Some(models) = supported_models {
        if !request.model.trim().is_empty() && !models.contains(&request.model) {
            let formatted = error_display::format_llm_error(
                provider_name,
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest(formatted));
        }
    }

    for message in &request.messages {
        if let Err(err) = message.validate_for_provider(validation_provider) {
            let formatted = error_display::format_llm_error(provider_name, &err);
            return Err(LLMError::InvalidRequest(formatted));
        }
    }

    Ok(())
}
