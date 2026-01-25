use serde_json::Value;

use crate::llm::provider::ToolCall;

use super::super::{extract_reasoning_trace, split_reasoning_from_text};

pub(super) fn append_reasoning_segment(segments: &mut Vec<String>, text: &str) {
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if segments
            .last()
            .map(|last| last.as_str() == trimmed)
            .unwrap_or(false)
        {
            continue;
        }
        segments.push(trimmed.to_string());
    }
}

pub(super) fn extract_tool_calls_from_content(message: &Value) -> Option<Vec<ToolCall>> {
    let parts = message.get("content").and_then(|value| value.as_array())?;
    let mut calls: Vec<ToolCall> = Vec::with_capacity(parts.len());

    for (index, part) in parts.iter().enumerate() {
        let map = match part.as_object() {
            Some(value) => value,
            None => continue,
        };

        let content_type = map.get("type").and_then(|value| value.as_str());
        let is_tool_call = matches!(content_type, Some("tool_call") | Some("function_call"))
            || (content_type.is_none()
                && map.contains_key("name")
                && map.contains_key("arguments"));

        if !is_tool_call {
            continue;
        }

        let id = map
            .get("id")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("tool_call_{}", index));

        let (name, arguments_value) =
            if let Some(function) = map.get("function").and_then(|value| value.as_object()) {
                (
                    function
                        .get("name")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string()),
                    function.get("arguments"),
                )
            } else {
                (
                    map.get("name")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string()),
                    map.get("arguments"),
                )
            };

        let Some(name) = name else {
            continue;
        };

        let arguments = arguments_value
            .map(|value| {
                if let Some(text) = value.as_str() {
                    text.to_string()
                } else if value.is_null() {
                    "{}".to_string()
                } else {
                    value.to_string()
                }
            })
            .unwrap_or_else(|| "{}".to_string());

        calls.push(ToolCall::function(id, name, arguments));
    }

    if calls.is_empty() { None } else { Some(calls) }
}

pub(super) fn extract_reasoning_from_message_content(message: &Value) -> Option<String> {
    let parts = message.get("content")?.as_array()?;
    let mut segments: Vec<String> = Vec::new();

    fn push_segment(segments: &mut Vec<String>, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return;
        }
        if segments
            .last()
            .map(|last| last.as_str() == trimmed)
            .unwrap_or(false)
        {
            return;
        }
        segments.push(trimmed.to_string());
    }

    for part in parts {
        match part {
            Value::Object(map) => {
                let part_type = map
                    .get("type")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");

                if matches!(part_type, "reasoning" | "thinking" | "analysis") {
                    if let Some(extracted) = extract_reasoning_trace(part) {
                        if !extracted.trim().is_empty() {
                            segments.push(extracted);
                            continue;
                        }
                    }

                    if let Some(text) = map.get("text").and_then(|value| value.as_str()) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            push_segment(&mut segments, trimmed);
                        }
                    }
                }
            }
            Value::String(text) => {
                let (mut markup_segments, cleaned) = split_reasoning_from_text(text);
                if !markup_segments.is_empty() {
                    for segment in markup_segments.drain(..) {
                        push_segment(&mut segments, &segment);
                    }
                    if let Some(cleaned_text) = cleaned {
                        push_segment(&mut segments, &cleaned_text);
                    }
                } else {
                    push_segment(&mut segments, text);
                }
            }
            _ => {}
        }
    }

    if segments.is_empty() {
        None
    } else {
        let mut combined = String::new();
        for (idx, segment) in segments.iter().enumerate() {
            if idx > 0 {
                combined.push('\n');
            }
            combined.push_str(segment);
        }
        Some(combined)
    }
}
