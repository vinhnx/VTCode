use crate::llm::error_display;
use crate::llm::provider::{LLMError, ToolCall};
use crate::llm::providers::{ReasoningBuffer, split_reasoning_from_text};
use serde_json::{Map, Value};

#[derive(Debug, thiserror::Error)]
pub enum StreamAssemblyError {
    #[error("missing field `{0}` in stream payload")]
    MissingField(&'static str),
    #[error("invalid stream payload: {0}")]
    InvalidPayload(String),
}

impl StreamAssemblyError {
    pub fn into_llm_error(self, provider: &str) -> LLMError {
        let message = self.to_string();
        let formatted = error_display::format_llm_error(provider, &message);
        LLMError::Provider(formatted)
    }
}

pub trait StreamTelemetry: Send + Sync {
    fn on_content_delta(&self, _delta: &str) {}
    fn on_reasoning_delta(&self, _delta: &str) {}
    fn on_tool_call_delta(&self) {}
}

#[derive(Default)]
pub struct NoopStreamTelemetry;

impl StreamTelemetry for NoopStreamTelemetry {}

#[derive(Default, Clone)]
pub struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCallBuilder {
    pub fn apply_delta(&mut self, delta: &Value) {
        if let Some(id) = delta.get("id").and_then(|value| value.as_str()) {
            self.id = Some(id.to_string());
        }

        if let Some(function) = delta.get("function") {
            if let Some(name) = function.get("name").and_then(|value| value.as_str()) {
                self.name = Some(name.to_string());
            }

            if let Some(arguments_value) = function.get("arguments") {
                if let Some(arguments) = arguments_value.as_str() {
                    self.arguments.push_str(arguments);
                } else if arguments_value.is_object() || arguments_value.is_array() {
                    self.arguments.push_str(&arguments_value.to_string());
                }
            }
        }
    }

    pub fn finalize(self, fallback_index: usize) -> Option<ToolCall> {
        let name = self.name?;
        let id = self
            .id
            .unwrap_or_else(|| format!("tool_call_{}", fallback_index));
        let arguments = if self.arguments.is_empty() {
            "{}".to_string()
        } else {
            self.arguments
        };

        Some(ToolCall::function(id, name, arguments))
    }
}

pub fn update_tool_calls(builders: &mut Vec<ToolCallBuilder>, deltas: &[Value]) {
    for (index, delta) in deltas.iter().enumerate() {
        if builders.len() <= index {
            builders.push(ToolCallBuilder::default());
        }
        let builder = builders
            .get_mut(index)
            .expect("tool call builder must exist after push");

        builder.apply_delta(delta);
    }
}

pub fn finalize_tool_calls(builders: Vec<ToolCallBuilder>) -> Option<Vec<ToolCall>> {
    let calls: Vec<ToolCall> = builders
        .into_iter()
        .enumerate()
        .filter_map(|(index, builder)| builder.finalize(index))
        .collect();

    if calls.is_empty() { None } else { Some(calls) }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StreamFragment {
    Content(String),
    Reasoning(String),
}

#[derive(Default, Debug)]
pub struct StreamDelta {
    fragments: Vec<StreamFragment>,
}

impl StreamDelta {
    pub fn push_content(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        match self.fragments.last_mut() {
            Some(StreamFragment::Content(existing)) => existing.push_str(text),
            _ => self
                .fragments
                .push(StreamFragment::Content(text.to_string())),
        }
    }

    pub fn push_reasoning(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        match self.fragments.last_mut() {
            Some(StreamFragment::Reasoning(existing)) => existing.push_str(text),
            _ => self
                .fragments
                .push(StreamFragment::Reasoning(text.to_string())),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }

    pub fn into_fragments(self) -> Vec<StreamFragment> {
        self.fragments
    }

    pub fn extend(&mut self, other: StreamDelta) {
        self.fragments.extend(other.fragments);
    }
}

pub fn append_text_with_reasoning(
    text: &str,
    aggregated_content: &mut String,
    reasoning: &mut ReasoningBuffer,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    let (segments, cleaned) = split_reasoning_from_text(text);

    if segments.is_empty() && cleaned.is_none() {
        if !text.is_empty() {
            aggregated_content.push_str(text);
            deltas.push_content(text);
            telemetry.on_content_delta(text);
        }
        return;
    }

    for segment in segments {
        if let Some(delta) = reasoning.push(&segment) {
            telemetry.on_reasoning_delta(&delta);
            deltas.push_reasoning(&delta);
        }
    }

    if let Some(cleaned_text) = cleaned {
        if !cleaned_text.is_empty() {
            aggregated_content.push_str(&cleaned_text);
            telemetry.on_content_delta(&cleaned_text);
            deltas.push_content(&cleaned_text);
        }
    }
}

pub fn append_reasoning_segments(
    reasoning: &mut ReasoningBuffer,
    text: &str,
    telemetry: &impl StreamTelemetry,
) -> Vec<String> {
    let mut emitted = Vec::new();
    let (mut segments, cleaned) = split_reasoning_from_text(text);

    if !segments.is_empty() {
        for segment in segments.drain(..) {
            if let Some(delta) = reasoning.push(&segment) {
                telemetry.on_reasoning_delta(&delta);
                emitted.push(delta);
            }
        }

        if let Some(cleaned_text) = cleaned {
            let trimmed = cleaned_text.trim();
            if !trimmed.is_empty() {
                if let Some(delta) = reasoning.push(trimmed) {
                    telemetry.on_reasoning_delta(&delta);
                    emitted.push(delta);
                }
            }
        }
    } else if let Some(delta) = reasoning.push(text) {
        telemetry.on_reasoning_delta(&delta);
        emitted.push(delta);
    }

    emitted
}

pub fn extract_data_payload(event: &str) -> Option<String> {
    let mut data_lines: Vec<String> = Vec::new();

    for raw_line in event.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}

pub fn find_sse_boundary(buffer: &str) -> Option<(usize, usize)> {
    let newline_boundary = buffer.find("\n\n").map(|idx| (idx, 2));
    let carriage_boundary = buffer.find("\r\n\r\n").map(|idx| (idx, 4));

    match (newline_boundary, carriage_boundary) {
        (Some((n_idx, n_len)), Some((c_idx, c_len))) => {
            if n_idx <= c_idx {
                Some((n_idx, n_len))
            } else {
                Some((c_idx, c_len))
            }
        }
        (Some(boundary), None) => Some(boundary),
        (None, Some(boundary)) => Some(boundary),
        (None, None) => None,
    }
}

pub fn apply_tool_call_delta_from_content(
    builders: &mut Vec<ToolCallBuilder>,
    container: &Map<String, Value>,
    telemetry: &impl StreamTelemetry,
) {
    apply_tool_call_delta_with_index(builders, container, telemetry, None);
}

fn apply_tool_call_delta_with_index(
    builders: &mut Vec<ToolCallBuilder>,
    container: &Map<String, Value>,
    telemetry: &impl StreamTelemetry,
    fallback_index: Option<usize>,
) {
    let explicit_index = container
        .get("tool_call")
        .and_then(|value| value.as_object())
        .and_then(|tool_call| tool_call.get("index"))
        .and_then(|value| value.as_u64())
        .or_else(|| container.get("index").and_then(|value| value.as_u64()));

    let index = explicit_index
        .map(|value| value as usize)
        .or(fallback_index)
        .unwrap_or(0);

    if let Some(nested) = container.get("delta").and_then(|value| value.as_object()) {
        apply_tool_call_delta_with_index(builders, nested, telemetry, Some(index));
    }

    let delta_source = container
        .get("tool_call")
        .and_then(|value| value.as_object())
        .unwrap_or(container);

    let mut delta_map = Map::new();

    if let Some(id_value) = delta_source.get("id").or_else(|| container.get("id")) {
        delta_map.insert("id".to_string(), id_value.clone());
    }

    if let Some(function_value) = delta_source
        .get("function")
        .or_else(|| container.get("function"))
    {
        delta_map.insert("function".to_string(), function_value.clone());
    }

    if delta_map.is_empty() {
        return;
    }

    if builders.len() <= index {
        builders.resize_with(index + 1, ToolCallBuilder::default);
    }

    let mut deltas = vec![Value::Null; index + 1];
    deltas[index] = Value::Object(delta_map);
    update_tool_calls(builders, &deltas);
    telemetry.on_tool_call_delta();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finalize_tool_calls_drops_empty_builders() {
        let builders = vec![ToolCallBuilder::default()];
        assert!(finalize_tool_calls(builders).is_none());
    }

    #[test]
    fn append_text_with_reasoning_tracks_segments() {
        let telemetry = NoopStreamTelemetry::default();
        let mut aggregated = String::new();
        let mut reasoning = ReasoningBuffer::default();
        let mut delta = StreamDelta::default();
        append_text_with_reasoning(
            "Hello",
            &mut aggregated,
            &mut reasoning,
            &mut delta,
            &telemetry,
        );
        assert_eq!(aggregated, "Hello");
        assert_eq!(
            delta.into_fragments(),
            vec![StreamFragment::Content("Hello".into())]
        );
    }

    #[test]
    fn apply_tool_call_delta_updates_builder() {
        let telemetry = NoopStreamTelemetry::default();
        let mut builders = Vec::new();
        let container = json!({
            "index": 0,
            "function": {"name": "foo", "arguments": "{}"}
        })
        .as_object()
        .cloned()
        .unwrap();
        apply_tool_call_delta_from_content(&mut builders, &container, &telemetry);
        let calls = finalize_tool_calls(builders).expect("call expected");
        assert_eq!(calls[0].function.name, "foo");
    }

    #[test]
    fn apply_tool_call_delta_uses_outer_index_for_nested_delta() {
        let telemetry = NoopStreamTelemetry::default();
        let mut builders = Vec::new();
        let container = json!({
            "delta": {
                "tool_call": {
                    "function": {
                        "arguments": "{\"value\":1}"
                    }
                }
            },
            "index": 1,
            "id": "call-1"
        })
        .as_object()
        .cloned()
        .unwrap();

        apply_tool_call_delta_from_content(&mut builders, &container, &telemetry);

        let calls = finalize_tool_calls(builders).expect("call expected");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].function.arguments, "{\"value\":1}");
    }

    #[test]
    fn extract_data_payload_merges_lines() {
        let event = ": keep-alive\n".to_string() + "data: {\"a\":1}\n" + "data: {\"b\":2}\n";
        let payload = extract_data_payload(&event);
        assert_eq!(payload.as_deref(), Some("{\"a\":1}\n{\"b\":2}"));
    }

    #[test]
    fn find_sse_boundary_prefers_newline() {
        let buffer = "data: foo\n\nrest";
        assert_eq!(find_sse_boundary(buffer), Some((9, 2)));
    }
}
