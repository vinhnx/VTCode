#![allow(clippy::result_large_err)]
use crate::config::core::{PromptCachingConfig, ProviderPromptCachingConfig};
use crate::llm::error_display;
use crate::llm::provider::{
    ContentPart, FinishReason, LLMError, LLMRequest, Message, MessageContent, MessageRole,
    ToolCall, ToolDefinition,
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

            let mut has_non_text = false;
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
                        has_non_text = true;
                        serialized_parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", mime_type, data)
                            }
                        }));
                    }
                    ContentPart::File {
                        filename,
                        file_id,
                        file_data,
                        file_url,
                        ..
                    } => {
                        if file_id.is_some() || file_data.is_some() {
                            has_non_text = true;
                            let mut file_payload = serde_json::Map::new();
                            if let Some(id) = file_id {
                                file_payload
                                    .insert("file_id".to_owned(), Value::String(id.clone()));
                            }
                            if let Some(name) = filename {
                                file_payload
                                    .insert("filename".to_owned(), Value::String(name.clone()));
                            }
                            if let Some(data) = file_data {
                                file_payload
                                    .insert("file_data".to_owned(), Value::String(data.clone()));
                            }
                            serialized_parts.push(json!({
                                "type": "file",
                                "file": Value::Object(file_payload)
                            }));
                        } else if let Some(url) = file_url {
                            // Chat Completions does not accept file_url; preserve URL as text fallback.
                            text_only.push_str(url);
                            serialized_parts.push(json!({
                                "type": "text",
                                "text": url
                            }));
                        }
                    }
                }
            }

            if has_non_text {
                Value::Array(serialized_parts)
            } else {
                Value::String(text_only)
            }
        }
    }
}

/// Serialize message content for OpenAI-compatible payloads and normalize tool
/// response content to plain text where required.
#[inline]
pub fn serialize_message_content_openai_for_role(
    role: &MessageRole,
    content: &MessageContent,
) -> Value {
    let serialized = serialize_message_content_openai(content);
    if role == &MessageRole::Tool && !serialized.is_string() {
        Value::String(content.as_text().into_owned())
    } else {
        serialized
    }
}

/// Serialize message content for OpenAI-compatible payloads while preserving
/// interleaved thinking history for supported assistant models.
pub fn serialize_message_content_openai_for_model(message: &Message, model: &str) -> Value {
    if let Some(interleaved_content) = assistant_interleaved_history_text(message, model) {
        Value::String(interleaved_content)
    } else {
        serialize_message_content_openai_for_role(&message.role, &message.content)
    }
}

/// Returns true when the model identifier points to MiniMax M2 family models.
/// Works across direct model ids and provider-qualified ids.
#[inline]
pub fn is_minimax_m2_model(model: &str) -> bool {
    model.to_ascii_lowercase().contains("minimax-m2")
}

#[inline]
fn is_glm_interleaved_thinking_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("glm-5")
        || lower.contains("glm45")
        || lower.contains("glm-4.5")
        || lower.contains("nemotron")
}

/// Returns true when the model family relies on interleaved `<think>...</think>`
/// history to maintain reasoning quality across turns.
#[inline]
pub fn is_interleaved_thinking_model(model: &str) -> bool {
    is_minimax_m2_model(model) || is_glm_interleaved_thinking_model(model)
}

#[inline]
fn text_contains_interleaved_reasoning_markup(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("<think")
        || lower.contains("<thinking")
        || lower.contains("<reasoning")
        || lower.contains("<analysis")
        || lower.contains("<thought")
}

fn message_content_is_text_only(content: &MessageContent) -> bool {
    match content {
        MessageContent::Text(_) => true,
        MessageContent::Parts(parts) => parts
            .iter()
            .all(|part| matches!(part, ContentPart::Text { .. })),
    }
}

fn preserved_interleaved_content_from_details(details: &[Value]) -> Option<String> {
    details.iter().find_map(|detail| match detail {
        Value::String(text)
            if !text.trim().is_empty() && text_contains_interleaved_reasoning_markup(text) =>
        {
            Some(text.clone())
        }
        _ => None,
    })
}

/// Rehydrates assistant history into the tagged form expected by interleaved
/// thinking models.
pub fn assistant_interleaved_history_text(message: &Message, model: &str) -> Option<String> {
    if message.role != MessageRole::Assistant
        || !is_interleaved_thinking_model(model)
        || !message_content_is_text_only(&message.content)
    {
        return None;
    }

    if let Some(details) = message.reasoning_details.as_deref()
        && let Some(raw_content) = preserved_interleaved_content_from_details(details)
    {
        return Some(raw_content);
    }

    let content = message.content.as_text();
    if text_contains_interleaved_reasoning_markup(content.as_ref()) {
        return Some(content.into_owned());
    }

    let reasoning = message
        .reasoning
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            message
                .reasoning_details
                .as_deref()
                .and_then(extract_reasoning_text_from_detail_values)
        })?;

    let mut combined = String::with_capacity(reasoning.len() + content.len() + 16);
    combined.push_str("<think>");
    combined.push_str(reasoning.trim());
    combined.push_str("</think>");
    combined.push_str(content.as_ref());
    Some(combined)
}

/// Stores the exact interleaved assistant content alongside normalized
/// reasoning so later turns can replay the original tagged history.
pub fn preserve_interleaved_content_in_reasoning_details(
    reasoning_details: &mut Option<Vec<String>>,
    raw_content: &str,
) {
    if raw_content.trim().is_empty() || !text_contains_interleaved_reasoning_markup(raw_content) {
        return;
    }

    match reasoning_details {
        Some(existing) => {
            if !existing.iter().any(|detail| detail == raw_content) {
                existing.push(raw_content.to_string());
            }
        }
        None => {
            *reasoning_details = Some(vec![raw_content.to_string()]);
        }
    }
}

/// Normalizes a reasoning detail into an object payload.
/// Accepts native objects or stringified JSON objects, and rejects everything else.
pub fn normalize_reasoning_detail_object(detail: &Value) -> Option<Value> {
    match detail {
        Value::Object(_) => Some(detail.clone()),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }

            if (trimmed.starts_with('{') || trimmed.starts_with('['))
                && let Ok(parsed) = serde_json::from_str::<Value>(trimmed)
                && parsed.is_object()
            {
                return Some(parsed);
            }

            None
        }
        _ => None,
    }
}

#[inline]
pub fn normalize_reasoning_detail_objects(details: &[Value]) -> Vec<Value> {
    details
        .iter()
        .filter_map(normalize_reasoning_detail_object)
        .collect()
}

#[inline]
pub fn append_normalized_reasoning_detail_items(input: &mut Vec<Value>, details: &[Value]) {
    for item in details {
        if let Some(normalized) = normalize_reasoning_detail_object(item) {
            input.push(normalized);
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

/// Remove generation-only controls from a payload before exact prompt-token counting.
/// Token-count endpoints generally require only prompt-side fields.
pub fn strip_generation_controls_for_token_count(payload: &mut Value) {
    let Some(root) = payload.as_object_mut() else {
        return;
    };

    for key in [
        "stream",
        "temperature",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
        "stop",
        "max_tokens",
        "max_output_tokens",
        "n",
        "seed",
        "tool_choice",
        "parallel_tool_config",
        "response_format",
        "reasoning_effort",
        "metadata",
        "prompt_cache_key",
    ] {
        root.remove(key);
    }
}

fn parse_u32_value(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| {
            value
                .as_i64()
                .and_then(|n| u64::try_from(n).ok())
                .and_then(|n| u32::try_from(n).ok())
        })
        .or_else(|| value.as_str().and_then(|s| s.parse::<u32>().ok()))
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    Some(cursor)
}

/// Parse prompt/input token counts from common response shapes.
pub fn parse_prompt_tokens_from_count_response(value: &Value) -> Option<u32> {
    const CANDIDATE_PATHS: &[&[&str]] = &[
        &["prompt_tokens"],
        &["input_tokens"],
        &["token_count"],
        &["usage", "prompt_tokens"],
        &["usage", "input_tokens"],
        &["data", "prompt_tokens"],
        &["data", "input_tokens"],
        &["data", "token_count"],
        &["usage", "total_tokens"],
        &["data", "total_tokens"],
        &["total_tokens"],
    ];

    for path in CANDIDATE_PATHS {
        if let Some(parsed) = value_at_path(value, path).and_then(parse_u32_value) {
            return Some(parsed);
        }
    }
    None
}

/// Execute an exact token-count request when provider endpoint is available.
/// Returns `Ok(None)` when endpoint appears unsupported.
pub async fn execute_token_count_request(
    request_builder: reqwest::RequestBuilder,
    payload: &Value,
    provider_name: &str,
) -> Result<Option<Value>, LLMError> {
    let response = request_builder.json(payload).send().await.map_err(|e| {
        let message = error_display::format_llm_error(
            provider_name,
            &format!("Token-count network error: {}", e),
        );
        LLMError::Network {
            message,
            metadata: None,
        }
    })?;

    let status = response.status();
    if matches!(
        status,
        reqwest::StatusCode::BAD_REQUEST
            | reqwest::StatusCode::UNPROCESSABLE_ENTITY
            | reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::NOT_IMPLEMENTED
    ) {
        return Ok(None);
    }

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let message = error_display::format_llm_error(
            provider_name,
            &format!("Token-count request failed ({}): {}", status, body),
        );
        return Err(LLMError::Provider {
            message,
            metadata: None,
        });
    }

    let value = response.json::<Value>().await.map_err(|e| {
        let message = error_display::format_llm_error(
            provider_name,
            &format!("Failed to parse token-count response: {}", e),
        );
        LLMError::Provider {
            message,
            metadata: None,
        }
    })?;

    Ok(Some(value))
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

        let content_value = serialize_message_content_openai_for_model(message, &request.model);
        message_map.insert(KEY_CONTENT.to_owned(), content_value);

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

        if message.role == MessageRole::Tool {
            match &message.tool_call_id {
                Some(tool_call_id) => {
                    message_map.insert(
                        KEY_TOOL_CALL_ID.to_owned(),
                        Value::String(tool_call_id.clone()),
                    );
                }
                None => {
                    return Err(LLMError::InvalidRequest {
                        message: format!(
                            "Tool response message missing required tool_call_id (provider: {})",
                            provider_key
                        ),
                        metadata: None,
                    });
                }
            }
        } else if let Some(tool_call_id) = &message.tool_call_id {
            message_map.insert(
                KEY_TOOL_CALL_ID.to_owned(),
                Value::String(tool_call_id.clone()),
            );
        }

        if provider_key == "zai"
            && message.role == MessageRole::Assistant
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

#[inline]
pub fn serialize_reasoning_detail_values(details: &[Value]) -> Option<Vec<String>> {
    let normalized = details
        .iter()
        .filter_map(|item| match item {
            Value::Null => None,
            Value::String(text) => {
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text.clone())
                }
            }
            _ => Some(item.to_string()),
        })
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub fn serialize_reasoning_details_field(details: &Value) -> Option<Vec<String>> {
    match details {
        Value::Array(items) => serialize_reasoning_detail_values(items),
        Value::Object(_) => Some(vec![details.to_string()]),
        Value::String(text) => {
            if text.trim().is_empty() {
                None
            } else {
                Some(vec![text.clone()])
            }
        }
        _ => None,
    }
}

fn reasoning_text_from_detail_value(detail: &Value) -> Option<String> {
    let normalized = match detail {
        Value::Object(_) => detail.clone(),
        Value::String(raw) => {
            let trimmed = raw.trim();
            if (trimmed.starts_with('{') || trimmed.starts_with('['))
                && let Ok(parsed) = serde_json::from_str::<Value>(trimmed)
            {
                parsed
            } else {
                return None;
            }
        }
        _ => return None,
    };

    crate::llm::providers::extract_reasoning_trace(&normalized).and_then(|trace| {
        let cleaned = crate::llm::providers::clean_reasoning_text(trace.trim());
        if cleaned.is_empty() {
            None
        } else {
            Some(cleaned)
        }
    })
}

pub fn extract_reasoning_text_from_detail_values(details: &[Value]) -> Option<String> {
    let mut fragments = Vec::new();
    for detail in details {
        let Some(text) = reasoning_text_from_detail_value(detail) else {
            continue;
        };
        if fragments
            .last()
            .is_none_or(|existing: &String| existing != &text)
        {
            fragments.push(text);
        }
    }

    if fragments.is_empty() {
        None
    } else {
        Some(fragments.join("\n\n"))
    }
}

pub fn extract_reasoning_text_from_serialized_details(details: &[String]) -> Option<String> {
    let mut fragments = Vec::new();
    for detail in details {
        let Ok(parsed) = serde_json::from_str::<Value>(detail) else {
            continue;
        };
        let Some(text) = reasoning_text_from_detail_value(&parsed) else {
            continue;
        };
        if fragments
            .last()
            .is_none_or(|existing: &String| existing != &text)
        {
            fragments.push(text);
        }
    }

    if fragments.is_empty() {
        None
    } else {
        Some(fragments.join("\n\n"))
    }
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

    let native_reasoning_details_json = message.get("reasoning_details");

    // Extract reasoning using custom extractor if provided
    let (mut reasoning, mut reasoning_details) = if let Some(extractor) = extract_reasoning {
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

        let reasoning_details =
            native_reasoning_details_json.and_then(serialize_reasoning_details_field);

        (reasoning, reasoning_details)
    };

    if reasoning.is_none()
        && let Some(details) = native_reasoning_details_json.and_then(|value| value.as_array())
    {
        reasoning = extract_reasoning_text_from_detail_values(details);
    }

    // Fallback: If no reasoning was found natively, try extracting from content
    if reasoning.is_none()
        && let Some(content_str) = &content
        && !content_str.is_empty()
    {
        let (extracted_reasoning, cleaned_content) = extract_reasoning_content(content_str);
        if !extracted_reasoning.is_empty() {
            reasoning = Some(extracted_reasoning.join("\n\n"));
            preserve_interleaved_content_in_reasoning_details(&mut reasoning_details, content_str);
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
pub fn make_anthropic_thinking_config(config: &crate::config::core::AnthropicConfig) -> Value {
    serde_json::json!({
        "thinking": {
            "type": config.interleaved_thinking_type_enabled,
            "budget_tokens": config.interleaved_thinking_budget_tokens
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        assistant_interleaved_history_text, extract_reasoning_text_from_detail_values,
        extract_reasoning_text_from_serialized_details, is_interleaved_thinking_model,
        is_minimax_m2_model, normalize_reasoning_detail_object, parse_response_openai_format,
    };
    use crate::llm::provider::Message;
    use serde_json::{Value, json};

    #[test]
    fn minimax_m2_model_detection_handles_variants() {
        assert!(is_minimax_m2_model("MiniMax-M2.5"));
        assert!(is_minimax_m2_model("minimax/minimax-m2.5"));
        assert!(is_minimax_m2_model("MiniMaxAI/MiniMax-M2.5:novita"));
        assert!(!is_minimax_m2_model("gpt-5"));
    }

    #[test]
    fn interleaved_thinking_model_detection_handles_glm5() {
        assert!(is_interleaved_thinking_model("glm-5"));
        assert!(is_interleaved_thinking_model("zai-org/GLM-5:novita"));
        assert!(is_interleaved_thinking_model("MiniMax-M2.5"));
        assert!(!is_interleaved_thinking_model("deepseek-r1"));
    }

    #[test]
    fn normalize_reasoning_detail_object_decodes_stringified_json_object() {
        let normalized = normalize_reasoning_detail_object(&json!(
            r#"{"type":"reasoning.text","id":"r1","text":"trace"}"#
        ))
        .expect("normalized object");
        assert!(normalized.is_object());
        assert_eq!(normalized["type"], "reasoning.text");
    }

    #[test]
    fn normalize_reasoning_detail_object_rejects_plain_text() {
        assert!(normalize_reasoning_detail_object(&json!("plain-text")).is_none());
    }

    #[test]
    fn assistant_interleaved_history_prefers_preserved_raw_detail() {
        let message = Message::assistant("answer".to_string())
            .with_reasoning_details(Some(vec![json!("<think>raw trace</think>answer")]));

        assert_eq!(
            assistant_interleaved_history_text(&message, "glm-5").as_deref(),
            Some("<think>raw trace</think>answer")
        );
    }

    #[test]
    fn assistant_interleaved_history_wraps_reasoning_when_needed() {
        let message =
            Message::assistant("answer".to_string()).with_reasoning(Some("trace".to_string()));

        assert_eq!(
            assistant_interleaved_history_text(&message, "MiniMax-M2.5").as_deref(),
            Some("<think>trace</think>answer")
        );
    }

    #[test]
    fn parse_openai_response_preserves_array_reasoning_details() {
        let response_json = json!({
            "choices": [{
                "message": {
                    "content": "done",
                    "reasoning_details": [{
                        "type": "reasoning.text",
                        "text": "step one"
                    }]
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        });

        let parsed = parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            response_json,
            "test",
            "test-model".to_string(),
            false,
            None,
        )
        .expect("response should parse");

        assert_eq!(parsed.reasoning.as_deref(), Some("step one"));
        assert!(parsed.reasoning_details.is_some());
        let first_detail = parsed
            .reasoning_details
            .as_ref()
            .and_then(|details| details.first())
            .expect("reasoning detail should exist");
        let parsed_detail: Value =
            serde_json::from_str(first_detail).expect("reasoning detail should be json");
        assert_eq!(parsed_detail["type"], "reasoning.text");
    }

    #[test]
    fn parse_openai_response_preserves_raw_interleaved_content_in_reasoning_details() {
        let response_json = json!({
            "choices": [{
                "message": {
                    "content": "<think>step one</think>done"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        });

        let parsed = parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
            response_json,
            "test",
            "glm-5".to_string(),
            false,
            None,
        )
        .expect("response should parse");

        assert_eq!(parsed.content.as_deref(), Some("done"));
        assert_eq!(parsed.reasoning.as_deref(), Some("step one"));
        assert_eq!(
            parsed
                .reasoning_details
                .as_ref()
                .and_then(|details| details.first())
                .map(String::as_str),
            Some("<think>step one</think>done")
        );
    }

    #[test]
    fn extract_reasoning_text_from_detail_values_handles_stringified_json() {
        let details = vec![json!(r#"{"type":"reasoning.text","text":"trace one"}"#)];
        assert_eq!(
            extract_reasoning_text_from_detail_values(&details).as_deref(),
            Some("trace one")
        );
    }

    #[test]
    fn extract_reasoning_text_from_serialized_details_handles_json_items() {
        let details = vec![
            json!({"type":"reasoning.text","text":"first"}).to_string(),
            json!({"type":"reasoning.text","text":"second"}).to_string(),
        ];
        assert_eq!(
            extract_reasoning_text_from_serialized_details(&details).as_deref(),
            Some("first\n\nsecond")
        );
    }
}
