use crate::error_display;
use crate::provider::{LLMError, LLMNormalizedStream, LLMResponse, NormalizedStreamEvent};
use crate::providers::shared::{extract_data_payload, find_sse_boundary};
use async_stream::try_stream;
use futures::StreamExt;
use hashbrown::{HashMap, HashSet};
use serde_json::{Value, json};

use super::{StreamAggregator, parse_cached_prompt_tokens_from_usage};

// Retained shared Responses stream processor.
// Rig 0.38.2 can consume SSE, but VTCode needs a provider-agnostic
// NormalizedStreamEvent contract: text/refusal/reasoning deltas, tool-call
// start and argument deltas, tolerant empty-final-response recovery, and
// backend error text. Protected by this module's `responses_stream` tests.
// Remove only when Rig exposes an event adapter with the same normalised
// surface for all VTCode providers that use Responses-style streaming.
pub struct ResponsesNormalizedStreamOptions {
    pub provider_name: &'static str,
    pub model: String,
    pub emit_reasoning: bool,
    pub include_cached_prompt_metrics: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResponsesStreamEventPolicy {
    MeaningfulConversion,
    DocumentedStatusMarkerNoop,
    DocumentedValueBearingRigGap,
    Unsupported,
}

struct ResponsesNormalizedStreamProcessor<P> {
    options: ResponsesNormalizedStreamOptions,
    parse_final_response: P,
    aggregator: StreamAggregator,
    seen_tool_calls: HashSet<String>,
    tool_call_indexes: HashMap<String, usize>,
    tool_call_names: HashMap<String, String>,
    item_id_to_call_id: HashMap<String, String>,
    next_tool_call_index: usize,
    final_response: Option<Value>,
    done: bool,
}

impl<P> ResponsesNormalizedStreamProcessor<P>
where
    P: Fn(Value) -> Result<LLMResponse, LLMError>,
{
    fn new(options: ResponsesNormalizedStreamOptions, parse_final_response: P) -> Self {
        Self {
            aggregator: StreamAggregator::new(options.model.clone()),
            options,
            parse_final_response,
            seen_tool_calls: HashSet::new(),
            tool_call_indexes: HashMap::new(),
            tool_call_names: HashMap::new(),
            item_id_to_call_id: HashMap::new(),
            next_tool_call_index: 0,
            final_response: None,
            done: false,
        }
    }

    fn is_done(&self) -> bool {
        self.done
    }

    fn handle_payload(&mut self, payload: Value) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let mut events = Vec::new();

        if let Some(usage) = payload.get("usage").cloned()
            && let Ok(usage) = serde_json::from_value(usage)
        {
            self.aggregator.set_usage(usage);
        }

        let policy = response_stream_event_policy(&payload)
            .map_err(|message| provider_error(self.options.provider_name, message))?;
        let event_type = payload
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| provider_error(self.options.provider_name, "missing event type"))?;

        match policy {
            ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop
            | ResponsesStreamEventPolicy::DocumentedValueBearingRigGap => {
                // vtcode-llm has no normalised runtime surface for provider-hosted
                // code, provider-side MCP, partial images, streamed custom-tool
                // input, or annotation metadata. Custom-tool execution remains
                // owned by final `response.completed` replay, which preserves
                // the complete custom input without changing dispatch authority.
                return Ok(events);
            }
            ResponsesStreamEventPolicy::Unsupported => {
                return Err(provider_error(
                    self.options.provider_name,
                    format!("unsupported Responses stream event type `{event_type}`"),
                ));
            }
            ResponsesStreamEventPolicy::MeaningfulConversion => {}
        }

        match event_type {
            "response.output_text.delta" => {
                let delta = payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| provider_error(self.options.provider_name, "missing delta"))?;
                for event in self.aggregator.handle_content(delta) {
                    match event {
                        crate::provider::LLMStreamEvent::Token { delta } => {
                            events.push(NormalizedStreamEvent::TextDelta { delta });
                        }
                        crate::provider::LLMStreamEvent::Reasoning { delta }
                            if self.options.emit_reasoning =>
                        {
                            events.push(NormalizedStreamEvent::ReasoningDelta { delta });
                        }
                        _ => {}
                    }
                }
            }
            "response.refusal.delta" => {
                let delta = payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| provider_error(self.options.provider_name, "missing delta"))?;
                if !delta.is_empty() {
                    self.aggregator.content.push_str(delta);
                    events.push(NormalizedStreamEvent::TextDelta {
                        delta: delta.to_string(),
                    });
                }
            }
            "response.reasoning_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_content.delta" => {
                let delta = payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| provider_error(self.options.provider_name, "missing delta"))?;
                if self.options.emit_reasoning
                    && let Some(delta) = self.aggregator.handle_reasoning(delta)
                {
                    events.push(NormalizedStreamEvent::ReasoningDelta { delta });
                }
            }
            "response.reasoning_text.done" => {
                let text = optional_string_field(self.options.provider_name, &payload, "text")?;
                let delta = optional_string_field(self.options.provider_name, &payload, "delta")?;
                if self.options.emit_reasoning
                    && let Some(text) = text.or(delta)
                    && let Some(delta) = self.aggregator.handle_reasoning(&text)
                {
                    events.push(NormalizedStreamEvent::ReasoningDelta { delta });
                }
            }
            "response.output_item.added" | "response.output_item.done" => {
                if let Some(item) = payload.get("item") {
                    let tool_call = self.capture_tool_call_metadata(
                        item,
                        payload
                            .get("output_index")
                            .and_then(Value::as_u64)
                            .map(|value| value as usize),
                    );
                    if let Some((call_id, name)) = tool_call {
                        self.push_tool_call_start(&mut events, call_id, Some(name));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let delta = payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| provider_error(self.options.provider_name, "missing delta"))?;
                let item_id = payload.get("item_id").and_then(Value::as_str);
                let payload_call_id = payload.get("call_id").and_then(Value::as_str);
                self.capture_item_call_id_mapping(item_id, payload_call_id);
                let call_id = self
                    .resolve_provider_call_id(item_id, payload_call_id)
                    .unwrap_or_else(|| format!("tool_call_{}", self.next_tool_call_index));
                let index = self.resolve_tool_call_index(
                    &call_id,
                    payload
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .map(|value| value as usize),
                );

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
                    events.push(NormalizedStreamEvent::ToolCallDelta {
                        call_id,
                        delta: delta.to_string(),
                    });
                }
            }
            "response.completed" => {
                if let Some(response) = payload.get("response") {
                    self.final_response = Some(response.clone());
                }
                self.done = true;
            }
            "response.failed" | "response.incomplete" | "error" => {
                let message = extract_error_message(&payload)
                    .unwrap_or_else(|| "unknown error from responses stream".to_string());
                return Err(provider_error(self.options.provider_name, message));
            }
            _ => {
                return Err(provider_error(
                    self.options.provider_name,
                    format!("unhandled Responses stream event type `{event_type}`"),
                ));
            }
        }

        Ok(events)
    }

    fn finish(self) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let streamed = self.aggregator.finalize();
        let mut response = if let Some(final_response) = self.final_response {
            match (self.parse_final_response)(final_response.clone()) {
                Ok(response) => response,
                Err(_)
                    if final_response_output_is_empty(&final_response)
                        && streamed_response_is_usable(&streamed) =>
                {
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
        events.push(NormalizedStreamEvent::Done {
            response: Box::new(response),
        });
        Ok(events)
    }

    fn capture_tool_call_metadata(
        &mut self,
        item: &Value,
        output_index: Option<usize>,
    ) -> Option<(String, String)> {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        if item_type != "function_call" {
            return None;
        }

        let item_id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let provider_call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let call_id = provider_call_id.or(item_id);
        let name = item.get("name").and_then(Value::as_str).or_else(|| {
            item.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
        });
        if let (Some(call_id), Some(name)) = (call_id, name) {
            self.capture_item_call_id_mapping(item_id, provider_call_id);
            self.tool_call_names
                .entry(call_id.to_string())
                .or_insert_with(|| name.to_string());
            let output_index = output_index.or_else(|| {
                item_id.and_then(|item_id| self.tool_call_indexes.get(item_id).copied())
            });
            let index = self.resolve_tool_call_index(call_id, output_index);
            self.aggregator.handle_tool_calls(&[json!({
                "index": index,
                "id": call_id,
                "function": {
                    "name": name,
                }
            })]);
            return Some((call_id.to_string(), name.to_string()));
        }

        None
    }

    fn capture_item_call_id_mapping(&mut self, item_id: Option<&str>, call_id: Option<&str>) {
        let Some(item_id) = item_id.filter(|value| !value.is_empty()) else {
            return;
        };
        let Some(call_id) = call_id.filter(|value| !value.is_empty()) else {
            return;
        };
        self.item_id_to_call_id
            .insert(item_id.to_string(), call_id.to_string());
    }

    fn resolve_provider_call_id(
        &self,
        item_id: Option<&str>,
        call_id: Option<&str>,
    ) -> Option<String> {
        call_id
            .filter(|value| !value.is_empty())
            .or_else(|| {
                item_id.and_then(|value| self.item_id_to_call_id.get(value).map(String::as_str))
            })
            .or_else(|| item_id.filter(|value| !value.is_empty()))
            .map(ToOwned::to_owned)
    }

    fn push_tool_call_start(
        &mut self,
        events: &mut Vec<NormalizedStreamEvent>,
        call_id: String,
        name: Option<String>,
    ) {
        if self.seen_tool_calls.insert(call_id.clone()) {
            events.push(NormalizedStreamEvent::ToolCallStart { call_id, name });
        }
    }

    fn resolve_tool_call_index(&mut self, call_id: &str, output_index: Option<usize>) -> usize {
        if let Some(index) = output_index {
            self.tool_call_indexes.insert(call_id.to_string(), index);
            self.next_tool_call_index = self.next_tool_call_index.max(index + 1);
            return index;
        }

        if let Some(index) = self.tool_call_indexes.get(call_id).copied() {
            return index;
        }

        let index = self.next_tool_call_index;
        self.tool_call_indexes.insert(call_id.to_string(), index);
        self.next_tool_call_index += 1;
        index
    }
}

pub(crate) fn response_stream_event_policy(
    payload: &Value,
) -> Result<ResponsesStreamEventPolicy, &'static str> {
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
        | "response.code_interpreter_call.completed" => {
            ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop
        }
        "response.code_interpreter_call_code.delta"
        | "response.code_interpreter_call_code.done"
        | "response.mcp_call_arguments.delta"
        | "response.mcp_call_arguments.done"
        | "response.image_generation_call.partial_image"
        | "response.custom_tool_call_input.delta"
        | "response.custom_tool_call_input.done"
        | "response.output_text.annotation.added" => {
            ResponsesStreamEventPolicy::DocumentedValueBearingRigGap
        }
        _ => ResponsesStreamEventPolicy::Unsupported,
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
        let mut buffer = String::new();

        while let Some(chunk_result) = body_stream.next().await {
            let chunk = chunk_result.map_err(|err| provider_error(
                provider_name,
                format!("streaming error: {err}"),
            ))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some((split_idx, delimiter_len)) = find_sse_boundary(&buffer) {
                let event = buffer[..split_idx].to_string();
                buffer.drain(..split_idx + delimiter_len);

                if let Some(data_payload) = extract_data_payload(&event) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload.is_empty() || trimmed_payload == "[DONE]" {
                        continue;
                    }

                    let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                        provider_error(provider_name, format!("invalid stream payload: {err}"))
                    })?;

                    for event in processor.handle_payload(payload)? {
                        yield event;
                    }

                    if processor.is_done() {
                        break;
                    }
                }
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
    response
        .content
        .as_deref()
        .is_some_and(|content| !content.is_empty())
        || response
            .tool_calls
            .as_ref()
            .is_some_and(|tool_calls| !tool_calls.is_empty())
        || response
            .reasoning
            .as_deref()
            .is_some_and(|reasoning| !reasoning.is_empty())
        || response
            .reasoning_details
            .as_ref()
            .is_some_and(|details| !details.is_empty())
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
    } else if let (Some(content), Some(streamed_content)) =
        (&mut response.content, streamed.content)
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
    let cached_prompt_tokens =
        parse_cached_prompt_tokens_from_usage(usage_value, include_cached_prompt_metrics);
    Some(crate::provider::Usage {
        prompt_tokens: usage_value
            .get("input_tokens")
            .or_else(|| usage_value.get("prompt_tokens"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        completion_tokens: usage_value
            .get("output_tokens")
            .or_else(|| usage_value.get("completion_tokens"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        total_tokens: usage_value
            .get("total_tokens")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        cached_prompt_tokens,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        iterations: None,
    })
}

fn extract_error_message(payload: &Value) -> Option<String> {
    payload
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            payload
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn provider_error(provider_name: &str, message: impl Into<String>) -> LLMError {
    let message = error_display::format_llm_error(provider_name, &message.into());
    LLMError::Provider {
        message,
        metadata: None,
    }
}

fn optional_string_field(
    provider_name: &str,
    payload: &Value,
    field: &'static str,
) -> Result<Option<String>, LLMError> {
    match payload.get(field) {
        Some(value) => value
            .as_str()
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| {
                provider_error(
                    provider_name,
                    format!("field `{field}` in stream payload must be a string"),
                )
            }),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResponsesNormalizedStreamOptions, ResponsesNormalizedStreamProcessor,
        ResponsesStreamEventPolicy, provider_error,
    };
    use crate::provider::{FinishReason, LLMResponse, NormalizedStreamEvent, ToolCall};
    use serde_json::{Value, json};

    fn options() -> ResponsesNormalizedStreamOptions {
        ResponsesNormalizedStreamOptions {
            provider_name: "TestProvider",
            model: "gpt-5".to_string(),
            emit_reasoning: true,
            include_cached_prompt_metrics: false,
        }
    }

    fn parse_response(value: Value) -> Result<LLMResponse, crate::provider::LLMError> {
        let content = value
            .get("output")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("content"))
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        Ok(LLMResponse {
            content,
            model: "gpt-5".to_string(),
            finish_reason: FinishReason::Stop,
            ..Default::default()
        })
    }

    #[test]
    fn text_delta_and_completed_yield_text_then_done() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

        let events = processor
            .handle_payload(json!({
                "type": "response.output_text.delta",
                "delta": "hello"
            }))
            .expect("text delta should parse");
        assert!(matches!(
            events.as_slice(),
            [NormalizedStreamEvent::TextDelta { delta }] if delta == "hello"
        ));

        let completed_events = processor
            .handle_payload(json!({
                "type": "response.completed",
                "response": {
                    "output": [{
                        "type": "message",
                        "content": [{"type": "output_text", "text": "hello"}]
                    }]
                }
            }))
            .expect("completed event should parse");
        assert!(completed_events.is_empty());

        let finished = processor.finish().expect("finish should succeed");
        assert!(matches!(
            finished.as_slice(),
            [NormalizedStreamEvent::Done { response }]
                if response.content.as_deref() == Some("hello")
        ));
    }

    #[test]
    fn empty_final_response_uses_streamed_text_and_preserves_metadata() {
        let mut options = options();
        options.include_cached_prompt_metrics = true;
        let mut processor = ResponsesNormalizedStreamProcessor::new(options, |value| {
            let output = value
                .get("output")
                .and_then(Value::as_array)
                .ok_or_else(|| provider_error("TestProvider", "missing output"))?;
            if output.is_empty() {
                return Err(provider_error("TestProvider", "No output in response"));
            }
            parse_response(value)
        });

        processor
            .handle_payload(json!({
                "type": "response.output_text.delta",
                "delta": "streamed answer"
            }))
            .expect("text delta should parse");
        processor
            .handle_payload(json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_streamed",
                    "output": [],
                    "usage": {
                        "input_tokens": 11,
                        "output_tokens": 7,
                        "total_tokens": 18,
                        "input_tokens_details": {
                            "cached_tokens": 5
                        }
                    }
                }
            }))
            .expect("completed event should parse");

        let finished = processor.finish().expect("finish should succeed");
        let [
            NormalizedStreamEvent::Usage { usage },
            NormalizedStreamEvent::Done { response },
        ] = finished.as_slice()
        else {
            panic!("expected usage then done");
        };
        assert_eq!(usage.prompt_tokens, 11);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.total_tokens, 18);
        assert_eq!(usage.cached_prompt_tokens, Some(5));
        assert_eq!(response.content.as_deref(), Some("streamed answer"));
        assert_eq!(response.request_id.as_deref(), Some("resp_streamed"));
    }

    #[test]
    fn tool_call_deltas_emit_start_and_finish_with_assembled_tool_call() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |_| {
            Ok(LLMResponse {
                model: "gpt-5".to_string(),
                ..Default::default()
            })
        });

        let started = processor
            .handle_payload(json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "function_call",
                    "id": "call_1",
                    "name": "search_workspace"
                }
            }))
            .expect("output item metadata should parse");
        assert!(matches!(
            started.as_slice(),
            [NormalizedStreamEvent::ToolCallStart { call_id, name }]
                if call_id == "call_1" && name.as_deref() == Some("search_workspace")
        ));

        let first = processor
            .handle_payload(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "call_1",
                "delta": "{\"query\":\"vt"
            }))
            .expect("first tool delta should parse");
        assert!(matches!(
            first.as_slice(),
            [NormalizedStreamEvent::ToolCallDelta { call_id: delta_call_id, delta }]
            if delta_call_id == "call_1"
                && delta == "{\"query\":\"vt"
        ));

        let second = processor
            .handle_payload(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "call_1",
                "delta": "code\"}"
            }))
            .expect("second tool delta should parse");
        assert!(matches!(
            second.as_slice(),
            [NormalizedStreamEvent::ToolCallDelta { call_id, delta }]
                if call_id == "call_1" && delta == "code\"}"
        ));

        let finished = processor.finish().expect("finish should succeed");
        let response = match finished.as_slice() {
            [NormalizedStreamEvent::Done { response }] => response,
            _ => panic!("expected done event"),
        };
        let tool_calls = response
            .tool_calls
            .as_ref()
            .expect("tool call should be assembled");
        assert_eq!(
            tool_calls,
            &vec![ToolCall::function(
                "call_1".to_string(),
                "search_workspace".to_string(),
                "{\"query\":\"vtcode\"}".to_string(),
            )]
        );
    }

    #[test]
    fn tool_call_deltas_use_provider_call_id_when_item_id_differs() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |_| {
            Ok(LLMResponse {
                model: "gpt-5".to_string(),
                ..Default::default()
            })
        });

        let started = processor
            .handle_payload(json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "search_workspace"
                }
            }))
            .expect("output item metadata should parse");
        assert!(matches!(
            started.as_slice(),
            [NormalizedStreamEvent::ToolCallStart { call_id, name }]
                if call_id == "call_1" && name.as_deref() == Some("search_workspace")
        ));

        let delta = processor
            .handle_payload(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "delta": "{\"query\":\"vtcode\"}"
            }))
            .expect("tool delta should parse");
        assert!(matches!(
            delta.as_slice(),
            [NormalizedStreamEvent::ToolCallDelta { call_id, delta }]
                if call_id == "call_1" && delta == "{\"query\":\"vtcode\"}"
        ));

        let finished = processor.finish().expect("finish should succeed");
        let response = match finished.as_slice() {
            [NormalizedStreamEvent::Done { response }] => response,
            _ => panic!("expected done event"),
        };
        let tool_calls = response
            .tool_calls
            .as_ref()
            .expect("tool call should be assembled");
        assert_eq!(
            tool_calls,
            &vec![ToolCall::function(
                "call_1".to_string(),
                "search_workspace".to_string(),
                "{\"query\":\"vtcode\"}".to_string(),
            )]
        );
    }

    #[test]
    fn refusal_delta_streams_visible_output() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

        let events = processor
            .handle_payload(json!({
                "type": "response.refusal.delta",
                "delta": "I can't help with that"
            }))
            .expect("refusal delta should parse");
        assert!(matches!(
            events.as_slice(),
            [NormalizedStreamEvent::TextDelta { delta }]
                if delta == "I can't help with that"
        ));

        let finished = processor.finish().expect("finish should succeed");
        assert!(matches!(
            finished.as_slice(),
            [NormalizedStreamEvent::Done { response }]
                if response.content.as_deref() == Some("I can't help with that")
        ));
    }

    #[test]
    fn failed_incomplete_and_error_events_surface_backend_message() {
        for payload in [
            json!({"type": "response.failed", "response": {"error": {"message": "failed"}}}),
            json!({"type": "response.incomplete", "response": {"error": {"message": "incomplete"}}}),
            json!({"type": "error", "error": {"message": "errored"}}),
        ] {
            let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
            let error = processor
                .handle_payload(payload)
                .expect_err("error payload should fail");
            assert!(
                error.to_string().contains("failed")
                    || error.to_string().contains("incomplete")
                    || error.to_string().contains("errored")
            );
        }
    }

    #[test]
    fn stream_event_policy_classifies_documented_families() {
        assert_eq!(
            super::response_stream_event_policy_for_type("response.output_text.delta"),
            ResponsesStreamEventPolicy::MeaningfulConversion
        );
        assert_eq!(
            super::response_stream_event_policy_for_type("response.queued"),
            ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop
        );
        assert_eq!(
            super::response_stream_event_policy_for_type("keepalive"),
            ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop
        );

        for event_type in [
            "response.code_interpreter_call_code.delta",
            "response.code_interpreter_call_code.done",
            "response.mcp_call_arguments.delta",
            "response.mcp_call_arguments.done",
            "response.image_generation_call.partial_image",
            "response.custom_tool_call_input.delta",
            "response.custom_tool_call_input.done",
            "response.output_text.annotation.added",
            "response.reasoning_text.done",
        ] {
            assert_ne!(
                super::response_stream_event_policy_for_type(event_type),
                ResponsesStreamEventPolicy::DocumentedStatusMarkerNoop,
                "{event_type} must not be treated as a status-only no-op"
            );
        }

        assert_eq!(
            super::response_stream_event_policy_for_type(
                "response.code_interpreter_call_code.delta"
            ),
            ResponsesStreamEventPolicy::DocumentedValueBearingRigGap
        );
        assert_eq!(
            super::response_stream_event_policy_for_type("response.not_a_real_event"),
            ResponsesStreamEventPolicy::Unsupported
        );
    }

    #[test]
    fn documented_status_marker_events_are_explicit_noops() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

        for payload in [
            json!({"type": "response.created", "response": {"id": "resp_1"}}),
            json!({"type": "response.in_progress", "response": {"id": "resp_1"}}),
            json!({"type": "response.queued", "response": {"id": "resp_1"}}),
            json!({"type": "response.file_search_call.searching", "item_id": "fs_1", "output_index": 0}),
            json!({"type": "response.web_search_call.completed", "item_id": "ws_1", "output_index": 1}),
            json!({"type": "response.image_generation_call.generating", "item_id": "ig_1", "output_index": 2}),
            json!({"type": "response.mcp_call.completed", "item_id": "mcp_1", "output_index": 3}),
            json!({"type": "response.mcp_list_tools.completed", "item_id": "mcp_tools_1", "output_index": 4}),
            json!({"type": "response.code_interpreter_call.interpreting", "item_id": "ci_1", "output_index": 5}),
            json!({"type": "response.content_part.done", "item_id": "msg_1", "output_index": 6}),
            json!({"type": "response.function_call_arguments.done", "item_id": "fc_1", "output_index": 7}),
        ] {
            let events = processor
                .handle_payload(payload)
                .expect("documented status marker should be accepted");
            assert!(events.is_empty());
        }
    }

    #[test]
    fn documented_value_bearing_rig_gaps_are_explicit_non_runtime_events() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

        for payload in [
            json!({
                "type": "response.code_interpreter_call_code.delta",
                "item_id": "ci_1",
                "output_index": 0,
                "sequence_number": 1,
                "delta": "print(1)"
            }),
            json!({
                "type": "response.code_interpreter_call_code.done",
                "item_id": "ci_1",
                "output_index": 0,
                "sequence_number": 2,
                "code": "print(1)"
            }),
            json!({
                "type": "response.mcp_call_arguments.delta",
                "item_id": "mcp_1",
                "call_id": "call_mcp",
                "output_index": 1,
                "sequence_number": 3,
                "delta": "{\"query\":\"vtcode\"}"
            }),
            json!({
                "type": "response.mcp_call_arguments.done",
                "item_id": "mcp_1",
                "call_id": "call_mcp",
                "output_index": 1,
                "sequence_number": 4,
                "arguments": "{\"query\":\"vtcode\"}"
            }),
            json!({
                "type": "response.image_generation_call.partial_image",
                "item_id": "img_1",
                "output_index": 2,
                "sequence_number": 5,
                "partial_image_b64": "ZmFrZQ=="
            }),
            json!({
                "type": "response.custom_tool_call_input.delta",
                "item_id": "custom_1",
                "call_id": "call_custom",
                "output_index": 3,
                "sequence_number": 6,
                "delta": "partial"
            }),
            json!({
                "type": "response.custom_tool_call_input.done",
                "item_id": "custom_1",
                "call_id": "call_custom",
                "output_index": 3,
                "sequence_number": 7,
                "input": "partial input"
            }),
            json!({
                "type": "response.output_text.annotation.added",
                "item_id": "msg_1",
                "output_index": 4,
                "sequence_number": 8,
                "annotation": {"type": "url_citation", "url": "https://example.test"}
            }),
        ] {
            let events = processor
                .handle_payload(payload)
                .expect("documented value-bearing gap should be accepted");
            assert!(events.is_empty());
        }
    }

    #[test]
    fn custom_tool_stream_input_is_preserved_by_final_response_replay() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |value| {
            crate::providers::openai::responses_api::parse_responses_payload(
                value,
                "gpt-5".to_string(),
                false,
            )
        });

        for payload in [
            json!({
                "type": "response.custom_tool_call_input.delta",
                "item_id": "custom_1",
                "call_id": "call_custom",
                "output_index": 0,
                "delta": "{\"cmd\":"
            }),
            json!({
                "type": "response.custom_tool_call_input.done",
                "item_id": "custom_1",
                "call_id": "call_custom",
                "output_index": 0,
                "input": "{\"cmd\":\"test\"}"
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_custom",
                    "output": [{
                        "type": "custom_tool_call",
                        "id": "custom_1",
                        "call_id": "call_custom",
                        "name": "hosted_shell",
                        "input": "{\"cmd\":\"test\"}"
                    }]
                }
            }),
        ] {
            let events = processor
                .handle_payload(payload)
                .expect("custom tool stream event should parse");
            assert!(events.is_empty());
        }

        let finished = processor.finish().expect("finish should succeed");
        let response = match finished.as_slice() {
            [NormalizedStreamEvent::Done { response }] => response,
            _ => panic!("expected done event"),
        };
        let tool_calls = response
            .tool_calls
            .as_ref()
            .expect("custom tool call should be replayed from final response");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_custom");
        assert!(tool_calls[0].is_custom());
        assert_eq!(tool_calls[0].text.as_deref(), Some("{\"cmd\":\"test\"}"));
    }

    #[test]
    fn reasoning_text_done_is_marker_or_final_reasoning_text() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
        let marker = processor
            .handle_payload(json!({"type": "response.reasoning_text.done"}))
            .expect("marker done should parse");
        assert!(marker.is_empty());

        let final_text = processor
            .handle_payload(json!({
                "type": "response.reasoning_text.done",
                "text": "final reasoning"
            }))
            .expect("final reasoning text should parse");
        assert!(matches!(
            final_text.as_slice(),
            [NormalizedStreamEvent::ReasoningDelta { delta }]
                if delta == "final reasoning"
        ));
    }

    #[test]
    fn unsupported_responses_event_reports_provider_error() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
        let error = processor
            .handle_payload(json!({"type": "response.not_a_real_event"}))
            .expect_err("unsupported event should fail");
        assert!(
            error
                .to_string()
                .contains("unsupported Responses stream event type")
        );
    }

    #[test]
    fn missing_delta_reports_provider_error() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
        let error = processor
            .handle_payload(json!({"type": "response.output_text.delta"}))
            .expect_err("missing delta should fail");
        assert_eq!(
            error.to_string(),
            provider_error("TestProvider", "missing delta").to_string()
        );
    }
}
