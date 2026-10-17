//! Multimodal tool output and canonical compacted-history conversion.

use crate::provider::{AssistantPhase, ContentPart, Message, MessageContent, MessageRole, ToolCall};
use serde_json::Value;

use super::generate_tool_call_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FunctionOutputContentItem {
    InputText { text: String },
    InputImage { image_url: String },
}

impl FunctionOutputContentItem {
    fn from_value(value: &Value) -> Option<Self> {
        let item_type = value.get("type").and_then(Value::as_str)?;
        match item_type {
            "input_text" | "output_text" => Some(Self::InputText {
                text: value.get("text").and_then(Value::as_str)?.to_string(),
            }),
            "input_image" => Some(Self::InputImage {
                image_url: value.get("image_url").and_then(Value::as_str)?.to_string(),
            }),
            _ => None,
        }
    }

    fn to_function_output_json(&self) -> Value {
        match self {
            Self::InputText { text } => serde_json::json!({
                "type": "input_text",
                "text": text
            }),
            Self::InputImage { image_url } => serde_json::json!({
                "type": "input_image",
                "image_url": image_url
            }),
        }
    }

    fn to_tool_result_json(&self) -> Value {
        match self {
            Self::InputText { text } => serde_json::json!({
                "type": "output_text",
                "text": text
            }),
            Self::InputImage { image_url } => serde_json::json!({
                "type": "input_image",
                "image_url": image_url
            }),
        }
    }
}

fn parse_function_output_content_items_array(items: &[Value]) -> Option<Vec<FunctionOutputContentItem>> {
    items
        .iter()
        .map(FunctionOutputContentItem::from_value)
        .collect::<Option<Vec<_>>>()
}

fn parse_function_output_content_items_value(value: &Value) -> Option<Vec<FunctionOutputContentItem>> {
    match value {
        Value::Array(items) => parse_function_output_content_items_array(items),
        Value::Object(obj) => ["content_items", "content", "output", "body"]
            .iter()
            .find_map(|key| obj.get(*key))
            .and_then(parse_function_output_content_items_value),
        Value::String(text) => parse_function_output_content_items_text(text),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

fn parse_function_output_content_items_text(text: &str) -> Option<Vec<FunctionOutputContentItem>> {
    let trimmed = text.trim();
    if !(trimmed.starts_with('[') || trimmed.starts_with('{')) {
        return None;
    }
    let parsed: Value = serde_json::from_str(trimmed).ok()?;
    parse_function_output_content_items_value(&parsed)
}

fn function_output_items_from_parts(parts: &[ContentPart]) -> Vec<FunctionOutputContentItem> {
    let mut items = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text { text } => {
                if text.trim().is_empty() {
                    continue;
                }
                items.push(FunctionOutputContentItem::InputText { text: text.clone() });
            }
            ContentPart::Image { data, mime_type, .. } => {
                items.push(FunctionOutputContentItem::InputImage {
                    image_url: format!("data:{mime_type};base64,{data}"),
                });
            }
            ContentPart::File { .. } => {}
        }
    }
    items
}

pub(crate) fn tool_result_content_from_message_content(content: &MessageContent) -> Vec<Value> {
    match content {
        MessageContent::Text(text) => {
            if text.trim().is_empty() {
                return Vec::new();
            }
            if let Some(items) = parse_function_output_content_items_text(text) {
                return items.iter().map(FunctionOutputContentItem::to_tool_result_json).collect();
            }
            vec![serde_json::json!({
                "type": "output_text",
                "text": text
            })]
        }
        MessageContent::Parts(parts) => function_output_items_from_parts(parts)
            .iter()
            .map(FunctionOutputContentItem::to_tool_result_json)
            .collect(),
    }
}

fn function_output_value_from_items(items: Vec<FunctionOutputContentItem>) -> Value {
    if items.is_empty() {
        return Value::String(String::new());
    }
    let has_image = items
        .iter()
        .any(|item| matches!(item, FunctionOutputContentItem::InputImage { .. }));
    if has_image {
        return Value::Array(items.iter().map(FunctionOutputContentItem::to_function_output_json).collect());
    }
    Value::String(text_from_function_output_items(&items).unwrap_or_default())
}

pub(crate) fn function_output_value_from_message_content(content: &MessageContent) -> Value {
    match content {
        MessageContent::Text(text) => {
            if let Some(items) = parse_function_output_content_items_text(text) {
                return function_output_value_from_items(items);
            }
            Value::String(text.clone())
        }
        MessageContent::Parts(parts) => {
            let items = function_output_items_from_parts(parts);
            function_output_value_from_items(items)
        }
    }
}

fn text_from_function_output_items(items: &[FunctionOutputContentItem]) -> Option<String> {
    let mut text = String::new();
    for item in items {
        match item {
            FunctionOutputContentItem::InputText { text: segment } => text.push_str(segment),
            FunctionOutputContentItem::InputImage { .. } => return None,
        }
    }
    Some(text)
}

fn function_output_value_to_history_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(items) = parse_function_output_content_items_value(value)
        && let Some(text) = text_from_function_output_items(&items)
    {
        return text;
    }
    if let Some(text) = value.get("content").and_then(Value::as_str) {
        return text.to_string();
    }
    value.to_string()
}

fn append_output_item_text(value: &Value, text: &mut String) {
    if let Some(part_text) = value.get("text").and_then(Value::as_str) {
        text.push_str(part_text);
    }
    if let Some(part_output) = value.get("output").and_then(Value::as_str) {
        text.push_str(part_output);
    }
    if let Some(refusal) = value.get("refusal").and_then(Value::as_str) {
        text.push_str(refusal);
    }

    match value {
        Value::String(s) => text.push_str(s),
        Value::Array(parts) => {
            for part in parts {
                append_output_item_text(part, text);
            }
        }
        Value::Object(_) => {
            if let Some(content) = value.get("content") {
                append_output_item_text(content, text);
            }
        }
        _ => {}
    }
}

fn output_item_text(content: &Value) -> String {
    let mut text = String::new();
    append_output_item_text(content, &mut text);
    text
}

fn parse_function_call_item(item: &Value) -> Option<ToolCall> {
    let function_obj = item.get("function").and_then(Value::as_object);
    let namespace = item
        .get("namespace")
        .and_then(Value::as_str)
        .or_else(|| function_obj.and_then(|f| f.get("namespace").and_then(Value::as_str)))
        .map(ToOwned::to_owned);
    let name = function_obj
        .and_then(|f| f.get("name").and_then(Value::as_str))
        .or_else(|| item.get("name").and_then(Value::as_str))?
        .to_string();

    let id = item
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| item.get("call_id").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .unwrap_or("tool_call_compacted")
        .to_string();

    let arguments_value = function_obj.and_then(|f| f.get("arguments")).or_else(|| item.get("arguments"));
    let arguments = arguments_value.map_or_else(
        || "{}".to_string(),
        |value| value.as_str().map(ToOwned::to_owned).unwrap_or_else(|| value.to_string()),
    );

    Some(ToolCall::function_with_namespace(id, namespace, name, arguments))
}

fn parse_message_item(item: &Value) -> Option<Message> {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("assistant");
    let content_value = item.get("content").unwrap_or(&Value::Null);
    let content = output_item_text(content_value).trim().to_string();

    let tool_calls: Vec<ToolCall> = content_value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
            if part_type == "function_call" || part_type == "tool_call" {
                parse_function_call_item(part)
            } else {
                None
            }
        })
        .collect();

    let tool_result = content_value.as_array().into_iter().flatten().find_map(|part| {
        let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
        if part_type != "tool_result" {
            return None;
        }

        let tool_call_id = part
            .get("tool_call_id")
            .and_then(Value::as_str)
            .or_else(|| item.get("tool_call_id").and_then(Value::as_str))
            .or_else(|| item.get("call_id").and_then(Value::as_str))
            .map(ToOwned::to_owned)?;

        let tool_output = output_item_text(part.get("content").unwrap_or(&Value::Null)).trim().to_string();
        Some((tool_call_id, tool_output))
    });

    let assistant_phase = item
        .get("phase")
        .and_then(Value::as_str)
        .and_then(AssistantPhase::from_wire_str);

    match role {
        "system" => Some(Message::system(content)),
        "developer" => Some(Message::system(content)),
        "user" => Some(Message::user(content)),
        "assistant" => {
            if tool_calls.is_empty() {
                Some(Message::assistant(content).with_phase(assistant_phase))
            } else {
                Some(Message::assistant_with_tools(content, tool_calls).with_phase(assistant_phase))
            }
        }
        "tool" => {
            if let Some((tool_call_id, tool_output)) = tool_result {
                return Some(Message::tool_response(tool_call_id, tool_output));
            }

            let tool_call_id = item
                .get("tool_call_id")
                .and_then(Value::as_str)
                .or_else(|| item.get("call_id").and_then(Value::as_str))
                .map(ToOwned::to_owned)?;
            Some(Message::tool_response(tool_call_id, content))
        }
        _ => Some(Message {
            role: MessageRole::Assistant,
            content: MessageContent::text(content),
            ..Message::default()
        }),
    }
}

#[inline]
fn preserve_opaque_item(item: &Value) -> Message {
    Message::assistant(String::new()).with_reasoning_details(Some(vec![item.clone()]))
}

/// Convert `/responses/compact` output items into VT Code message history.
///
/// Opaque/unmapped items are preserved in `reasoning_details` so they can be
/// forwarded back to Responses-compatible providers on subsequent turns.
pub(crate) fn parse_compacted_output_messages(output: &[Value]) -> Vec<Message> {
    let mut messages = Vec::new();

    for item in output {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        match item_type {
            "message" => {
                if let Some(message) = parse_message_item(item) {
                    messages.push(message);
                } else {
                    messages.push(preserve_opaque_item(item));
                }
            }
            "function_call" | "tool_call" => {
                if let Some(tool_call) = parse_function_call_item(item) {
                    messages.push(Message::assistant_with_tools(String::new(), vec![tool_call]));
                }
            }
            "function_call_output" => {
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("id").and_then(Value::as_str))
                    .filter(|value| !value.is_empty());
                if let Some(call_id) = call_id {
                    let output_text = item
                        .get("output")
                        .map(function_output_value_to_history_text)
                        .unwrap_or_default();
                    messages.push(Message::tool_response(call_id.to_string(), output_text));
                } else {
                    messages.push(preserve_opaque_item(item));
                }
            }
            _ => {
                messages.push(preserve_opaque_item(item));
            }
        }
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_compacted_output_messages_keeps_messages() {
        let output = vec![json!({
            "type": "message",
            "role": "assistant",
            "phase": "final_answer",
            "content": [
                { "type": "output_text", "text": "Compacted response" }
            ]
        })];

        let parsed = parse_compacted_output_messages(&output);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].role, MessageRole::Assistant);
        assert_eq!(parsed[0].phase, Some(AssistantPhase::FinalAnswer));
        assert_eq!(parsed[0].content.as_text(), "Compacted response");
    }

    #[test]
    fn parse_compacted_output_messages_keeps_tool_pairs() {
        let output = vec![
            json!({
                "type": "function_call",
                "id": "call_1",
                "name": "shell",
                "arguments": "{\"command\":\"pwd\"}"
            }),
            json!({
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "/tmp/work"
            }),
        ];

        let parsed = parse_compacted_output_messages(&output);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].role, MessageRole::Assistant);
        assert!(parsed[0].tool_calls.is_some());
        assert_eq!(parsed[1].role, MessageRole::Tool);
        assert_eq!(parsed[1].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn parse_compacted_output_messages_serializes_multimodal_function_output() {
        let output = vec![json!({
            "type": "function_call_output",
            "call_id": "call_1",
            "output": [
                { "type": "input_text", "text": "inline image note" },
                { "type": "input_image", "image_url": "data:image/png;base64,abc" }
            ]
        })];

        let parsed = parse_compacted_output_messages(&output);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].role, MessageRole::Tool);
        assert_eq!(parsed[0].tool_call_id.as_deref(), Some("call_1"));
        let text = parsed[0].content.as_text();
        assert!(text.contains("\"input_image\""));
        assert!(text.contains("inline image note"));
    }

    #[test]
    fn function_output_value_parses_multimodal_tool_output_text() {
        let content = MessageContent::Text(
            r#"[{"type":"input_text","text":"note"},{"type":"input_image","image_url":"data:image/png;base64,abc"}]"#
                .to_string(),
        );
        let output = function_output_value_from_message_content(&content);
        let items = output.as_array().expect("expected array output");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["type"], "input_text");
        assert_eq!(items[0]["text"], "note");
        assert_eq!(items[1]["type"], "input_image");
        assert_eq!(items[1]["image_url"], "data:image/png;base64,abc");
    }

    #[test]
    fn parse_compacted_output_messages_preserves_compaction_items() {
        let output = vec![json!({
            "type": "compaction",
            "encrypted_content": "opaque_state"
        })];

        let parsed = parse_compacted_output_messages(&output);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].role, MessageRole::Assistant);
        let preserved = parsed[0]
            .reasoning_details
            .as_ref()
            .and_then(|items| items.first())
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str);
        assert_eq!(preserved, Some("compaction"));
    }

    #[test]
    fn parse_compacted_output_messages_parses_tool_result_messages() {
        let output = vec![json!({
            "type": "message",
            "role": "tool",
            "content": [
                {
                    "type": "tool_result",
                    "tool_call_id": "call_42",
                    "content": [
                        { "type": "output_text", "text": "done" }
                    ]
                }
            ]
        })];

        let parsed = parse_compacted_output_messages(&output);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].role, MessageRole::Tool);
        assert_eq!(parsed[0].tool_call_id.as_deref(), Some("call_42"));
        assert_eq!(parsed[0].content.as_text(), "done");
    }
}
