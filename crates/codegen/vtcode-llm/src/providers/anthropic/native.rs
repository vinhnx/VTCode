//! Bound computer-use route for Anthropic Messages.
//!
//! The ordinary parser rejects native toolset blocks. This route declares the
//! toolset, then decodes natives alongside ordinaries into [`NativeResponse`].
//! History replay uses the verbatim assistant blocks, so thinking signatures
//! and block order survive untouched.

use serde_json::{Value, json};

use crate::error_display;
use crate::provider::{FinishReason, LLMError, NativeCall, NativeCallBody, NativeProvider, NativeResponse, ToolCall};

use super::computer_use::{declaration, decode_call};

/// Append the computer toolset entry to a request body built by the ordinary
/// converter, preserving any declared function tools for mixed turns.
///
/// The converter serializes a struct, so the body is always an object; the
/// guard below is defensive only.
pub(super) fn declare(body: &mut Value) {
    debug_assert!(body.is_object(), "Anthropic request body must be an object");
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

/// Parse a bound-route response: text, thinking, ordinary calls, and decoded
/// natives, with the verbatim content blocks for replay.
///
/// # Errors
///
/// Returns an error for a missing content array or an invalid native call
/// identity. Malformed member actions retain their correlation identity with
/// the decode failure instead of failing the turn.
pub(super) fn parse_response_native(response_json: Value, model: String) -> Result<NativeResponse, LLMError> {
    let content = response_json.get("content").and_then(Value::as_array).ok_or_else(|| {
        let formatted = error_display::format_llm_error("Anthropic", "Invalid response format: missing content");
        LLMError::Provider { message: formatted, metadata: None }
    })?;

    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut native_calls = Vec::new();
    let mut assistant_blocks = Vec::with_capacity(content.len());
    let declined = super::response_parser::declined_partial_positions(content);
    for (position, block) in content.iter().enumerate() {
        if declined.get(position) == Some(&true) {
            continue;
        }
        assistant_blocks.push(block.clone());
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    text_parts.push(text.to_string());
                }
            }
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                    reasoning_parts.push(thinking.to_string());
                }
            }
            Some("tool_use") => match decode_call(block).map_err(|error| LLMError::Provider {
                message: format!("Invalid Anthropic computer call: {error}"),
                metadata: None,
            })? {
                Some(call) => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                    native_calls.push(NativeCall {
                        id: call.id().to_string(),
                        provider: NativeProvider::Anthropic,
                        body: NativeCallBody::Anthropic {
                            name,
                            action: call.action.map_err(|error| error.to_string()),
                        },
                    });
                }
                None => {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    if name == "structured_output" {
                        let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                        text_parts.push(input.to_string());
                    } else if !id.is_empty() && !name.is_empty() {
                        let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                        let arguments = input.to_string();
                        tool_calls.push(ToolCall::function(id, name, arguments));
                    }
                }
            },
            _ => {}
        }
    }

    let reasoning = if reasoning_parts.is_empty() {
        None
    } else {
        let joined = reasoning_parts.join("\n");
        let trimmed = joined.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };
    let stop_reason = response_json.get("stop_reason").and_then(Value::as_str).unwrap_or("end_turn");
    Ok(NativeResponse {
        content: if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.concat())
        },
        model,
        usage: response_json.get("usage").map(super::response_parser::parse_usage),
        finish_reason: super::response_parser::parse_finish_reason(stop_reason),
        reasoning,
        tool_calls,
        native_calls,
        assistant_blocks,
        request_id: None,
        organization_id: None,
    })
}

#[cfg(test)]
mod tests;
