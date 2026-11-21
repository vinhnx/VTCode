//! OpenAI-compatible stream assembly, ordered deltas, and tool-call accumulation.

use crate::error_display;
use crate::provider::{LLMError, LLMResponse, LLMStreamEvent, ToolCall};
use crate::providers::common::{extract_reasoning_text_from_serialized_details, map_finish_reason_common};
use crate::providers::split_reasoning_from_text;
use serde_json::{Map, Value};

use super::sse::find_sse_boundary;
use super::{
    ReasoningBuffer, TagStreamSanitizer, Utf8StreamDecoder, drain_and_bound_sse_tail, extract_data_payload,
    find_sse_boundary_bytes,
};

#[derive(Debug, thiserror::Error)]
pub enum StreamAssemblyError {
    #[error("missing field `{0}` in stream payload")]
    MissingField(&'static str),
    #[error("invalid stream payload: {0}")]
    InvalidPayload(String),
}

impl StreamAssemblyError {
    #[cold]
    pub(crate) fn into_llm_error(self, provider: &str) -> LLMError {
        let message = self.to_string();
        let formatted = error_display::format_llm_error(provider, &message);
        LLMError::Provider { message: formatted, metadata: None }
    }
}

pub trait StreamTelemetry: Send + Sync {
    fn on_content_delta(&self, _delta: &str) {}
    fn on_reasoning_delta(&self, _delta: &str) {}
    fn on_reasoning_stage(&self, _stage: &str) {}
    fn on_tool_call_delta(&self) {}
}

#[derive(Default)]
pub struct NoopStreamTelemetry;

impl StreamTelemetry for NoopStreamTelemetry {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamFragment {
    Content(String),
    Reasoning(String),
}

#[derive(Default, Debug)]
pub struct StreamDelta {
    fragments: Vec<StreamFragment>,
}

impl StreamDelta {
    pub(crate) fn push_content(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        match self.fragments.last_mut() {
            Some(StreamFragment::Content(existing)) => existing.push_str(text),
            _ => self.fragments.push(StreamFragment::Content(text.to_string())),
        }
    }

    pub(crate) fn push_reasoning(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        match self.fragments.last_mut() {
            Some(StreamFragment::Reasoning(existing)) => existing.push_str(text),
            _ => self.fragments.push(StreamFragment::Reasoning(text.to_string())),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }

    pub(crate) fn into_fragments(self) -> Vec<StreamFragment> {
        self.fragments
    }

    pub fn extend(&mut self, other: StreamDelta) {
        self.fragments.extend(other.fragments);
    }
}

/// Generate a globally unique fallback id for tool calls whose provider
/// omitted one. Index-based fallbacks (`tool_call_{index}`) reset every
/// response, so the same id recurs across assistant messages and corrupts
/// id-keyed history correlation downstream. Uniqueness across responses and
/// process restarts (resumed sessions replay ids from checkpoints) is
/// required, hence a random uuid rather than a counter.
pub(crate) fn generate_tool_call_id() -> String {
    format!("call_{}", uuid::Uuid::new_v4().simple())
}

#[derive(Default, Clone)]
pub struct ToolCallBuilder {
    id: Option<String>,
    namespace: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCallBuilder {
    pub(crate) fn apply_delta(&mut self, delta: &Value) {
        if let Some(id) = delta.get("id").and_then(|value| value.as_str()) {
            self.id = Some(id.to_string());
        }

        if let Some(namespace) = delta.get("namespace").and_then(|value| value.as_str()) {
            self.namespace = Some(namespace.to_string());
        }

        if let Some(function) = delta.get("function") {
            if let Some(namespace) = function.get("namespace").and_then(|value| value.as_str()) {
                self.namespace = Some(namespace.to_string());
            }

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

    fn finalize(self) -> Option<ToolCall> {
        let name = self.name?;
        let id = self.id.unwrap_or_else(generate_tool_call_id);
        let arguments = if self.arguments.is_empty() {
            "{}".to_string()
        } else {
            self.arguments
        };

        Some(ToolCall::function_with_namespace(id, self.namespace, name, arguments))
    }
}

fn update_tool_calls(builders: &mut Vec<ToolCallBuilder>, deltas: &[Value]) {
    for (position, delta) in deltas.iter().enumerate() {
        let index = delta
            .get("index")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or(position);

        if builders.len() <= index {
            builders.resize_with(index + 1, ToolCallBuilder::default);
        }
        let Some(builder) = builders.get_mut(index) else {
            continue;
        };

        builder.apply_delta(delta);
    }
}

fn finalize_tool_calls(builders: Vec<ToolCallBuilder>) -> Option<Vec<ToolCall>> {
    let calls: Vec<ToolCall> = builders.into_iter().filter_map(ToolCallBuilder::finalize).collect();

    (!calls.is_empty()).then_some(calls)
}

/// Helper to aggregate streaming events and produce a final LLMResponse.
pub(crate) struct StreamAggregator {
    model: String,
    pub(crate) content: String,
    pub(crate) reasoning: String,
    reasoning_details: Vec<String>,
    reasoning_buffer: ReasoningBuffer,
    pub(crate) tool_builders: Vec<ToolCallBuilder>,
    pub(crate) usage: Option<crate::provider::Usage>,
    finish_reason: crate::provider::FinishReason,
    pub(crate) sanitizer: TagStreamSanitizer,
    pub(crate) compaction: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenAiDeltaOrder {
    ReasoningFirst,
    ContentFirst,
}

fn emit_reasoning_delta(
    aggregator: &mut StreamAggregator,
    tx: &tokio::sync::mpsc::UnboundedSender<Result<LLMStreamEvent, LLMError>>,
    delta: &Value,
    reasoning_fields: &[&'static str],
) {
    // Pick the first non-empty reasoning field, allowing providers to declare a
    // fallback order (e.g. `["reasoning", "reasoning_content"]`).
    // Empty strings are skipped so that a present-but-empty primary field
    // does not block fallback to the next candidate.
    let Some(reasoning) = reasoning_fields
        .iter()
        .find_map(|field| delta.get(*field).and_then(Value::as_str).filter(|s| !s.is_empty()))
    else {
        return;
    };
    let Some(delta) = aggregator.handle_reasoning(reasoning) else {
        return;
    };
    let _ = tx.send(Ok(LLMStreamEvent::Reasoning { delta }));
}

fn emit_content_delta(
    aggregator: &mut StreamAggregator,
    tx: &tokio::sync::mpsc::UnboundedSender<Result<LLMStreamEvent, LLMError>>,
    delta: &Value,
) {
    let Some(content) = delta.get("content").and_then(Value::as_str) else {
        return;
    };
    for event in aggregator.handle_content(content) {
        let _ = tx.send(Ok(event));
    }
}

pub(crate) fn handle_openai_compatible_chunk(
    value: &Value,
    aggregator: &mut StreamAggregator,
    tx: &tokio::sync::mpsc::UnboundedSender<Result<LLMStreamEvent, LLMError>>,
    reasoning_fields: &[&'static str],
    delta_order: OpenAiDeltaOrder,
    include_cache_metrics: bool,
) {
    if let Some(choices) = value.get("choices").and_then(Value::as_array)
        && let Some(choice) = choices.first()
    {
        if let Some(delta) = choice.get("delta") {
            match delta_order {
                OpenAiDeltaOrder::ReasoningFirst => {
                    emit_reasoning_delta(aggregator, tx, delta, reasoning_fields);
                    emit_content_delta(aggregator, tx, delta);
                }
                OpenAiDeltaOrder::ContentFirst => {
                    emit_content_delta(aggregator, tx, delta);
                    emit_reasoning_delta(aggregator, tx, delta, reasoning_fields);
                }
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                aggregator.handle_tool_calls(tool_calls);
            }
        }

        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            aggregator.set_finish_reason(map_finish_reason_common(reason));
        }
    }

    if let Some(_usage_value) = value.get("usage")
        && let Some(usage) = crate::providers::common::parse_usage_openai_format(value, include_cache_metrics)
    {
        aggregator.set_usage(usage);
    }
}

impl StreamAggregator {
    pub(crate) fn new(model: String) -> Self {
        Self {
            model,
            content: String::new(),
            reasoning: String::new(),
            reasoning_details: Vec::new(),
            reasoning_buffer: ReasoningBuffer::default(),
            tool_builders: Vec::new(),
            usage: None,
            finish_reason: crate::provider::FinishReason::Stop,
            sanitizer: TagStreamSanitizer::new(),
            compaction: None,
        }
    }

    /// Process a content delta, applying sanitization for reasoning tags.
    pub(crate) fn handle_content(&mut self, delta: &str) -> Vec<LLMStreamEvent> {
        self.content.push_str(delta);
        self.sanitizer.process_chunk(delta)
    }

    /// Process a reasoning delta from a dedicated field.
    pub(crate) fn handle_reasoning(&mut self, delta: &str) -> Option<String> {
        let result = self.reasoning_buffer.push(delta);
        if let Some(ref d) = result {
            self.reasoning.push_str(d);
        }
        result
    }

    /// Store structured reasoning details received from streaming deltas.
    pub(crate) fn set_reasoning_details(&mut self, details: &[Value]) {
        if details.is_empty() {
            return;
        }

        self.reasoning_details = details
            .iter()
            .map(|detail| detail.as_str().map(ToOwned::to_owned).unwrap_or_else(|| detail.to_string()))
            .collect();
    }

    /// Append a completed structured item received from a streaming
    /// output-item event.
    pub(crate) fn append_reasoning_detail(&mut self, detail: &Value) {
        let serialized = detail.as_str().map(ToOwned::to_owned).unwrap_or_else(|| detail.to_string());
        if !self.reasoning_details.iter().any(|existing| existing == &serialized) {
            self.reasoning_details.push(serialized);
        }
    }

    /// Process tool call deltas.
    pub(crate) fn handle_tool_calls(&mut self, deltas: &[Value]) {
        update_tool_calls(&mut self.tool_builders, deltas);
    }

    /// Set usage metrics.
    pub(crate) fn set_usage(&mut self, usage: crate::provider::Usage) {
        self.usage = Some(usage);
    }

    /// Set finish reason.
    pub(crate) fn set_finish_reason(&mut self, reason: crate::provider::FinishReason) {
        self.finish_reason = reason;
    }

    /// Finalize and produce the completed LLMResponse.
    pub(crate) fn finalize(mut self) -> LLMResponse {
        // Collect any leftover bits from sanitizer
        for event in self.sanitizer.finalize() {
            match event {
                LLMStreamEvent::Token { delta } => {
                    self.content.push_str(&delta);
                }
                LLMStreamEvent::Reasoning { delta } => {
                    self.reasoning.push_str(&delta);
                }
                _ => {}
            }
        }

        let reasoning_details = if self.reasoning_details.is_empty() {
            None
        } else {
            Some(self.reasoning_details)
        };
        let mut reasoning = if self.reasoning.is_empty() {
            self.reasoning_buffer.finalize()
        } else {
            Some(self.reasoning)
        };
        if reasoning.is_none() {
            reasoning = reasoning_details
                .as_ref()
                .and_then(|details| extract_reasoning_text_from_serialized_details(details));
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
            reasoning,
            reasoning_details,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
            compaction: self.compaction,
        }
    }
}

/// Common helper for processing OpenAI-compatible SSE streams.
///
/// This simplifies stream implementations across providers like DeepSeek, ZAI, Moonshot, etc.
/// Especially optimized for high-performance models like Gemini 3 and GLM-5.
pub(crate) async fn process_openai_stream<S, E, F>(
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
    use crate::providers::error_handling::format_network_error;
    use futures::StreamExt;

    let mut buf: Vec<u8> = Vec::new();
    let mut offset = 0usize;
    let mut decoder = Utf8StreamDecoder::new();
    let mut last_response_value = None;

    while let Some(chunk_result) = byte_stream.next().await {
        let chunk_bytes = chunk_result.map_err(|e| format_network_error(provider_name, &e.to_string()))?;
        decoder.push_bytes(&chunk_bytes, &mut buf);

        while let Some((boundary_idx, boundary_len)) = find_sse_boundary_bytes(&buf, offset) {
            let event = std::str::from_utf8(&buf[offset..boundary_idx]).expect("valid utf-8 stream data");
            offset = boundary_idx + boundary_len;

            if let Some(data) = extract_data_payload(event) {
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

        // Drain the consumed prefix, then fail closed when the retained tail
        // tops the bound: a peer that never emits an event boundary must not
        // grow memory without limit. Measured after the drain so one
        // large-but-complete event still parses.
        drain_and_bound_sse_tail(&mut buf, &mut offset).map_err(|overflow| LLMError::Provider {
            message: error_display::format_llm_error(provider_name, &overflow.to_string()),
            metadata: None,
        })?;
    }

    // Attempt to extract final response metadata (usage, etc) from last chunk if not already done
    let mut final_response = LLMResponse {
        content: None,
        tool_calls: None,
        model,
        usage: None,
        finish_reason: crate::provider::FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
        compaction: None,
    };

    if let Some(value) = last_response_value
        && value.get("usage").is_some()
    {
        final_response.usage = crate::providers::common::parse_usage_openai_format(&value, true);
    }

    Ok(final_response)
}

pub(crate) fn parse_openai_tool_calls(calls: &[Value]) -> Vec<ToolCall> {
    calls
        .iter()
        .filter_map(|call| {
            let id = call.get("id").and_then(|v| v.as_str())?;
            let function = call.get("function")?;
            let namespace = call
                .get("namespace")
                .and_then(|v| v.as_str())
                .or_else(|| function.get("namespace").and_then(|v| v.as_str()))
                .map(ToOwned::to_owned);
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
            Some(ToolCall::function_with_namespace(id.to_string(), namespace, name.to_string(), serialized))
        })
        .collect()
}

fn push_unique_tool_reference(tool_references: &mut Vec<String>, tool_name: &str) {
    if !tool_references.iter().any(|existing| existing == tool_name) {
        tool_references.push(tool_name.to_string());
    }
}

pub(crate) fn collect_tool_references_from_tool_search_output(value: &Value, tool_references: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_tool_references_from_tool_search_output(item, tool_references);
            }
        }
        Value::Object(object) => {
            if let Some(tools) = object.get("tools").and_then(Value::as_array) {
                for tool in tools {
                    collect_tool_references_from_tool_search_output(tool, tool_references);
                }
            } else if let Some(tool_name) = object.get("tool_name").and_then(Value::as_str) {
                push_unique_tool_reference(tool_references, tool_name);
            } else if let Some(function) = object.get("function").and_then(Value::as_object)
                && let Some(tool_name) = function.get("name").and_then(Value::as_str)
            {
                push_unique_tool_reference(tool_references, tool_name);
            } else if let Some(tool_name) = object.get("name").and_then(Value::as_str) {
                push_unique_tool_reference(tool_references, tool_name);
            }

            if let Some(tool_refs) = object.get("tool_references").and_then(Value::as_array) {
                for tool_ref in tool_refs {
                    collect_tool_references_from_tool_search_output(tool_ref, tool_references);
                }
            }
        }
        _ => {}
    }
}

fn append_text_with_reasoning(
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

fn apply_tool_call_delta_from_content(
    builders: &mut Vec<ToolCallBuilder>,
    container: &Map<String, Value>,
    telemetry: &impl StreamTelemetry,
) {
    apply_tool_call_delta_with_index(builders, container, telemetry, None, None);
}

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

    let index = explicit_index.map(|value| value as usize).or(fallback_index).unwrap_or(0);

    let current_id = extract_tool_call_id(container).or_else(|| fallback_id.clone());

    if let Some(nested) = container.get("delta").and_then(|value| value.as_object()) {
        apply_tool_call_delta_with_index(builders, nested, telemetry, Some(index), current_id.clone());
    }

    let delta_source = container
        .get("tool_call")
        .and_then(|value| value.as_object())
        .unwrap_or(container);

    let mut delta_map = Map::new();

    if let Some(id_value) = extract_tool_call_id(delta_source).or_else(|| current_id.clone()) {
        delta_map.insert("id".to_string(), id_value);
    }

    if let Some(function_value) = delta_source.get("function").or_else(|| container.get("function")) {
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
mod tests;
