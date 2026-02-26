#![allow(clippy::result_large_err)]
use crate::config::core::{PromptCachingConfig, ProviderPromptCachingConfig};
use crate::llm::error_display;
use crate::llm::provider::{
    ContentPart, FinishReason, LLMError, LLMRequest, Message, MessageContent, ToolCall,
    ToolDefinition,
};
use crate::llm::types as llm_types;
use crate::llm::utils::extract_reasoning_content;
use serde_json::{Value, json};

/// Serializes tool definitions to OpenAI-compatible JSON format.
/// Used by DeepSeek, ZAI, Moonshot, and other OpenAI-compatible providers.
/// For OpenAI-specific features (GPT-5.1 native tools), use OpenAIProvider's serialize_tools.
///
/// This function normalizes all tool types to "function" type for compatibility with
/// OpenAI-compatible APIs that don't support special tool types like "apply_patch".
#[inline]
pub fn serialize_tools_openai_format(tools: &[ToolDefinition]) -> Option<Vec<Value>> {
    if tools.is_empty() {
        return None;
    }
    Some(
        tools
            .iter()
            .filter_map(|tool| {
                if tool.tool_type == "web_search" {
                    let mut payload = serde_json::Map::new();
                    payload.insert("type".to_owned(), Value::String("web_search".to_owned()));
                    payload.insert(
                        "web_search".to_owned(),
                        tool.web_search
                            .clone()
                            .unwrap_or_else(|| json!({"enable": true})),
                    );
                    return Some(Value::Object(payload));
                }

                // For OpenAI-compatible APIs, normalize all tools to function type
                // Special types like "apply_patch", "shell", "custom" are GPT-5.x specific
                tool.function.as_ref().map(|func| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": func.name,
                            "description": func.description,
                            "parameters": func.parameters
                        }
                    })
                })
            })
            .collect(),
    )
}

/// Serialize message content for OpenAI-compatible chat payloads.
/// Falls back to a string when there are no image parts.
pub fn serialize_message_content_openai(content: &MessageContent) -> Value {
    match content {
        MessageContent::Text(text) => Value::String(text.clone()),
        MessageContent::Parts(parts) => {
            if parts.is_empty() {
                return Value::String(String::new());
            }

            let mut has_image = false;
            let mut serialized_parts = Vec::with_capacity(parts.len());
            let mut text_only = String::new();

            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        text_only.push_str(text);
                        serialized_parts.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                    ContentPart::Image {
                        data, mime_type, ..
                    } => {
                        has_image = true;
                        serialized_parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", mime_type, data)
                            }
                        }));
                    }
                }
            }

            if has_image {
                Value::Array(serialized_parts)
            } else {
                Value::String(text_only)
            }
        }
    }
}

pub fn resolve_model(model: Option<String>, default_model: &str) -> String {
    model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_model.to_owned())
}

/// Creates a default LLM request with a single user message.
/// Used by all providers for their LLMClient implementation.
#[inline]
pub fn make_default_request(prompt: &str, model: &str) -> LLMRequest {
    LLMRequest {
        messages: vec![Message::user(prompt.to_owned())],
        model: model.to_owned(),
        ..Default::default()
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
    if trimmed.starts_with('{')
        && let Ok(value) = serde_json::from_str::<Value>(trimmed)
        && let Some(request) = parse_json(&value)
    {
        return request;
    }
    make_default_request(prompt, model)
}

/// Converts provider Usage to llm_types::Usage.
/// Shared by all LLMClient implementations.
#[inline]
pub fn convert_usage_to_llm_types(usage: crate::llm::provider::Usage) -> llm_types::Usage {
    usage
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

    if let Some(var_name) = env_var_name
        && let Ok(value) = std::env::var(var_name)
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    default_base_url.to_string()
}

/// Get or create HTTP client with custom timeouts
pub fn get_http_client_for_timeouts(
    connect_timeout: std::time::Duration,
    read_timeout: std::time::Duration,
) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(read_timeout)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub fn extract_prompt_cache_settings_default(
    prompt_cache: Option<PromptCachingConfig>,
    _provider_key: &str,
) -> (bool, bool) {
    match prompt_cache {
        Some(cfg) if cfg.enabled => (true, cfg.enabled),
        _ => (false, false),
    }
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
        "refusal" => FinishReason::Refusal,
        other => FinishReason::Error(other.to_string()),
    }
}

// Pre-allocated keys to avoid repeated allocations
const KEY_ROLE: &str = "role";
const KEY_CONTENT: &str = "content";
const KEY_TOOL_CALLS: &str = "tool_calls";
const KEY_TOOL_CALL_ID: &str = "tool_call_id";
const KEY_REASONING_CONTENT: &str = "reasoning_content";

/// Serializes messages to OpenAI-compatible JSON format.
/// Used by DeepSeek, Moonshot, and other OpenAI-compatible providers.
pub fn serialize_messages_openai_format(
    request: &LLMRequest,
    provider_key: &str,
) -> Result<Vec<Value>, LLMError> {
    use serde_json::{Map, json};

    let mut messages = Vec::with_capacity(request.messages.len());

    for message in &request.messages {
        message
            .validate_for_provider(provider_key)
            .map_err(|e| LLMError::InvalidRequest {
                message: e,
                metadata: None,
            })?;

        let mut message_map = Map::with_capacity(4); // Pre-allocate for role, content, tool_calls, tool_call_id
        message_map.insert(
            KEY_ROLE.to_owned(),
            Value::String(message.role.as_generic_str().to_owned()),
        );
        message_map.insert(
            KEY_CONTENT.to_owned(),
            serialize_message_content_openai(&message.content),
        );

        if let Some(tool_calls) = &message.tool_calls {
            // Optimize: Use references to avoid cloning
            let serialized_calls = tool_calls
                .iter()
                .filter_map(|call| {
                    call.function.as_ref().map(|func| {
                        json!({
                            "id": &call.id,
                            "type": "function",
                            "function": {
                                "name": &func.name,
                                "arguments": &func.arguments
                            }
                        })
                    })
                })
                .collect::<Vec<_>>();
            message_map.insert(KEY_TOOL_CALLS.to_owned(), Value::Array(serialized_calls));
        }

        if let Some(tool_call_id) = &message.tool_call_id {
            message_map.insert(
                KEY_TOOL_CALL_ID.to_owned(),
                Value::String(tool_call_id.clone()),
            );
        }

        if provider_key == "zai"
            && message.role == crate::llm::provider::MessageRole::Assistant
            && let Some(reasoning) = &message.reasoning
        {
            message_map.insert(
                KEY_REASONING_CONTENT.to_owned(),
                Value::String(reasoning.clone()),
            );
        }

        messages.push(Value::Object(message_map));
    }

    Ok(messages)
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
        let formatted = error_display::format_llm_error(provider_name, "Messages cannot be empty");
        return Err(LLMError::InvalidRequest {
            message: formatted,
            metadata: None,
        });
    }

    if let Some(models) = supported_models
        && !request.model.trim().is_empty()
        && !models.contains(&request.model)
    {
        let msg = format!("Unsupported model: {}", request.model);
        let formatted = error_display::format_llm_error(provider_name, &msg);
        return Err(LLMError::InvalidRequest {
            message: formatted,
            metadata: None,
        });
    }

    for message in &request.messages {
        if let Err(err) = message.validate_for_provider(validation_provider) {
            let formatted = error_display::format_llm_error(provider_name, &err);
            return Err(LLMError::InvalidRequest {
                message: formatted,
                metadata: None,
            });
        }
    }

    Ok(())
}

/// Parses chat request from OpenAI-compatible JSON format.
/// Used by DeepSeek, ZAI, OpenRouter, and other OpenAI-compatible providers.
///
/// # Arguments
/// * `value` - JSON value containing the chat request
/// * `default_model` - Default model to use if not specified in request
/// * `content_extractor` - Optional function to extract content from JSON (defaults to simple string extraction)
///
/// # Returns
/// `Some(LLMRequest)` if parsing succeeds, `None` otherwise
pub fn parse_chat_request_openai_format(value: &Value, default_model: &str) -> Option<LLMRequest> {
    parse_chat_request_openai_format_with_extractor(value, default_model, |c| {
        c.as_str().map(|s| s.to_string()).unwrap_or_default()
    })
}

/// Parses chat request with custom content extraction logic.
/// Use this when provider has special content format (e.g., array of content blocks).
pub fn parse_chat_request_openai_format_with_extractor<F>(
    value: &Value,
    default_model: &str,
    content_extractor: F,
) -> Option<LLMRequest>
where
    F: Fn(&Value) -> String,
{
    use crate::llm::provider::Message;

    let messages_value = value.get("messages")?.as_array()?;
    let mut system_prompt = value
        .get("system")
        .and_then(|entry| entry.as_str())
        .map(|text| text.to_string());
    let mut messages = Vec::with_capacity(messages_value.len());

    for entry in messages_value {
        let role = entry
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or(crate::config::constants::message_roles::USER);
        let content = entry
            .get("content")
            .map(&content_extractor)
            .unwrap_or_default();

        match role {
            "system" => {
                if system_prompt.is_none() && !content.is_empty() {
                    system_prompt = Some(content);
                }
            }
            "assistant" => {
                let tool_calls = entry
                    .get("tool_calls")
                    .and_then(|tc| tc.as_array())
                    .map(|calls| {
                        calls
                            .iter()
                            .filter_map(parse_tool_call_openai_format)
                            .collect::<Vec<_>>()
                    })
                    .filter(|calls| !calls.is_empty());

                if let Some(calls) = tool_calls {
                    messages.push(Message::assistant_with_tools(content, calls));
                } else {
                    messages.push(Message::assistant(content));
                }
            }
            "tool" => {
                if let Some(tool_call_id) = entry.get("tool_call_id").and_then(|v| v.as_str()) {
                    messages.push(Message::tool_response(tool_call_id.to_string(), content));
                }
            }
            _ => {
                messages.push(Message::user(content));
            }
        }
    }

    Some(LLMRequest {
        messages,
        system_prompt: system_prompt.map(std::sync::Arc::new),
        model: value
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(default_model)
            .to_string(),
        max_tokens: value
            .get("max_tokens")
            .and_then(|m| m.as_u64())
            .map(|m| m as u32),
        temperature: value
            .get("temperature")
            .and_then(|t| t.as_f64())
            .map(|t| t as f32),
        stream: value
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false),
        ..Default::default()
    })
}

/// Extracts content from a message value, handling both string and array formats.
#[inline]
pub fn extract_content_from_message(message: &Value) -> Option<String> {
    message.get("content").and_then(|value| match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Array(parts) => {
            let mut combined = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    combined.push_str(text);
                }
            }
            let trimmed = combined.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        _ => None,
    })
}

/// Parses usage information from OpenAI-compatible response format.
#[inline]
pub fn parse_usage_openai_format(
    response_json: &Value,
    include_cache_metrics: bool,
) -> Option<crate::llm::provider::Usage> {
    response_json
        .get("usage")
        .map(|usage_value| crate::llm::provider::Usage {
            prompt_tokens: usage_value
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: usage_value
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: usage_value
                .get("total_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cached_prompt_tokens: if include_cache_metrics {
                usage_value
                    .get("prompt_cache_hit_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
            } else {
                None
            },
            cache_creation_tokens: if include_cache_metrics {
                usage_value
                    .get("prompt_cache_miss_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
            } else {
                None
            },
            cache_read_tokens: None,
        })
}

/// Remove generation-tuning fields for token-count estimation requests.
#[inline]
pub fn strip_generation_controls_for_token_count(payload: &mut Value) {
    if let Some(object) = payload.as_object_mut() {
        object.remove("stream");
        object.remove("temperature");
        object.remove("top_p");
        object.remove("top_k");
        object.remove("max_output_tokens");
        object.remove("max_tokens");
        object.remove("reasoning");
        object.remove("reasoning_effort");
    }
}

/// Parse prompt token count from common count-response shapes across providers.
#[inline]
pub fn parse_prompt_tokens_from_count_response(value: &Value) -> Option<u32> {
    value
        .get("input_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get("usage")
                .and_then(|usage| usage.get("input_tokens"))
                .and_then(Value::as_u64)
        })
        .or_else(|| value.get("prompt_tokens").and_then(Value::as_u64))
        .or_else(|| {
            value
                .get("usage")
                .and_then(|usage| usage.get("prompt_tokens"))
                .and_then(Value::as_u64)
        })
        .and_then(|raw| u32::try_from(raw).ok())
}

/// Execute a token-count request and return parsed JSON on success.
/// Returns `Ok(None)` for non-success HTTP status so callers can treat unsupported
/// endpoints as "exact counting unavailable" without raising provider errors.
pub async fn execute_token_count_request(
    builder: reqwest::RequestBuilder,
    payload: &Value,
    provider_name: &str,
) -> Result<Option<Value>, LLMError> {
    let response = builder.json(payload).send().await.map_err(|e| {
        let formatted_error = error_display::format_llm_error(
            provider_name,
            &format!("Token count network error: {}", e),
        );
        LLMError::Network {
            message: formatted_error,
            metadata: None,
        }
    })?;

    if !response.status().is_success() {
        return Ok(None);
    }

    let value = response.json().await.map_err(|e| {
        let formatted_error = error_display::format_llm_error(
            provider_name,
            &format!("Failed to parse token count response: {}", e),
        );
        LLMError::Provider {
            message: formatted_error,
            metadata: None,
        }
    })?;

    Ok(Some(value))
}

/// Parses OpenAI-compatible response format.
/// Used by DeepSeek, Moonshot, and other OpenAI-compatible providers.
///
/// # Arguments
/// * `response_json` - The JSON response from the API
/// * `provider_name` - Provider name for error messages
/// * `model` - Model name to include in the response
/// * `include_cache_metrics` - Whether to parse cache-related usage metrics
/// * `extract_reasoning` - Optional function to extract reasoning content from message/choice
///
/// # Returns
/// Parsed LLMResponse or error
pub fn parse_response_openai_format<F>(
    response_json: Value,
    provider_name: &str,
    model: String,
    include_cache_metrics: bool,
    extract_reasoning: Option<F>,
) -> Result<crate::llm::provider::LLMResponse, LLMError>
where
    F: Fn(&Value, &Value) -> Option<String>,
{
    use crate::llm::provider::LLMResponse;

    let choices = response_json
        .get("choices")
        .and_then(|value| value.as_array())
        .ok_or_else(|| {
            let formatted_error = error_display::format_llm_error(
                provider_name,
                "Invalid response format: missing choices",
            );
            LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

    if choices.is_empty() {
        let formatted_error =
            error_display::format_llm_error(provider_name, "No choices in response");
        return Err(LLMError::Provider {
            message: formatted_error,
            metadata: None,
        });
    }

    let choice = &choices[0];
    let message = choice.get("message").ok_or_else(|| {
        let formatted_error = error_display::format_llm_error(
            provider_name,
            "Invalid response format: missing message",
        );
        LLMError::Provider {
            message: formatted_error,
            metadata: None,
        }
    })?;

    let mut content = extract_content_from_message(message);

    let tool_calls = message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|calls| {
            calls
                .iter()
                .filter_map(parse_tool_call_openai_format)
                .collect::<Vec<_>>()
        })
        .filter(|calls| !calls.is_empty());

    // Extract reasoning using custom extractor if provided
    let (mut reasoning, reasoning_details) = if let Some(extractor) = extract_reasoning {
        // Extractor should return (reasoning, reasoning_details)
        // For backwards compatibility, we'll wrap it if it only returns reasoning
        // But let's assume we update the extractor signature if needed.
        // For now, let's just stick to the current signature but handle it better.
        (extractor(message, choice), None)
    } else {
        // Default: check message.reasoning_content or choice.reasoning
        let reasoning = message
            .get("reasoning_content")
            .or_else(|| message.get("reasoning"))
            .and_then(|rc| rc.as_str())
            .map(|s| s.to_string());

        let reasoning_details = message
            .get("reasoning_details")
            .and_then(|rd| rd.as_str())
            .map(|s| vec![s.to_string()]);

        (reasoning, reasoning_details)
    };

    // Fallback: If no reasoning was found natively, try extracting from content
    if reasoning.is_none()
        && let Some(content_str) = &content
        && !content_str.is_empty()
    {
        let (extracted_reasoning, cleaned_content) = extract_reasoning_content(content_str);
        if !extracted_reasoning.is_empty() {
            reasoning = Some(extracted_reasoning.join("\n\n"));
            // If the content was mostly reasoning, we update it to the cleaned version
            content = cleaned_content;
        }
    }

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|value| value.as_str())
        .map(map_finish_reason_common)
        .unwrap_or(FinishReason::Stop);

    let usage = parse_usage_openai_format(&response_json, include_cache_metrics);

    Ok(LLMResponse {
        content,
        tool_calls,
        model,
        usage,
        finish_reason,
        reasoning,
        reasoning_details,
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
    })
}

/// Generates the interleaved thinking configuration for Anthropic models.
/// This provides consistent thinking configuration across all Anthropic provider implementations.
///
/// # Arguments
/// * `config` - Anthropic configuration containing thinking settings
///
/// Returns a JSON Value containing the thinking configuration with:
/// - type: Configured value (default: "enabled")
/// - budget_tokens: Configured value (default: 12000)
#[inline]
pub fn make_anthropic_thinking_config(
    config: &crate::config::core::AnthropicConfig,
) -> serde_json::Value {
    serde_json::json!({
        "thinking": {
            "type": config.interleaved_thinking_type_enabled,
            "budget_tokens": config.interleaved_thinking_budget_tokens
        }
    })
}
