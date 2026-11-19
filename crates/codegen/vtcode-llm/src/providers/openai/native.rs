//! Bound computer-use route for OpenAI Responses.
//!
//! The ordinary parser rejects native computer items. This route declares the
//! computer tool, then decodes natives alongside ordinaries into
//! [`NativeResponse`]. History replay uses the verbatim output items, so a
//! follow-up request restates the original calls plus their outputs without
//! server-side chaining.

use serde_json::Value;

use crate::error_display;
use crate::provider::{
    FinishReason, LLMError, NativeCall, NativeCallBody, NativeProvider, NativeResponse, ToolCall, Usage,
};
use crate::providers::shared::{
    RESPONSES_COMPLETION_TOKEN_KEYS, RESPONSES_PROMPT_TOKEN_KEYS, parse_cache_write_tokens_from_usage,
    parse_cached_prompt_tokens_from_usage, usage_u32_from_keys,
};

use super::computer_use::{declaration, decode_call};
use super::responses_api::parse_responses_tool_call;

/// Append the computer tool entry to a request body built by the ordinary
/// converter, preserving any declared function tools for mixed turns.
///
/// The converter serializes a struct, so the body is always an object; the
/// guard below is defensive only.
pub(super) fn declare(body: &mut Value) {
    debug_assert!(body.is_object(), "OpenAI request body must be an object");
    let entry = declaration();
    let Some(obj) = body.as_object_mut() else {
        return;
    };
    match obj.get_mut("tools") {
        Some(Value::Array(tools)) => tools.push(entry),
        _ => {
            obj.insert("tools".to_owned(), Value::Array(vec![entry]));
        }
    }
}

/// Parse a bound-route response: text, ordinary calls, and decoded native
/// calls, with the verbatim output items for replay.
///
/// # Errors
///
/// Returns an error for a missing or empty output array, or for a malformed
/// or incomplete computer call. A malformed native batch fails the whole
/// turn: unlike Anthropic member blocks, OpenAI actions cannot be answered
/// individually.
pub(super) fn parse_response_native(response_json: Value, model: String) -> Result<NativeResponse, LLMError> {
    let output = response_json.get("output").and_then(Value::as_array).ok_or_else(|| {
        let formatted = error_display::format_llm_error("OpenAI", "Invalid Responses API format: missing output array");
        LLMError::Provider { message: formatted, metadata: None }
    })?;
    if output.is_empty() {
        let formatted = error_display::format_llm_error("OpenAI", "No output in response");
        return Err(LLMError::Provider { message: formatted, metadata: None });
    }

    let mut content_fragments = Vec::new();
    let mut reasoning_text_fragments = Vec::new();
    let mut tool_calls = Vec::new();
    let mut native_calls = Vec::new();
    for (position, item) in output.iter().enumerate() {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        match item_type {
            "message" => {
                if let Some(content_array) = item.get("content").and_then(Value::as_array) {
                    for entry in content_array {
                        match entry.get("type").and_then(Value::as_str).unwrap_or("") {
                            "text" | "output_text" => {
                                if let Some(text) = entry.get("text").and_then(Value::as_str)
                                    && !text.is_empty()
                                {
                                    content_fragments.push(text.to_string());
                                }
                            }
                            "reasoning" => {
                                if let Some(text) = entry.get("text").and_then(Value::as_str)
                                    && !text.is_empty()
                                {
                                    reasoning_text_fragments.push(text.to_string());
                                }
                            }
                            "function_call" | "tool_call" | "custom_tool_call" => {
                                if let Some(call) = parse_responses_tool_call(entry) {
                                    tool_calls.push(call);
                                }
                            }
                            "refusal" => {
                                if let Some(refusal) = entry.get("refusal").and_then(Value::as_str)
                                    && !refusal.is_empty()
                                {
                                    content_fragments.push(format!("[Refusal: {refusal}]"));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            "function_call" | "tool_call" | "custom_tool_call" => {
                if let Some(call) = parse_responses_tool_call(item) {
                    tool_calls.push(call);
                }
            }
            "computer_call" => {
                let call = decode_call(item).map_err(|error| LLMError::Provider {
                    message: format!("Invalid OpenAI computer call: {error}"),
                    metadata: None,
                })?;
                if let Some(call) = call {
                    native_calls.push(NativeCall {
                        id: call.id().to_string(),
                        provider: NativeProvider::OpenAi,
                        index: position,
                        body: NativeCallBody::OpenAi {
                            replay_item: call.replay_item().clone(),
                            actions: call.actions,
                        },
                    });
                }
            }
            "reasoning" => {
                if let Some(summary_array) = item.get("summary").and_then(Value::as_array) {
                    for summary_part in summary_array {
                        if let Some(text) = summary_part.get("text").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            reasoning_text_fragments.push(text.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(NativeResponse {
        content: if content_fragments.is_empty() {
            None
        } else {
            Some(content_fragments.join(""))
        },
        model,
        usage: response_json.get("usage").map(|usage_value| Usage {
            prompt_tokens: usage_u32_from_keys(usage_value, RESPONSES_PROMPT_TOKEN_KEYS),
            completion_tokens: usage_u32_from_keys(usage_value, RESPONSES_COMPLETION_TOKEN_KEYS),
            total_tokens: usage_u32_from_keys(usage_value, &["total_tokens"]),
            cached_prompt_tokens: parse_cached_prompt_tokens_from_usage(usage_value, true),
            cache_creation_tokens: parse_cache_write_tokens_from_usage(usage_value, true),
            cache_read_tokens: None,
            iterations: None,
        }),
        finish_reason: if tool_calls.is_empty() && native_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolCalls
        },
        reasoning: if reasoning_text_fragments.is_empty() {
            None
        } else {
            Some(reasoning_text_fragments.join("\n\n"))
        },
        tool_calls,
        native_calls,
        assistant_blocks: output.clone(),
        request_id: response_json
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| response_json.get("request_id").and_then(Value::as_str).map(ToOwned::to_owned)),
        organization_id: None,
    })
}

#[cfg(test)]
mod tests;
