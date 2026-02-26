use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMResponse, ToolCall};
pub use crate::llm::providers::ReasoningBuffer;
mod tag_sanitizer;
use crate::llm::providers::split_reasoning_from_text;
use serde_json::{Map, Value};
pub use tag_sanitizer::TagStreamSanitizer;

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
        LLMError::Provider {
            message: formatted,
            metadata: None,
        }
    }
}

pub trait StreamTelemetry: Send + Sync {
    fn on_content_delta(&self, _delta: &str) {}
    fn on_reasoning_delta(&self, _delta: &str) {}
    fn on_reasoning_stage(&self, _stage: &str) {}
    fn on_tool_call_delta(&self) {}
}

#[derive(Default)]
#[allow(dead_code)]
pub struct NoopStreamTelemetry;

impl StreamTelemetry for NoopStreamTelemetry {}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamFragment {
    Content(String),
    Reasoning(String),
}

#[allow(dead_code)]
#[derive(Default, Debug)]
pub struct StreamDelta {
    fragments: Vec<StreamFragment>,
}

#[allow(dead_code)]
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
    for (position, delta) in deltas.iter().enumerate() {
        let index = delta
            .get("index")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or(position);

        if builders.len() <= index {
            builders.resize_with(index + 1, ToolCallBuilder::default);
        }
        let builder = builders
            .get_mut(index)
            .expect("tool call builder must exist after resize");

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

/// Helper to aggregate streaming events and produce a final LLMResponse.
pub struct StreamAggregator {
    pub model: String,
    pub content: String,
    pub reasoning: String,
    pub reasoning_buffer: ReasoningBuffer,
    pub tool_builders: Vec<ToolCallBuilder>,
    pub usage: Option<crate::llm::provider::Usage>,
    pub finish_reason: crate::llm::provider::FinishReason,
    pub sanitizer: TagStreamSanitizer,
}

impl StreamAggregator {
    pub fn new(model: String) -> Self {
        Self {
            model,
            content: String::new(),
            reasoning: String::new(),
            reasoning_buffer: ReasoningBuffer::default(),
            tool_builders: Vec::new(),
            usage: None,
            finish_reason: crate::llm::provider::FinishReason::Stop,
            sanitizer: TagStreamSanitizer::new(),
        }
    }

    /// Process a content delta, applying sanitization for reasoning tags.
    pub fn handle_content(&mut self, delta: &str) -> Vec<crate::llm::provider::LLMStreamEvent> {
        self.content.push_str(delta);
        self.sanitizer.process_chunk(delta)
    }

    /// Process a reasoning delta from a dedicated field.
    pub fn handle_reasoning(&mut self, delta: &str) -> Option<String> {
        let result = self.reasoning_buffer.push(delta);
        if let Some(ref d) = result {
            self.reasoning.push_str(d);
        }
        result
    }

    /// Process tool call deltas.
    pub fn handle_tool_calls(&mut self, deltas: &[Value]) {
        update_tool_calls(&mut self.tool_builders, deltas);
    }

    /// Set usage metrics.
    pub fn set_usage(&mut self, usage: crate::llm::provider::Usage) {
        self.usage = Some(usage);
    }

    /// Set finish reason.
    pub fn set_finish_reason(&mut self, reason: crate::llm::provider::FinishReason) {
        self.finish_reason = reason;
    }

    /// Finalize and produce the completed LLMResponse.
    pub fn finalize(mut self) -> LLMResponse {
        // Collect any leftover bits from sanitizer
        for event in self.sanitizer.finalize() {
            match event {
                crate::llm::provider::LLMStreamEvent::Token { delta } => {
                    self.content.push_str(&delta);
                }
                crate::llm::provider::LLMStreamEvent::Reasoning { delta } => {
                    self.reasoning.push_str(&delta);
                }
                _ => {}
            }
        }

        LLMResponse {
            content: if self.content.is_empty() {
                None
            } else {
                Some(self.content)
            },
            tool_calls: finalize_tool_calls(self.tool_builders),
            model: self.model,
            usage: self.usage,
            finish_reason: self.finish_reason,
            reasoning: if self.reasoning.is_empty() {
                self.reasoning_buffer.finalize()
            } else {
                Some(self.reasoning)
            },
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        }
    }
}

/// Common helper for processing OpenAI-compatible SSE streams.
///
/// This simplifies stream implementations across providers like DeepSeek, ZAI, Moonshot, etc.
/// Especially optimized for high-performance models like Gemini 3 and GLM-5.
pub async fn process_openai_stream<S, E, F>(
    mut byte_stream: S,
    provider_name: &'static str,
    model: String,
    mut on_chunk: F,
) -> Result<LLMResponse, LLMError>
where
    S: futures::Stream<Item = Result<bytes::Bytes, E>> + Unpin,
    E: std::fmt::Display,
    F: FnMut(Value) -> Result<(), LLMError>,
{
    use crate::llm::providers::error_handling::format_network_error;
    use futures::StreamExt;

    let mut buffer = String::new();
    let mut last_response_value = None;

    while let Some(chunk_result) = byte_stream.next().await {
        let chunk_bytes =
            chunk_result.map_err(|e| format_network_error(provider_name, &e.to_string()))?;
        let chunk_str = String::from_utf8_lossy(&chunk_bytes);
        buffer.push_str(&chunk_str);

        while let Some((boundary_idx, boundary_len)) = find_sse_boundary(&buffer) {
            let event = buffer[..boundary_idx].to_string();
            buffer.drain(..boundary_idx + boundary_len);

            if let Some(data) = extract_data_payload(&event) {
                if data == "[DONE]" {
                    break;
                }

                for line in data.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                        on_chunk(value.clone())?;
                        last_response_value = Some(value);
                    }
                }
            }
        }
    }

    // Attempt to extract final response metadata (usage, etc) from last chunk if not already done
    let mut final_response = LLMResponse {
        content: None,
        tool_calls: None,
        model,
        usage: None,
        finish_reason: crate::llm::provider::FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
    };

    if let Some(value) = last_response_value
        && value.get("usage").is_some()
    {
        final_response.usage =
            crate::llm::providers::common::parse_usage_openai_format(&value, true);
    }

    Ok(final_response)
}

pub fn parse_openai_tool_calls(calls: &[Value]) -> Vec<ToolCall> {
    calls
        .iter()
        .filter_map(|call| {
            let id = call.get("id").and_then(|v| v.as_str())?;
            let function = call.get("function")?;
            let name = function.get("name").and_then(|v| v.as_str())?;
            let arguments = function.get("arguments");
            let serialized = arguments.map_or_else(
                || "{}".to_string(),
                |value| {
                    if value.is_string() {
                        value.as_str().unwrap_or("").to_string()
                    } else {
                        value.to_string()
                    }
                },
            );
            Some(ToolCall::function(
                id.to_string(),
                name.to_string(),
                serialized,
            ))
        })
        .collect()
}

#[allow(dead_code)]
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
        if let Some(stage) = &segment.stage {
            telemetry.on_reasoning_stage(stage);
        }
        if let Some(delta) = reasoning.push(&segment.text) {
            telemetry.on_reasoning_delta(&delta);
            deltas.push_reasoning(&delta);
        }
    }

    if let Some(cleaned_text) = cleaned
        && !cleaned_text.is_empty()
    {
        aggregated_content.push_str(&cleaned_text);
        telemetry.on_content_delta(&cleaned_text);
        deltas.push_content(&cleaned_text);
    }
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

#[allow(dead_code)]
pub fn apply_tool_call_delta_from_content(
    builders: &mut Vec<ToolCallBuilder>,
    container: &Map<String, Value>,
    telemetry: &impl StreamTelemetry,
) {
    apply_tool_call_delta_with_index(builders, container, telemetry, None, None);
}

#[allow(dead_code)]
fn apply_tool_call_delta_with_index(
    builders: &mut Vec<ToolCallBuilder>,
    container: &Map<String, Value>,
    telemetry: &impl StreamTelemetry,
    fallback_index: Option<usize>,
    fallback_id: Option<Value>,
) {
    fn extract_tool_call_id(container: &Map<String, Value>) -> Option<Value> {
        container.get("id").cloned().or_else(|| {
            container
                .get("tool_call")
                .and_then(|value| value.as_object())
                .and_then(|inner| inner.get("id"))
                .cloned()
        })
    }

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

    let current_id = extract_tool_call_id(container).or_else(|| fallback_id.clone());

    if let Some(nested) = container.get("delta").and_then(|value| value.as_object()) {
        apply_tool_call_delta_with_index(
            builders,
            nested,
            telemetry,
            Some(index),
            current_id.clone(),
        );
    }

    let delta_source = container
        .get("tool_call")
        .and_then(|value| value.as_object())
        .unwrap_or(container);

    let mut delta_map = Map::new();

    if let Some(id_value) = extract_tool_call_id(delta_source).or_else(|| current_id.clone()) {
        delta_map.insert("id".to_string(), id_value);
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
        let func = calls[0]
            .function
            .as_ref()
            .expect("function call should be present");
        assert_eq!(func.name, "foo");
    }

    #[test]
    fn apply_tool_call_delta_uses_outer_index_for_nested_delta() {
        let telemetry = NoopStreamTelemetry::default();
        let mut builders = Vec::new();
        let container = json!({
            "delta": {
                "tool_call": {
                    "function": {
                        "name": "foo",
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
        let func = calls[0]
            .function
            .as_ref()
            .expect("function call should be present");
        assert_eq!(func.arguments, "{\"value\":1}");
    }

    #[test]
    fn update_tool_calls_respects_explicit_index() {
        let mut builders = Vec::new();
        let deltas = vec![json!({
            "index": 2,
            "id": "call_3",
            "function": {
                "name": "get_weather",
                "arguments": "{\"city\":\"Beijing\"}"
            }
        })];

        update_tool_calls(&mut builders, &deltas);

        let calls = finalize_tool_calls(builders).expect("call expected");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_3");
        let function = calls[0].function.as_ref().expect("function expected");
        assert_eq!(function.name, "get_weather");
        assert_eq!(function.arguments, "{\"city\":\"Beijing\"}");
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
