//! Reasoning normalization and interleaved assistant history replay.

use crate::provider::{ContentPart, Message, MessageContent, MessageRole};
use serde_json::{Value, json};
use vtcode_commons::formatting::contains_ignore_ascii_case;

/// Returns true when the model identifier points to MiniMax M2 family models.
/// Works across direct model ids and provider-qualified ids.
#[inline]
pub(crate) fn is_minimax_m2_model(model: &str) -> bool {
    contains_ignore_ascii_case(model, "minimax-m2.5")
        || contains_ignore_ascii_case(model, "minimax-m2.7")
        || contains_ignore_ascii_case(model, "minimax-m3")
}

#[inline]
fn is_glm_interleaved_thinking_model(model: &str) -> bool {
    contains_ignore_ascii_case(model, "glm-5")
        || contains_ignore_ascii_case(model, "glm45")
        || contains_ignore_ascii_case(model, "glm-4.5")
}

/// Returns true when the model family relies on interleaved `<think>...</think>`
/// history to maintain reasoning quality across turns.
#[inline]
pub(crate) fn is_interleaved_thinking_model(model: &str) -> bool {
    is_minimax_m2_model(model) || is_glm_interleaved_thinking_model(model)
}

#[inline]
fn text_contains_interleaved_reasoning_markup(text: &str) -> bool {
    contains_ignore_ascii_case(text, "<think")
        || contains_ignore_ascii_case(text, "<thinking")
        || contains_ignore_ascii_case(text, "<reasoning")
        || contains_ignore_ascii_case(text, "<analysis")
        || contains_ignore_ascii_case(text, "<thought")
}

pub(super) fn message_content_is_text_only(content: &MessageContent) -> bool {
    match content {
        MessageContent::Text(_) => true,
        MessageContent::Parts(parts) => parts.iter().all(|part| matches!(part, ContentPart::Text { .. })),
    }
}

fn preserved_interleaved_content_from_details(details: &[Value]) -> Option<String> {
    details.iter().find_map(|detail| match detail {
        Value::String(text) if !text.trim().is_empty() && text_contains_interleaved_reasoning_markup(text) => {
            Some(text.clone())
        }
        _ => None,
    })
}

/// Rehydrates assistant history into the tagged form expected by interleaved
/// thinking models.
pub(crate) fn assistant_interleaved_history_text(message: &Message, model: &str) -> Option<String> {
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
pub(crate) fn preserve_interleaved_content_in_reasoning_details(
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
pub(crate) fn normalize_reasoning_detail_object(detail: &Value) -> Option<Value> {
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
pub(crate) fn normalize_reasoning_detail_objects(details: &[Value]) -> Vec<Value> {
    details.iter().filter_map(normalize_reasoning_detail_object).collect()
}

#[inline]
pub(crate) fn append_normalized_reasoning_detail_items(input: &mut Vec<Value>, details: &[Value]) {
    for item in details {
        if let Some(normalized) = normalize_reasoning_detail_object(item) {
            input.push(normalized);
        }
    }
}

#[inline]
pub(crate) fn serialize_reasoning_detail_values(details: &[Value]) -> Option<Vec<String>> {
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
    if normalized.is_empty() { None } else { Some(normalized) }
}

pub(super) fn serialize_reasoning_details_field(details: &Value) -> Option<Vec<String>> {
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

    crate::providers::extract_reasoning_trace(&normalized).and_then(|trace| {
        let cleaned = crate::providers::clean_reasoning_text(trace.trim());
        if cleaned.is_empty() { None } else { Some(cleaned) }
    })
}

pub fn extract_reasoning_text_from_detail_values(details: &[Value]) -> Option<String> {
    let mut fragments = Vec::new();
    for detail in details {
        let Some(text) = reasoning_text_from_detail_value(detail) else {
            continue;
        };
        if fragments.last().is_none_or(|existing| existing != &text) {
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
        if fragments.last().is_none_or(|existing| existing != &text) {
            fragments.push(text);
        }
    }

    if fragments.is_empty() {
        None
    } else {
        Some(fragments.join("\n\n"))
    }
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
pub fn make_anthropic_thinking_config(config: &vtcode_config::core::AnthropicConfig) -> Value {
    serde_json::json!({
        "thinking": {
            "type": config.interleaved_thinking_type_enabled,
            "budget_tokens": config.interleaved_thinking_budget_tokens
        }
    })
}
