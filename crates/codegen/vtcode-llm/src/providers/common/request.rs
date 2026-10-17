//! Shared request defaults, model validation, and system directives.

use crate::error_display;
use crate::provider::{LLMError, LLMRequest, Message, MessageRole};
use serde_json::Value;

/// Widens an f32 sampling parameter to f64 through its shortest round-trip
/// decimal form. Direct `f32 as f64` promotion keeps the binary tail
/// (`0.7f32` → `0.699999988079071`), which strict backends reject as invalid
/// precision; routing the value through its shortest repr keeps the wire form
/// compact (`0.7`) without losing round-trip fidelity.
pub(crate) fn sampling_param_f64(value: f32) -> f64 {
    if !value.is_finite() {
        return f64::from(value);
    }
    format!("{value}").parse().unwrap_or_else(|_| f64::from(value))
}

/// Converts a float parameter (temperature, top_p, …) into a JSON number,
/// rejecting NaN and infinity with an `LLMError::InvalidRequest`.
pub(crate) fn float_to_json_number(value: f32) -> Result<serde_json::Number, LLMError> {
    serde_json::Number::from_f64(sampling_param_f64(value)).ok_or_else(|| LLMError::InvalidRequest {
        message: "invalid numeric parameter value (NaN or infinity)".to_string(),
        metadata: None,
    })
}

/// Collects non-empty history system directives that should be preserved when a
/// provider accepts a separate top-level system prompt but cannot reliably
/// consume follow-up `system` chat messages.
pub(crate) fn collect_history_system_directives(request: &LLMRequest) -> Vec<String> {
    request
        .messages
        .iter()
        .filter(|message| message.role == MessageRole::System)
        .map(|message| message.content.as_text().trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// Merges a base system prompt with history directives using a simple bulleted
/// section. Providers with custom cache shaping can reuse the collected
/// directives and apply their own section placement.
pub(crate) fn merge_system_prompt_with_history_directives(
    base_prompt: Option<&str>,
    directives: &[String],
    section_header: &str,
) -> Option<String> {
    let mut system_prompt = base_prompt
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(str::to_owned)
        .unwrap_or_default();

    if directives.is_empty() {
        return (!system_prompt.is_empty()).then_some(system_prompt);
    }

    if !system_prompt.is_empty() {
        system_prompt.push('\n');
    }
    system_prompt.push_str(section_header);
    system_prompt.push('\n');
    for directive in directives {
        system_prompt.push_str("- ");
        system_prompt.push_str(directive);
        system_prompt.push('\n');
    }

    Some(system_prompt)
}

pub(crate) fn resolve_model(model: Option<String>, default_model: &str) -> String {
    model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_model.to_owned())
}

/// Ensures the request has a non-empty model, falling back to the provider's default.
/// Mutates the request in place and returns the resolved model string.
pub(crate) fn ensure_model(request: &mut LLMRequest, default_model: &str) -> String {
    if request.model.trim().is_empty() {
        request.model = default_model.to_owned();
    }
    request.model.clone()
}

/// Validates a request against a static list of supported model strings.
/// Convenience wrapper around `validate_request_common` that converts
/// `&[&str]` to `Vec<String>` internally.
pub(crate) fn validate_supported_models(
    request: &LLMRequest,
    provider_name: &str,
    provider_key: &str,
    supported_models: &[&str],
) -> Result<(), LLMError> {
    let models: Vec<String> = supported_models.iter().map(|m| m.to_string()).collect();
    validate_request_common(request, provider_name, provider_key, Some(&models))
}

/// Creates a default LLM request with a single user message.
/// Used by all providers for their LLMClient implementation.
#[inline]
pub(crate) fn make_default_request(prompt: &str, model: &str) -> LLMRequest {
    LLMRequest {
        messages: std::sync::Arc::new(vec![Message::user(prompt.to_owned())]),
        model: model.to_owned(),
        ..Default::default()
    }
}

/// Parses a client prompt that may be a JSON chat request or plain text.
/// Returns a parsed LLMRequest from JSON if valid, or a default request with the prompt.
#[inline]
pub(crate) fn parse_client_prompt_common<F>(prompt: &str, model: &str, parse_json: F) -> LLMRequest
where
    F: FnOnce(&Value) -> Option<LLMRequest>,
{
    let trimmed = prompt.trim_start();
    if trimmed.starts_with('{')
        && let Ok(value) = serde_json::from_str::<Value>(trimmed)
        && let Some(request) = parse_json(&value)
    {
        return request;
    }
    make_default_request(prompt, model)
}

/// Validates an LLM request with common checks.
/// Checks for empty messages and validates each message for the given provider.
pub(crate) fn validate_request_common(
    request: &LLMRequest,
    provider_name: &str,
    validation_provider: &str,
    supported_models: Option<&[String]>,
) -> Result<(), LLMError> {
    if request.messages.is_empty() {
        let formatted = error_display::format_llm_error(provider_name, "Messages cannot be empty");
        return Err(LLMError::InvalidRequest { message: formatted, metadata: None });
    }

    // Check for deprecated OpenAI models even when no supported-model allowlist
    // is supplied (the native OpenAI API passes `None` to allow arbitrary future
    // IDs). Known deprecated IDs are rejected with an actionable replacement.
    if !request.model.trim().is_empty()
        && validation_provider.eq_ignore_ascii_case("openai")
        && let Some((replacement, reason)) =
            vtcode_config::constants::models::openai::deprecated_model_replacement(&request.model)
    {
        let msg = format!(
            "Unsupported model: {}. {}. Update your config to use `{}` or run /model to pick a current model.",
            request.model, reason, replacement
        );
        let formatted = error_display::format_llm_error(provider_name, &msg);
        return Err(LLMError::InvalidRequest { message: formatted, metadata: None });
    }

    if let Some(models) = supported_models
        && !request.model.trim().is_empty()
        && !models.contains(&request.model)
    {
        let msg = build_unsupported_model_error(validation_provider, &request.model);
        let formatted = error_display::format_llm_error(provider_name, &msg);
        return Err(LLMError::InvalidRequest { message: formatted, metadata: None });
    }

    for message in request.messages.iter() {
        if let Err(err) = message.validate_for_provider(validation_provider) {
            let formatted = error_display::format_llm_error(provider_name, &err);
            return Err(LLMError::InvalidRequest { message: formatted, metadata: None });
        }
    }

    Ok(())
}

/// Builds an actionable error message for an unsupported model, including
/// deprecation migration guidance when the model is a known deprecated OpenAI ID.
/// The `validation_provider` scopes the deprecation lookup to avoid suggesting
/// OpenAI-specific replacements for non-OpenAI providers that happen to share an ID.
fn build_unsupported_model_error(validation_provider: &str, model: &str) -> String {
    if validation_provider.eq_ignore_ascii_case("openai")
        && let Some((replacement, reason)) =
            vtcode_config::constants::models::openai::deprecated_model_replacement(model)
    {
        return format!(
            "Unsupported model: {model}. {reason}. Update your config to use `{replacement}` or run /model to pick a current model."
        );
    }
    format!("Unsupported model: {model}")
}
