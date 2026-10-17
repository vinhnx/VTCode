//! OpenAI-compatible chat wire serialization and response parsing.

use super::reasoning::{message_content_is_text_only, serialize_reasoning_details_field};
use super::{
    assistant_interleaved_history_text, extract_reasoning_text_from_detail_values,
    preserve_interleaved_content_in_reasoning_details,
};
use crate::error_display;
use crate::provider::{
    ContentPart, FinishReason, LLMError, LLMRequest, Message, MessageContent, MessageRole, ToolCall, ToolDefinition,
};
use crate::providers::openai::tool_serialization::sanitize_openai_function_parameters;
use crate::types as llm_types;
use crate::utils::extract_reasoning_content;
use serde_json::{Value, json};

/// Serializes tool definitions to OpenAI-compatible JSON format.
/// Used by DeepSeek, ZAI, Moonshot, and other OpenAI-compatible providers.
/// For OpenAI-specific features (GPT-5.1 native tools), use OpenAIProvider's serialize_tools.
///
/// This function normalizes all tool types to "function" type for compatibility with
/// OpenAI-compatible APIs that don't support special tool types like "apply_patch".
#[inline]
pub(crate) fn serialize_tools_openai_format(tools: &[ToolDefinition]) -> Option<Vec<Value>> {
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
                        tool.web_search.clone().unwrap_or_else(|| json!({"enable": true})),
                    );
                    return Some(Value::Object(payload));
                }

                // For OpenAI-compatible APIs, normalize all tools to function type
                // Special types like "apply_patch", "shell", "custom" are GPT-5.x specific
                tool.function.as_ref().map(|func| {
                    let parameters = sanitize_openai_function_parameters(func.parameters.clone(), true);
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": func.name,
                            "description": func.description,
                            "parameters": parameters
                        }
                    })
                })
            })
            .collect(),
    )
}

/// Build a `data:` URL for base64 images (DRY helper shared across providers).
#[inline]
fn data_url(mime_type: &str, data: &str) -> String {
    let mut s = String::with_capacity(13 + mime_type.len() + data.len());
    s.push_str("data:");
    s.push_str(mime_type);
    s.push_str(";base64,");
    s.push_str(data);
    s
}

/// Serialize a single `Image` part to OpenAI `image_url` wire format.
///
/// Uses `ImageSource` dispatch (KISS/DRY guard rail) and strongly typed `ImageDetail`.
#[inline]
fn serialize_image_part(
    data: &str,
    mime_type: &str,
    detail: &Option<crate::provider::ImageDetail>,
    image_url: &Option<String>,
) -> Value {
    let url = if let Some(ext) = image_url {
        ext.clone()
    } else {
        data_url(mime_type, data)
    };
    let mut image_url_obj = serde_json::Map::new();
    image_url_obj.insert("url".to_owned(), Value::String(url));
    if let Some(d) = detail {
        image_url_obj.insert("detail".to_owned(), Value::String(d.as_str().to_owned()));
    }
    json!({
        "type": "image_url",
        "image_url": Value::Object(image_url_obj)
    })
}

/// Serialize message content for OpenAI-compatible chat payloads.
/// Falls back to a string when there are no image parts.
///
/// This is a thin orchestration over `serialize_image_part` and file handling.
/// Provider-specific quirks (e.g., DeepSeek flat `file_id`) are handled in
/// `DeepSeekSpec::finish_payload`, not here, keeping `common.rs` provider-agnostic.
pub(crate) fn serialize_message_content_openai(content: &MessageContent) -> Value {
    match content {
        MessageContent::Text(text) => Value::String(text.clone()),
        MessageContent::Parts(parts) => {
            if parts.is_empty() {
                return Value::String(String::new());
            }

            // Filter first: most `Parts` payloads are text-only, so avoid
            // building a `Vec<Value>` of JSON part objects only to discard it
            // and keep the concatenated string.
            if message_content_is_text_only(content) {
                let mut text_only = String::new();
                for part in parts {
                    if let ContentPart::Text { text } = part {
                        text_only.push_str(text);
                    }
                }
                return Value::String(text_only);
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
                    ContentPart::Image { data, mime_type, detail, image_url, .. } => {
                        has_non_text = true;
                        serialized_parts.push(serialize_image_part(data, mime_type, detail, image_url));
                    }
                    ContentPart::File { filename, file_id, file_data, file_url, .. } => {
                        if file_id.is_some() || file_data.is_some() {
                            has_non_text = true;
                            // Chat Completions: keep legacy nested shape; DeepSeek flat
                            // transform is applied in `DeepSeekSpec::finish_payload` if needed.
                            let mut file_payload = serde_json::Map::new();
                            if let Some(id) = file_id {
                                file_payload.insert("file_id".to_owned(), Value::String(id.clone()));
                            }
                            if let Some(name) = filename {
                                file_payload.insert("filename".to_owned(), Value::String(name.clone()));
                            }
                            if let Some(data) = file_data {
                                file_payload.insert("file_data".to_owned(), Value::String(data.clone()));
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
pub(crate) fn serialize_message_content_openai_for_role(role: &MessageRole, content: &MessageContent) -> Value {
    let serialized = serialize_message_content_openai(content);
    if role == &MessageRole::Tool && !serialized.is_string() {
        Value::String(content.as_text().into_owned())
    } else {
        serialized
    }
}

/// Serialize message content for OpenAI-compatible payloads while preserving
/// interleaved thinking history for supported assistant models.
pub(crate) fn serialize_message_content_openai_for_model(message: &Message, model: &str) -> Value {
    if let Some(interleaved_content) = assistant_interleaved_history_text(message, model) {
        Value::String(interleaved_content)
    } else {
        serialize_message_content_openai_for_role(&message.role, &message.content)
    }
}

/// Converts provider Usage to llm_types::Usage.
/// Shared by all LLMClient implementations.
#[inline]
pub(crate) fn convert_usage_to_llm_types(usage: crate::provider::Usage) -> llm_types::Usage {
    usage
}

/// Parses a tool call from OpenAI-compatible JSON format.
/// Works for DeepSeek, ZAI, and other OpenAI-compatible providers.
#[inline]
fn parse_tool_call_openai_format(value: &Value) -> Option<ToolCall> {
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

    Some(ToolCall::function(id.to_string(), name.to_string(), arguments.unwrap_or_else(|| "{}".to_string())))
}

/// Maps common finish reason strings to FinishReason enum.
/// Handles standard OpenAI-compatible finish reasons.
#[inline]
pub(crate) fn map_finish_reason_common(reason: &str) -> FinishReason {
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
pub(crate) fn serialize_messages_openai_format(
    request: &LLMRequest,
    provider_key: &str,
) -> Result<Vec<Value>, LLMError> {
    use serde_json::{Map, json};

    let mut messages = Vec::with_capacity(request.messages.len());

    for message in request.messages.iter() {
        message
            .validate_for_provider(provider_key)
            .map_err(|e| LLMError::InvalidRequest { message: e, metadata: None })?;

        let mut message_map = Map::with_capacity(4); // Pre-allocate for role, content, tool_calls, tool_call_id
        message_map.insert(KEY_ROLE.to_owned(), Value::String(message.role.as_generic_str().to_owned()));

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
                    message_map.insert(KEY_TOOL_CALL_ID.to_owned(), Value::String(tool_call_id.clone()));
                }
                None => {
                    return Err(LLMError::InvalidRequest {
                        message: format!(
                            "Tool response message missing required tool_call_id (provider: {provider_key})"
                        ),
                        metadata: None,
                    });
                }
            }
        } else if let Some(tool_call_id) = &message.tool_call_id {
            message_map.insert(KEY_TOOL_CALL_ID.to_owned(), Value::String(tool_call_id.clone()));
        }

        if message.role == MessageRole::Assistant
            && let Some(reasoning) = &message.reasoning
        {
            message_map.insert(KEY_REASONING_CONTENT.to_owned(), Value::String(reasoning.clone()));
        }

        messages.push(Value::Object(message_map));
    }

    Ok(messages)
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
pub(crate) fn parse_chat_request_openai_format(value: &Value, default_model: &str) -> Option<LLMRequest> {
    parse_chat_request_openai_format_with_extractor(value, default_model, |c| {
        c.as_str().map(|s| s.to_string()).unwrap_or_default()
    })
}

/// Parses chat request with custom content extraction logic.
/// Use this when provider has special content format (e.g., array of content blocks).
fn parse_chat_request_openai_format_with_extractor<F>(
    value: &Value,
    default_model: &str,
    content_extractor: F,
) -> Option<LLMRequest>
where
    F: Fn(&Value) -> String,
{
    use crate::provider::{AssistantPhase, Message};

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
            .unwrap_or(vtcode_config::constants::message_roles::USER);
        let content = entry.get("content").map(&content_extractor).unwrap_or_default();
        let assistant_phase = entry
            .get("phase")
            .and_then(Value::as_str)
            .and_then(AssistantPhase::from_wire_str);

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
                    .map(|calls| calls.iter().filter_map(parse_tool_call_openai_format).collect::<Vec<_>>())
                    .filter(|calls| !calls.is_empty());

                if let Some(calls) = tool_calls {
                    messages.push(Message::assistant_with_tools(content, calls).with_phase(assistant_phase));
                } else {
                    messages.push(Message::assistant(content).with_phase(assistant_phase));
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
        messages: std::sync::Arc::new(messages),
        system_prompt: system_prompt.map(std::sync::Arc::from),
        model: value.get("model").and_then(|m| m.as_str()).unwrap_or(default_model).to_string(),
        max_tokens: value.get("max_tokens").and_then(|m| m.as_u64()).map(|m| m as u32),
        temperature: value.get("temperature").and_then(|t| t.as_f64()).map(|t| t as f32),
        stream: value.get("stream").and_then(|s| s.as_bool()).unwrap_or(false),
        ..Default::default()
    })
}

/// Extracts content from a message value, handling both string and array formats.
#[inline]
fn extract_content_from_message(message: &Value) -> Option<String> {
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
///
/// When cache metrics are enabled, OpenAI-style hosts may report cache reads as
/// `prompt_cache_hit_tokens` or `prompt_tokens_details.cached_tokens`. Surface
/// the same value in both `cached_prompt_tokens` and `cache_read_tokens` so
/// trajectory/cache-health telemetry is not zero-filled.
#[inline]
pub(crate) fn parse_usage_openai_format(
    response_json: &Value,
    include_cache_metrics: bool,
) -> Option<crate::provider::Usage> {
    response_json.get("usage").map(|usage_value| {
        let cached_prompt_tokens = if include_cache_metrics {
            crate::providers::shared::parse_cached_prompt_tokens_from_usage(usage_value, true).or_else(|| {
                usage_value
                    .get("prompt_cache_hit_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
            })
        } else {
            None
        };
        crate::provider::Usage {
            prompt_tokens: usage_value.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: usage_value.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            total_tokens: usage_value.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            cached_prompt_tokens,
            cache_creation_tokens: if include_cache_metrics {
                crate::providers::shared::parse_cache_write_tokens_from_usage(usage_value, true).or_else(|| {
                    usage_value
                        .get("prompt_cache_miss_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32)
                })
            } else {
                None
            },
            cache_read_tokens: cached_prompt_tokens,
            iterations: None,
        }
    })
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
pub(crate) fn parse_response_openai_format<F>(
    response_json: Value,
    provider_name: &str,
    model: String,
    include_cache_metrics: bool,
    extract_reasoning: Option<F>,
) -> Result<crate::provider::LLMResponse, LLMError>
where
    F: Fn(&Value, &Value) -> Option<String>,
{
    use crate::provider::LLMResponse;

    let choices = response_json.get("choices").and_then(|value| value.as_array()).ok_or_else(|| {
        let formatted_error =
            error_display::format_llm_error(provider_name, "Invalid response format: missing choices");
        LLMError::Provider { message: formatted_error, metadata: None }
    })?;

    if choices.is_empty() {
        let formatted_error = error_display::format_llm_error(provider_name, "No choices in response");
        return Err(LLMError::Provider { message: formatted_error, metadata: None });
    }

    let choice = &choices[0];
    let message = choice.get("message").ok_or_else(|| {
        let formatted_error =
            error_display::format_llm_error(provider_name, "Invalid response format: missing message");
        LLMError::Provider { message: formatted_error, metadata: None }
    })?;

    let mut content = extract_content_from_message(message);

    let tool_calls = message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|calls| calls.iter().filter_map(parse_tool_call_openai_format).collect::<Vec<_>>())
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

        let reasoning_details = native_reasoning_details_json.and_then(serialize_reasoning_details_field);

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
        compaction: None,
    })
}
