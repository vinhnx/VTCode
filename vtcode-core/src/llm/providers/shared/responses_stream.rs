use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMNormalizedStream, LLMResponse, NormalizedStreamEvent};
use crate::llm::providers::openai::responses_adapter::{
    ResponsesStreamAdapter, ResponsesStreamEvent,
};
use crate::llm::providers::shared::{Utf8StreamDecoder, extract_data_payload, find_sse_boundary};
use async_stream::try_stream;
use futures::StreamExt;
use hashbrown::{HashMap, HashSet};
use serde_json::{Value, json};

use super::{StreamAggregator, parse_cached_prompt_tokens_from_usage};

// Retained shared Responses stream processor.
// Rig 0.39 can consume SSE, but VTCode needs a provider-agnostic
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

struct ResponsesNormalizedStreamProcessor<P> {
    options: ResponsesNormalizedStreamOptions,
    parse_final_response: P,
    aggregator: StreamAggregator,
    seen_tool_calls: HashSet<String>,
    tool_call_indexes: HashMap<String, usize>,
    tool_call_names: HashMap<String, String>,
    tool_call_ids_by_item_id: HashMap<String, String>,
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
            tool_call_ids_by_item_id: HashMap::new(),
            next_tool_call_index: 0,
            final_response: None,
            done: false,
        }
    }

    fn is_done(&self) -> bool {
        self.done
    }

    fn handle_payload(&mut self, payload: Value) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let event = ResponsesStreamAdapter::parse_payload_for_provider(
            self.options.provider_name,
            payload,
        )?;
        self.handle_event(event)
    }

    fn handle_event(
        &mut self,
        event: ResponsesStreamEvent,
    ) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let mut events = Vec::new();

        match event {
            ResponsesStreamEvent::TextDelta { delta } => {
                for event in self.aggregator.handle_content(&delta) {
                    match event {
                        crate::llm::provider::LLMStreamEvent::Token { delta } => {
                            events.push(NormalizedStreamEvent::TextDelta { delta });
                        }
                        crate::llm::provider::LLMStreamEvent::Reasoning { delta }
                            if self.options.emit_reasoning =>
                        {
                            events.push(NormalizedStreamEvent::ReasoningDelta { delta });
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
            ResponsesStreamEvent::ReasoningDelta { delta } => {
                if self.options.emit_reasoning
                    && let Some(delta) = self.aggregator.handle_reasoning(&delta)
                {
                    events.push(NormalizedStreamEvent::ReasoningDelta { delta });
                }
            }
            ResponsesStreamEvent::FunctionCallNameDelta {
                call_id,
                item_id,
                name,
                output_index,
            } => {
                self.record_tool_call_item_id(item_id.as_deref(), &call_id);
                self.record_tool_call_name(&call_id, &name, output_index);
                self.push_tool_call_start(&mut events, call_id, Some(name));
            }
            ResponsesStreamEvent::FunctionCallArgumentsDelta {
                call_id,
                item_id,
                delta,
                output_index,
            } => {
                let call_id = self.provider_tool_call_id(item_id.as_deref(), call_id);
                let call_id = if call_id.is_empty() {
                    format!("tool_call_{}", self.next_tool_call_index)
                } else {
                    call_id
                };
                let index = self.resolve_tool_call_index(&call_id, output_index);

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
            ResponsesStreamEvent::CompletedToolCall {
                call_id,
                item_id,
                name,
                arguments,
                output_index,
            } => {
                self.record_tool_call_item_id(item_id.as_deref(), &call_id);
                let index = self.record_tool_call_name(&call_id, &name, output_index);
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
            ResponsesStreamEvent::Lifecycle { .. }
            | ResponsesStreamEvent::ProviderValueBearingRigGap { .. }
            | ResponsesStreamEvent::Unknown => {}
        }

        Ok(events)
    }

    fn record_tool_call_name(
        &mut self,
        call_id: &str,
        name: &str,
        output_index: Option<usize>,
    ) -> usize {
        self.tool_call_names
            .entry(call_id.to_string())
            .or_insert_with(|| name.to_string());
        let index = self.resolve_tool_call_index(call_id, output_index);
        self.aggregator.handle_tool_calls(&[json!({
            "index": index,
            "id": call_id,
            "function": {
                "name": name,
            }
        })]);
        index
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
        let mut decoder = Utf8StreamDecoder::new();

        while let Some(chunk_result) = body_stream.next().await {
            let chunk = chunk_result.map_err(|err| provider_error(
                provider_name,
                format!("streaming error: {err}"),
            ))?;
            buffer.push_str(&decoder.push(&chunk));

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
) -> Option<crate::llm::provider::Usage> {
    let usage_value = final_response.get("usage")?;
    let cached_prompt_tokens =
        parse_cached_prompt_tokens_from_usage(usage_value, include_cached_prompt_metrics);
    Some(crate::llm::provider::Usage {
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

fn provider_error(provider_name: &str, message: impl Into<String>) -> LLMError {
    let message = error_display::format_llm_error(provider_name, &message.into());
    LLMError::Provider {
        message,
        metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResponsesNormalizedStreamOptions, ResponsesNormalizedStreamProcessor, provider_error,
    };
    use crate::llm::provider::{FinishReason, LLMResponse, NormalizedStreamEvent, ToolCall};
    use serde_json::{Value, json};

    fn options() -> ResponsesNormalizedStreamOptions {
        ResponsesNormalizedStreamOptions {
            provider_name: "TestProvider",
            model: "gpt-5".to_string(),
            emit_reasoning: true,
            include_cached_prompt_metrics: false,
        }
    }

    fn parse_response(value: Value) -> Result<LLMResponse, crate::llm::provider::LLMError> {
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

    fn response_fixture(status: &str, output: Value, usage: Value) -> Value {
        json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 1,
            "status": status,
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "max_output_tokens": null,
            "model": "gpt-5",
            "usage": usage,
            "output": output,
            "tools": []
        })
    }

    fn completed_response_fixture(output: Value) -> Value {
        response_fixture("completed", output, Value::Null)
    }

    fn text_delta_fixture(delta: &str) -> Value {
        json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 1,
            "delta": delta
        })
    }

    fn refusal_delta_fixture(delta: &str) -> Value {
        json!({
            "type": "response.refusal.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 1,
            "delta": delta
        })
    }

    #[test]
    fn text_delta_and_completed_yield_text_then_done() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

        let events = processor
            .handle_payload(text_delta_fixture("hello"))
            .expect("text delta should parse");
        assert!(matches!(
            events.as_slice(),
            [NormalizedStreamEvent::TextDelta { delta }] if delta == "hello"
        ));

        let completed_events = processor
            .handle_payload(json!({
                "type": "response.completed",
                "sequence_number": 2,
                "response": completed_response_fixture(json!([{
                        "type": "message",
                        "id": "msg_1",
                        "status": "completed",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "hello"}]
                    }]))
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
            .handle_payload(text_delta_fixture("streamed answer"))
            .expect("text delta should parse");
        processor
            .handle_payload(json!({
                "type": "response.completed",
                "sequence_number": 2,
                "response": {
                    "id": "resp_streamed",
                    "object": "response",
                    "created_at": 1,
                    "status": "completed",
                    "error": null,
                    "incomplete_details": null,
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": "gpt-5",
                    "usage": {
                        "input_tokens": 11,
                        "output_tokens": 7,
                        "total_tokens": 18,
                        "input_tokens_details": {
                            "cached_tokens": 5
                        }
                    },
                    "output": [],
                    "tools": []
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
                "item_id": "call_1",
                "output_index": 0,
                "sequence_number": 1,
                "item": {
                    "type": "function_call",
                    "id": "call_1",
                    "call_id": "call_1",
                    "name": "search_workspace",
                    "arguments": "",
                    "status": "in_progress"
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
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 2,
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
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 3,
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
                "item_id": "fc_1",
                "output_index": 0,
                "sequence_number": 1,
                "item": {
                    "type": "function_call",
                    "id": "fc_1",
                    "call_id": "call_1",
                    "name": "search_workspace",
                    "arguments": "",
                    "status": "in_progress"
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
                "item_id": "fc_1",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 2,
                "delta": "{\"query\":\"vt"
            }))
            .expect("first tool delta should parse");
        assert!(matches!(
            first.as_slice(),
            [NormalizedStreamEvent::ToolCallDelta { call_id, delta }]
                if call_id == "call_1" && delta == "{\"query\":\"vt"
        ));

        processor
            .handle_payload(json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 3,
                "delta": "code\"}"
            }))
            .expect("second tool delta should parse");

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
    fn custom_tool_input_stream_events_wait_for_completed_response_replay() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), |value| {
            let custom_call = value
                .get("output")
                .and_then(Value::as_array)
                .and_then(|items| items.first())
                .expect("custom tool output should exist");

            Ok(LLMResponse {
                model: "gpt-5".to_string(),
                finish_reason: FinishReason::ToolCalls,
                tool_calls: Some(vec![ToolCall::custom(
                    custom_call
                        .get("call_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    custom_call
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    custom_call
                        .get("input")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                )]),
                ..Default::default()
            })
        });

        let delta_events = processor
            .handle_payload(json!({
                "type": "response.custom_tool_call_input.delta",
                "sequence_number": 1,
                "item_id": "ct_1",
                "call_id": "call_patch_1",
                "output_index": 0,
                "delta": "*** Begin"
            }))
            .expect("custom tool input delta should parse");
        assert!(
            delta_events.is_empty(),
            "custom input deltas are preserved internally but are not runtime dispatch events"
        );

        let done_events = processor
            .handle_payload(json!({
                "type": "response.custom_tool_call_input.done",
                "sequence_number": 2,
                "item_id": "ct_1",
                "call_id": "call_patch_1",
                "output_index": 0,
                "input": "*** Begin Patch\n*** End Patch\n"
            }))
            .expect("custom tool input done should parse");
        assert!(
            done_events.is_empty(),
            "custom tool dispatch remains owned by response.completed replay"
        );

        let completed_events = processor
            .handle_payload(json!({
                "type": "response.completed",
                "sequence_number": 3,
                "response": completed_response_fixture(json!([{
                    "type": "custom_tool_call",
                    "id": "ct_1",
                    "call_id": "call_patch_1",
                    "name": "apply_patch",
                    "input": "*** Begin Patch\n*** End Patch\n"
                }]))
            }))
            .expect("completed event should parse");
        assert!(completed_events.is_empty());

        let finished = processor.finish().expect("finish should succeed");
        let response = match finished.as_slice() {
            [NormalizedStreamEvent::Done { response }] => response,
            _ => panic!("expected final done event"),
        };
        let tool_calls = response
            .tool_calls
            .as_ref()
            .expect("custom tool call should be replayed from final response");
        assert_eq!(tool_calls.len(), 1);
        assert!(tool_calls[0].is_custom());
        assert_eq!(tool_calls[0].id, "call_patch_1");
        assert_eq!(tool_calls[0].tool_name(), Some("apply_patch"));
        assert_eq!(
            tool_calls[0].raw_input(),
            Some("*** Begin Patch\n*** End Patch\n")
        );
    }

    #[test]
    fn refusal_delta_streams_visible_output() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);

        let events = processor
            .handle_payload(refusal_delta_fixture("I can't help with that"))
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
            json!({
                "type": "response.failed",
                "sequence_number": 1,
                "response": {
                    "id": "resp_failed",
                    "object": "response",
                    "created_at": 1,
                    "status": "failed",
                    "error": {"code": "failed", "message": "failed"},
                    "incomplete_details": null,
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": "gpt-5",
                    "usage": null,
                    "output": [],
                    "tools": []
                }
            }),
            json!({
                "type": "response.incomplete",
                "sequence_number": 1,
                "response": {
                    "id": "resp_incomplete",
                    "object": "response",
                    "created_at": 1,
                    "status": "incomplete",
                    "error": {"code": "incomplete", "message": "incomplete"},
                    "incomplete_details": {"reason": "incomplete"},
                    "instructions": null,
                    "max_output_tokens": null,
                    "model": "gpt-5",
                    "usage": null,
                    "output": [],
                    "tools": []
                }
            }),
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
    fn unknown_documented_events_are_ignored() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
        let events = processor
            .handle_payload(json!({
                "type": "response.file_search_call.searching",
                "query": "needle"
            }))
            .expect("unknown documented event should be ignored");
        assert!(events.is_empty());
        processor
            .handle_payload(json!({
                "type": "response.code_interpreter_call.code.delta",
                "delta": "print(1)"
            }))
            .expect("code interpreter event should be ignored");

        let finished = processor.finish().expect("finish should succeed");
        assert!(matches!(
            finished.as_slice(),
            [NormalizedStreamEvent::Done { .. }]
        ));
    }

    #[test]
    fn missing_delta_reports_provider_error() {
        let mut processor = ResponsesNormalizedStreamProcessor::new(options(), parse_response);
        let error = processor
            .handle_payload(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "sequence_number": 1
            }))
            .expect_err("missing delta should fail");
        assert!(error.to_string().contains("TestProvider"));
        assert!(error.to_string().contains("invalid stream payload"));
    }
}
