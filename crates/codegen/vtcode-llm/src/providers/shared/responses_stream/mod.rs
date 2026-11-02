use crate::error_display;
use crate::provider::{LLMError, LLMNormalizedStream, LLMResponse, NormalizedStreamEvent};
use crate::providers::shared::responses_adapter::{ResponsesStreamAdapter, ResponsesStreamEvent};
/// Usage-block alias chains accepted across Responses-style payloads.
pub(crate) const RESPONSES_PROMPT_TOKEN_KEYS: &[&str] = &["input_tokens", "prompt_tokens"];
pub(crate) const RESPONSES_COMPLETION_TOKEN_KEYS: &[&str] = &["output_tokens", "completion_tokens"];

use crate::providers::shared::{Utf8StreamDecoder, extract_data_payload, find_sse_boundary_bytes, usage_u32_from_keys};
use async_stream::try_stream;
use futures::StreamExt;
use hashbrown::{HashMap, HashSet};
use serde_json::{Value, json};

use super::{
    StreamAggregator, ToolCallIndexes, generate_tool_call_id, parse_cache_write_tokens_from_usage,
    parse_cached_prompt_tokens_from_usage,
};

// Retained shared Responses stream processor.
// Rig 0.40 can consume SSE, but VTCode needs a provider-agnostic
// NormalizedStreamEvent contract: text/refusal/reasoning deltas, tool-call
// start and argument deltas, tolerant empty-final-response recovery, and
// backend error text. Protected by this module's `responses_stream` tests.
// Remove only when Rig exposes an event adapter with the same normalised
// surface for all VTCode providers that use Responses-style streaming.
pub struct ResponsesNormalizedStreamOptions {
    pub(crate) provider_name: &'static str,
    pub(crate) model: String,
    pub(crate) emit_reasoning: bool,
    pub(crate) include_cached_prompt_metrics: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponsesStreamEventPolicy {
    MeaningfulConversion,
    DocumentedStatusMarkerNoop,
    DocumentedValueBearingRigGap,
    Unsupported,
}

pub(crate) fn response_stream_event_policy(payload: &Value) -> Result<ResponsesStreamEventPolicy, &'static str> {
    let Some(event_type) = payload.get("type") else {
        return Err("missing Responses stream event type");
    };
    let Some(event_type) = event_type.as_str() else {
        return Err("Responses stream event type must be a string");
    };

    Ok(response_stream_event_policy_for_type(event_type))
}

fn response_stream_event_policy_for_type(event_type: &str) -> ResponsesStreamEventPolicy {
    match event_type {
        "response.output_text.delta"
        | "response.refusal.delta"
        | "response.reasoning_text.delta"
        | "response.reasoning_summary_text.delta"
        | "response.reasoning_content.delta"
        | "response.reasoning_text.done"
        | "response.output_item.added"
        | "response.output_item.done"
        | "response.function_call_arguments.delta"
        | "response.completed"
        | "response.failed"
        | "response.incomplete"
        | "error" => ResponsesStreamEventPolicy::MeaningfulConversion,
        "response.created"
        | "response.in_progress"
        | "keepalive"
        | "response.queued"
        | "response.content_part.added"
        | "response.content_part.done"
        | "response.output_text.done"
        | "response.refusal.done"
        | "response.function_call_arguments.done"
        | "response.reasoning_summary_part.added"
        | "response.reasoning_summary_part.done"
        | "response.reasoning_summary_text.done"
        | "response.reasoning_part.added"
        | "response.reasoning_part.done"
        | "response.file_search_call.in_progress"
        | "response.file_search_call.searching"
        | "response.file_search_call.completed"
        | "response.web_search_call.in_progress"
        | "response.web_search_call.searching"
        | "response.web_search_call.completed"
        | "response.image_generation_call.in_progress"
        | "response.image_generation_call.generating"
        | "response.image_generation_call.completed"
        | "response.mcp_call.in_progress"
        | "response.mcp_call.completed"
        | "response.mcp_list_tools.in_progress"
        | "response.mcp_list_tools.completed"
        | "response.code_interpreter_call.in_progress"
        | "response.code_interpreter_call.interpreting"
        | "response.code_interpreter_call.completed" => ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop,
        "response.code_interpreter_call_code.delta"
        | "response.code_interpreter_call_code.done"
        | "response.mcp_call_arguments.delta"
        | "response.mcp_call_arguments.done"
        | "response.image_generation_call.partial_image"
        | "response.custom_tool_call_input.delta"
        | "response.custom_tool_call_input.done"
        | "response.output_text.annotation.added" => ResponsesStreamEventPolicy::DocumentedValueBearingRigGap,
        _ => ResponsesStreamEventPolicy::Unsupported,
    }
}

pub(crate) struct ResponsesNormalizedStreamProcessor<P> {
    options: ResponsesNormalizedStreamOptions,
    parse_final_response: P,
    aggregator: StreamAggregator,
    seen_tool_calls: HashSet<String>,
    tool_call_indexes: ToolCallIndexes,
    tool_call_names: HashMap<String, String>,
    tool_call_ids_by_item_id: HashMap<String, String>,
    fabricated_ids: HashMap<usize, String>,
    fabricated_fallback_id: Option<String>,
    final_response: Option<Value>,
    done: bool,
}

impl<P> ResponsesNormalizedStreamProcessor<P>
where
    P: Fn(Value) -> Result<LLMResponse, LLMError>,
{
    pub(crate) fn new(options: ResponsesNormalizedStreamOptions, parse_final_response: P) -> Self {
        Self {
            aggregator: StreamAggregator::new(options.model.clone()),
            options,
            parse_final_response,
            seen_tool_calls: HashSet::new(),
            tool_call_indexes: ToolCallIndexes::default(),
            tool_call_names: HashMap::new(),
            tool_call_ids_by_item_id: HashMap::new(),
            fabricated_ids: HashMap::new(),
            fabricated_fallback_id: None,
            final_response: None,
            done: false,
        }
    }

    /// Fabricate an id once per logical call and reuse it across deltas.
    /// Recomputing a fallback per delta fragments one id-less call into
    /// several builders, and index-based ids collide across responses.
    fn fabricated_call_id(&mut self, output_index: Option<usize>) -> String {
        match output_index {
            Some(index) => self.fabricated_ids.entry(index).or_insert_with(generate_tool_call_id).clone(),
            None => self.fabricated_fallback_id.get_or_insert_with(generate_tool_call_id).clone(),
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.done
    }

    #[cfg(test)]
    fn handle_payload(&mut self, payload: Value) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let serialized = serde_json::to_string(&payload)
            .map_err(|err| provider_error(self.options.provider_name, format!("invalid stream payload: {err}")))?;
        self.handle_payload_data(&serialized)
    }

    pub(crate) fn handle_payload_data(&mut self, payload: &str) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let event = ResponsesStreamAdapter::parse_sse_data_for_provider(self.options.provider_name, payload)?;
        self.handle_event(event)
    }

    fn handle_event(&mut self, event: ResponsesStreamEvent) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let mut events = Vec::new();

        match event {
            ResponsesStreamEvent::TextDelta { delta } => {
                for event in self.aggregator.handle_content(&delta) {
                    match event {
                        crate::provider::LLMStreamEvent::Token { delta } => {
                            events.push(NormalizedStreamEvent::TextDelta { delta });
                        }
                        crate::provider::LLMStreamEvent::Reasoning { delta } if self.options.emit_reasoning => {
                            events.push(NormalizedStreamEvent::ReasoningDelta {
                                delta,
                                source: crate::provider::ReasoningSource::Unknown,
                            });
                        }
                        _ => {}
                    }
                }
            }
            ResponsesStreamEvent::RefusalDelta { delta } => {
                if !delta.is_empty() {
                    self.aggregator.content.push_str(&delta);
                    events.push(NormalizedStreamEvent::TextDelta { delta });
                }
            }
            ResponsesStreamEvent::ReasoningDelta { delta, source } => {
                // Always retain provider reasoning before applying the public
                // emission policy. Continuation details may be required on a
                // later request even when this route must not render them.
                if let Some(delta) = self.aggregator.handle_reasoning(&delta) {
                    if self.options.emit_reasoning {
                        events.push(NormalizedStreamEvent::ReasoningDelta { delta, source });
                    }
                }
            }
            ResponsesStreamEvent::FunctionCallNameDelta { call_id, item_id, name, output_index } => {
                self.record_tool_call_item_id(item_id.as_deref(), &call_id);
                self.record_tool_call_name(&call_id, &name, output_index)?;
                self.push_tool_call_start(&mut events, call_id, Some(name));
            }
            ResponsesStreamEvent::FunctionCallArgumentsDelta { call_id, item_id, delta, output_index } => {
                let call_id = self.provider_tool_call_id(item_id.as_deref(), call_id);
                let call_id = if call_id.is_empty() {
                    self.fabricated_call_id(output_index)
                } else {
                    call_id
                };
                let index = self
                    .tool_call_indexes
                    .resolve(&call_id, output_index, self.options.provider_name)?;

                let name = self.tool_call_names.get(&call_id).cloned();
                self.push_tool_call_start(&mut events, call_id.clone(), name);

                if !delta.is_empty() {
                    self.aggregator.handle_tool_calls(&[json!({
                        "index": index,
                        "id": call_id,
                        "function": {
                            "arguments": delta,
                        }
                    })]);
                    events.push(NormalizedStreamEvent::ToolCallDelta { call_id, delta });
                }
            }
            ResponsesStreamEvent::CompletedToolCall { call_id, item_id, name, arguments, output_index } => {
                self.record_tool_call_item_id(item_id.as_deref(), &call_id);
                let index = self.record_tool_call_name(&call_id, &name, output_index)?;
                self.push_tool_call_start(&mut events, call_id.clone(), Some(name));
                self.aggregator.handle_tool_calls(&[json!({
                    "index": index,
                    "id": call_id,
                    "function": {
                        "arguments": arguments,
                    }
                })]);
            }
            ResponsesStreamEvent::CompletedResponse { response } => {
                self.final_response = Some(response);
                self.done = true;
            }
            ResponsesStreamEvent::Error { message } => {
                return Err(provider_error(self.options.provider_name, message));
            }
            ResponsesStreamEvent::ProviderValueBearingRigGap { event_type, payload, .. } => {
                // `added` compaction items can contain an incomplete opaque
                // state. Replay only the completed snapshot so a partial item
                // cannot replace or duplicate the continuation token.
                if event_type == "response.output_item.done"
                    && payload.get("item").and_then(|item| item.get("type")).and_then(Value::as_str)
                        == Some("compaction")
                    && let Some(item) = payload.get("item")
                {
                    self.aggregator.append_reasoning_detail(item);
                }
            }
            ResponsesStreamEvent::Lifecycle { .. } | ResponsesStreamEvent::Unknown => {}
        }

        Ok(events)
    }

    fn record_tool_call_name(
        &mut self,
        call_id: &str,
        name: &str,
        output_index: Option<usize>,
    ) -> Result<usize, LLMError> {
        self.tool_call_names
            .entry(call_id.to_string())
            .or_insert_with(|| name.to_string());
        let index = self
            .tool_call_indexes
            .resolve(call_id, output_index, self.options.provider_name)?;
        self.aggregator.handle_tool_calls(&[json!({
            "index": index,
            "id": call_id,
            "function": {
                "name": name,
            }
        })]);
        Ok(index)
    }

    pub(crate) fn finish(self) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let streamed = self.aggregator.finalize();
        let mut response = if let Some(final_response) = self.final_response {
            match (self.parse_final_response)(final_response.clone()) {
                Ok(response) => response,
                Err(_) if final_response_output_is_empty(&final_response) && streamed_response_is_usable(&streamed) => {
                    let mut response = streamed.clone();
                    merge_final_response_metadata(
                        &mut response,
                        &final_response,
                        self.options.include_cached_prompt_metrics,
                    );
                    response
                }
                Err(err) => return Err(err),
            }
        } else {
            streamed.clone()
        };

        merge_streamed_response(&mut response, streamed);

        let mut events = Vec::new();
        if let Some(usage) = response.usage.clone() {
            events.push(NormalizedStreamEvent::Usage { usage });
        }
        events.push(NormalizedStreamEvent::Done { response: Box::new(response) });
        Ok(events)
    }

    fn record_tool_call_item_id(&mut self, item_id: Option<&str>, call_id: &str) {
        let Some(item_id) = item_id.filter(|item_id| !item_id.is_empty()) else {
            return;
        };

        self.tool_call_ids_by_item_id
            .entry(item_id.to_string())
            .or_insert_with(|| call_id.to_string());
    }

    fn provider_tool_call_id(&self, item_id: Option<&str>, call_id: String) -> String {
        item_id
            .and_then(|item_id| self.tool_call_ids_by_item_id.get(item_id))
            .or_else(|| self.tool_call_ids_by_item_id.get(call_id.as_str()))
            .cloned()
            .unwrap_or(call_id)
    }

    fn push_tool_call_start(&mut self, events: &mut Vec<NormalizedStreamEvent>, call_id: String, name: Option<String>) {
        if self.seen_tool_calls.insert(call_id.clone()) {
            events.push(NormalizedStreamEvent::ToolCallStart { call_id, name });
        }
    }
}

pub fn create_responses_normalized_stream<P>(
    response: reqwest::Response,
    options: ResponsesNormalizedStreamOptions,
    parse_final_response: P,
) -> LLMNormalizedStream
where
    P: Fn(Value) -> Result<LLMResponse, LLMError> + Send + 'static,
{
    let stream = try_stream! {
        let provider_name = options.provider_name;
        let mut processor = ResponsesNormalizedStreamProcessor::new(options, parse_final_response);
        let mut body_stream = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut offset = 0usize;
        let mut decoder = Utf8StreamDecoder::new();

        while let Some(chunk_result) = body_stream.next().await {
            let chunk = chunk_result.map_err(|err| provider_error(
                provider_name,
                format!("streaming error: {err}"),
            ))?;
            decoder.push_bytes(&chunk, &mut buf);

            while let Some((split_idx, delimiter_len)) = find_sse_boundary_bytes(&buf, offset) {
                let event = std::str::from_utf8(&buf[offset..split_idx]).expect("valid utf-8 stream data");
                offset = split_idx + delimiter_len;

                if let Some(data_payload) = extract_data_payload(event) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload.is_empty() || trimmed_payload == "[DONE]" {
                        continue;
                    }

                    for event in processor.handle_payload_data(trimmed_payload)? {
                        yield event;
                    }

                    if processor.is_done() {
                        break;
                    }
                }
            }

            // Drain the consumed prefix so `buf` stays bounded to the
            // unprocessed tail rather than growing for the entire stream.
            if offset > 0 {
                buf.drain(..offset);
                offset = 0;
            }

            if processor.is_done() {
                break;
            }
        }

        for event in processor.finish()? {
            yield event;
        }
    };

    Box::pin(stream)
}

fn streamed_response_is_usable(response: &LLMResponse) -> bool {
    response.content.as_deref().is_some_and(|content| !content.is_empty())
        || response.tool_calls.as_ref().is_some_and(|tool_calls| !tool_calls.is_empty())
        || response.reasoning.as_deref().is_some_and(|reasoning| !reasoning.is_empty())
        || response.reasoning_details.as_ref().is_some_and(|details| !details.is_empty())
}

fn final_response_output_is_empty(final_response: &Value) -> bool {
    final_response
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn merge_streamed_response(response: &mut LLMResponse, streamed: LLMResponse) {
    if response.content.as_deref().unwrap_or_default().is_empty() {
        response.content = streamed.content;
    } else if let (Some(content), Some(streamed_content)) = (&mut response.content, streamed.content)
        && !streamed_content.is_empty()
        && !content.contains(&streamed_content)
    {
        content.push_str(&streamed_content);
    }

    if response.tool_calls.is_none() {
        response.tool_calls = streamed.tool_calls;
    }

    if response.usage.is_none() {
        response.usage = streamed.usage;
    }

    if response.reasoning.is_none() {
        response.reasoning = streamed.reasoning;
    }

    if response.reasoning_details.is_none() {
        response.reasoning_details = streamed.reasoning_details;
    }

    if response.tool_references.is_empty() && !streamed.tool_references.is_empty() {
        response.tool_references = streamed.tool_references;
    }

    if response.request_id.is_none() {
        response.request_id = streamed.request_id;
    }

    if response.organization_id.is_none() {
        response.organization_id = streamed.organization_id;
    }
}

fn merge_final_response_metadata(
    response: &mut LLMResponse,
    final_response: &Value,
    include_cached_prompt_metrics: bool,
) {
    if let Some(usage) = parse_responses_usage(final_response, include_cached_prompt_metrics) {
        response.usage = Some(usage);
    }

    if let Some(request_id) = final_response
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| final_response.get("request_id").and_then(Value::as_str))
    {
        response.request_id = Some(request_id.to_string());
    }
}

fn parse_responses_usage(
    final_response: &Value,
    include_cached_prompt_metrics: bool,
) -> Option<crate::provider::Usage> {
    let usage_value = final_response.get("usage")?;
    let cached_prompt_tokens = parse_cached_prompt_tokens_from_usage(usage_value, include_cached_prompt_metrics);
    Some(crate::provider::Usage {
        prompt_tokens: usage_u32_from_keys(usage_value, RESPONSES_PROMPT_TOKEN_KEYS),
        completion_tokens: usage_u32_from_keys(usage_value, RESPONSES_COMPLETION_TOKEN_KEYS),
        total_tokens: usage_u32_from_keys(usage_value, &["total_tokens"]),
        cached_prompt_tokens,
        cache_creation_tokens: parse_cache_write_tokens_from_usage(usage_value, include_cached_prompt_metrics),
        cache_read_tokens: None,
        iterations: None,
    })
}

fn provider_error(provider_name: &str, message: impl Into<String>) -> LLMError {
    let message = error_display::format_llm_error(provider_name, &message.into());
    LLMError::Provider { message, metadata: None }
}

#[cfg(test)]
mod tests;
