use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMNormalizedStream, LLMResponse, NormalizedStreamEvent};
use crate::llm::providers::shared::{extract_data_payload, find_sse_boundary};
use async_stream::try_stream;
use futures::StreamExt;
use hashbrown::{HashMap, HashSet};
use serde_json::{Value, json};

use super::StreamAggregator;

pub struct ResponsesNormalizedStreamOptions {
    pub provider_name: &'static str,
    pub model: String,
    pub emit_reasoning: bool,
}

struct ResponsesNormalizedStreamProcessor<P> {
    options: ResponsesNormalizedStreamOptions,
    parse_final_response: P,
    aggregator: StreamAggregator,
    seen_tool_calls: HashSet<String>,
    tool_call_indexes: HashMap<String, usize>,
    tool_call_names: HashMap<String, String>,
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

        let event_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
        match event_type {
            "response.output_text.delta" => {
                let delta = payload
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| provider_error(self.options.provider_name, "missing delta"))?;
                for event in self.aggregator.handle_content(delta) {
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
                if self.options.emit_reasoning
                    && let Some(delta) = payload.get("delta").and_then(Value::as_str)
                    && let Some(delta) = self.aggregator.handle_reasoning(delta)
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
                let call_id = payload
                    .get("item_id")
                    .and_then(Value::as_str)
                    .or_else(|| payload.get("call_id").and_then(Value::as_str))
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
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
            _ => {}
        }

        Ok(events)
    }

    fn finish(self) -> Result<Vec<NormalizedStreamEvent>, LLMError> {
        let streamed = self.aggregator.finalize();
        let mut response = if let Some(final_response) = self.final_response {
            (self.parse_final_response)(final_response)?
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

        let call_id = item
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| item.get("call_id").and_then(Value::as_str))
            .filter(|value| !value.is_empty());
        let name = item.get("name").and_then(Value::as_str).or_else(|| {
            item.get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
        });
        if let (Some(call_id), Some(name)) = (call_id, name) {
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
            return Some((call_id.to_string(), name.to_string()));
        }

        None
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
            .handle_payload(json!({"type": "response.output_text.delta"}))
            .expect_err("missing delta should fail");
        assert_eq!(
            error.to_string(),
            provider_error("TestProvider", "missing delta").to_string()
        );
    }
}
