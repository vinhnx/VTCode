//! Streaming decoders for OpenAI Chat Completions and Responses APIs.
//!
//! Retained custom decoder boundary: Rig's SSE parser does not currently prove
//! parity for VTCode's legacy `LLMStreamEvent` shape, fallback from empty final
//! Responses output to streamed deltas, cached prompt usage overlay, and
//! provider-specific error mapping. Protected by this module's
//! `stream_decoder` tests and provider mock streaming tests. Remove only once a
//! Rig stream adapter preserves the same final `LLMResponse` and runtime event
//! behaviour.

use crate::error_display;
use crate::provider;
use crate::providers::shared::StreamTelemetry;
use crate::providers::shared::parse_cached_prompt_tokens_from_usage;
use crate::providers::shared::{StreamAssemblyError, extract_data_payload, find_sse_boundary};
use async_stream::try_stream;
use futures::StreamExt;
use serde_json::Value;
use std::time::Instant;
use vtcode_tool_types::model_family::find_family_for_model;

use super::responses_api::parse_responses_payload;
use super::streaming::OpenAIStreamTelemetry;

fn strip_reasoning_for_model(
    model: &str,
    mut response: provider::LLMResponse,
) -> provider::LLMResponse {
    if !find_family_for_model(model).supports_reasoning_summaries {
        response.reasoning = None;
        response.reasoning_details = None;
    }

    response
}

fn streamed_response_is_usable(response: &provider::LLMResponse) -> bool {
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

fn merge_final_response_metadata(
    response: &mut provider::LLMResponse,
    final_response: &Value,
    include_cached_prompt_metrics: bool,
) {
    if let Some(usage_value) = final_response.get("usage") {
        let cached_prompt_tokens =
            parse_cached_prompt_tokens_from_usage(usage_value, include_cached_prompt_metrics);

        response.usage = Some(provider::Usage {
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
        });
    }

    if let Some(request_id) = final_response
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| final_response.get("request_id").and_then(Value::as_str))
    {
        response.request_id = Some(request_id.to_string());
    }
}

pub(crate) fn create_chat_stream(
    response: reqwest::Response,
    model: String,
) -> provider::LLMStream {
    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let retain_reasoning_summaries = find_family_for_model(&model).supports_reasoning_summaries;
        let mut aggregator = crate::providers::shared::StreamAggregator::new(model.clone());
        let telemetry = OpenAIStreamTelemetry;

        while let Some(chunk_result) = body_stream.next().await {
            let chunk = chunk_result.map_err(|err| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Streaming error: {}", err),
                );
                provider::LLMError::Network { message: formatted_error, metadata: None }
            })?;

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
                        StreamAssemblyError::InvalidPayload(err.to_string())
                            .into_llm_error("OpenAI")
                    })?;

                    if let Some(usage_val) = payload.get("usage")
                        && let Ok(u) = serde_json::from_value::<provider::Usage>(usage_val.clone()) {
                            aggregator.set_usage(u);
                        }

                    if let Some(choices) = payload.get("choices").and_then(|v| v.as_array())
                        && let Some(choice) = choices.first() {
                            if let Some(delta) = choice.get("delta") {
                                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                    telemetry.on_content_delta(content);
                                    for event in aggregator.handle_content(content) {
                                        yield event;
                                    }
                                }

                                if retain_reasoning_summaries
                                    && let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str())
                                    && let Some(delta) = aggregator.handle_reasoning(reasoning) {
                                        telemetry.on_reasoning_delta(&delta);
                                        yield provider::LLMStreamEvent::Reasoning { delta };
                                    }

                                if let Some(tool_deltas) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                    aggregator.handle_tool_calls(tool_deltas);
                                    telemetry.on_tool_call_delta();
                                }
                            }

                            if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                aggregator.set_finish_reason(match reason {
                                    "stop" => provider::FinishReason::Stop,
                                    "length" => provider::FinishReason::Length,
                                    "tool_calls" => provider::FinishReason::ToolCalls,
                                    "content_filter" => provider::FinishReason::ContentFilter,
                                    _ => provider::FinishReason::Stop,
                                });
                            }
                        }
                }
            }
        }

        let response = aggregator.finalize();
        let response = strip_reasoning_for_model(&model, response);
        yield provider::LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
}

#[cfg(test)]
mod tests {
    use super::{
        final_response_output_is_empty, merge_final_response_metadata, streamed_response_is_usable,
    };
    use crate::provider::{LLMResponse, ToolCall};
    use serde_json::json;

    #[test]
    fn responses_final_metadata_parses_cached_prompt_tokens_when_enabled() {
        let mut response = LLMResponse::default();
        merge_final_response_metadata(
            &mut response,
            &json!({
                "id": "resp_stream",
                "usage": {
                    "input_tokens": 12,
                    "output_tokens": 5,
                    "total_tokens": 17,
                    "input_tokens_details": {
                        "cached_tokens": 9
                    }
                }
            }),
            true,
        );

        assert_eq!(response.request_id.as_deref(), Some("resp_stream"));
        let usage = response.usage.expect("usage should be populated");
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.total_tokens, 17);
        assert_eq!(usage.cached_prompt_tokens, Some(9));
    }

    #[test]
    fn empty_final_response_can_use_streamed_tool_call_delta() {
        let response = LLMResponse {
            tool_calls: Some(vec![ToolCall::function(
                "call_1".to_string(),
                "search_workspace".to_string(),
                "{\"query\":\"vtcode\"}".to_string(),
            )]),
            ..Default::default()
        };

        assert!(final_response_output_is_empty(&json!({"output": []})));
        assert!(streamed_response_is_usable(&response));
    }
}

pub(crate) fn create_responses_stream(
    response: reqwest::Response,
    model: String,
    include_metrics: bool,
    _debug_model: Option<String>,
    _request_timer: Option<Instant>,
) -> provider::LLMStream {
    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregator = crate::providers::shared::StreamAggregator::new(model.clone());
        let retain_reasoning_summaries = find_family_for_model(&model).supports_reasoning_summaries;
        let mut final_response: Option<Value> = None;
        let mut done = false;
        #[cfg(debug_assertions)]
        let mut streamed_events_counter: usize = 0;
        let telemetry = OpenAIStreamTelemetry;

        while let Some(chunk_result) = body_stream.next().await {
            let chunk = chunk_result.map_err(|err| {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    &format!("Streaming error: {}", err),
                );
                provider::LLMError::Network { message: formatted_error, metadata: None }
            })?;

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some((split_idx, delimiter_len)) = find_sse_boundary(&buffer) {
                let event = buffer[..split_idx].to_string();
                buffer.drain(..split_idx + delimiter_len);
                #[cfg(debug_assertions)]
                {
                    streamed_events_counter = streamed_events_counter.saturating_add(1);
                }

                if let Some(data_payload) = extract_data_payload(&event) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload.is_empty() {
                        continue;
                    }

                    if trimmed_payload == "[DONE]" {
                        done = true;
                        break;
                    }

                    let payload: Value = serde_json::from_str(trimmed_payload).map_err(|err| {
                        StreamAssemblyError::InvalidPayload(err.to_string())
                            .into_llm_error("OpenAI")
                    })?;

                    if let Some(event_type) = payload.get("type").and_then(|value| value.as_str()) {
                        match event_type {
                            "response.output_text.delta" => {
                                let delta = payload
                                    .get("delta")
                                    .and_then(|value| value.as_str())
                                    .ok_or_else(|| {
                                        StreamAssemblyError::MissingField("delta")
                                            .into_llm_error("OpenAI")
                                    })?;
                                telemetry.on_content_delta(delta);

                                for event in aggregator.handle_content(delta) {
                                    yield event;
                                }
                            }
                            "response.refusal.delta" => {
                                let delta = payload
                                    .get("delta")
                                    .and_then(|value| value.as_str())
                                    .ok_or_else(|| {
                                        StreamAssemblyError::MissingField("delta")
                                            .into_llm_error("OpenAI")
                                    })?;
                                telemetry.on_content_delta(delta);
                                aggregator.content.push_str(delta);
                            }
                            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                                let delta = payload
                                    .get("delta")
                                    .and_then(|value| value.as_str())
                                    .ok_or_else(|| {
                                        StreamAssemblyError::MissingField("delta")
                                            .into_llm_error("OpenAI")
                                    })?;
                                if retain_reasoning_summaries
                                    && let Some(delta) = aggregator.handle_reasoning(delta) {
                                    telemetry.on_reasoning_delta(&delta);
                                    yield provider::LLMStreamEvent::Reasoning { delta };
                                }
                            }
                            "response.function_call_arguments.delta" => {}
                            "response.completed" => {
                                if let Some(response_value) = payload.get("response") {
                                    final_response = Some(response_value.clone());
                                }
                                done = true;
                            }
                            "response.failed" | "response.incomplete" => {
                                let error_message = if let Some(err) = payload.get("response")
                                    .and_then(|r| r.get("error"))
                                {
                                    err.get("message")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Unknown error")
                                } else {
                                    "Unknown error from Responses API"
                                };
                                let formatted_error = error_display::format_llm_error("OpenAI", error_message);
                                Err(provider::LLMError::Provider {
                                    message: formatted_error,
                                    metadata: None,
                                })?;
                            }
                            _ => {}
                        }
                    }
                }

                if done {
                    break;
                }
            }

            if done {
                break;
            }
        }

        let response_value = match final_response {
            Some(value) => value,
            None => {
                let formatted_error = error_display::format_llm_error(
                    "OpenAI",
                    "Stream ended without a completion event",
                );
                Err(provider::LLMError::Provider { message: formatted_error, metadata: None })?
            }
        };

        let final_aggregator_response = aggregator.finalize();
        let mut response = match parse_responses_payload(response_value.clone(), model.clone(), include_metrics) {
            Ok(response) => response,
            Err(_)
                if final_response_output_is_empty(&response_value)
                    && streamed_response_is_usable(&final_aggregator_response) =>
            {
                let mut response = final_aggregator_response.clone();
                merge_final_response_metadata(&mut response, &response_value, include_metrics);
                response
            }
            Err(err) => Err(err)?,
        };

        if response.content.is_none() {
            response.content = final_aggregator_response.content;
        } else if let (Some(c), Some(agg_c)) = (&mut response.content, final_aggregator_response.content)
            && !c.contains(&agg_c) {
                c.push_str(&agg_c);
            }

        if response.reasoning.is_none() {
            response.reasoning = final_aggregator_response.reasoning;
        }

        let response = strip_reasoning_for_model(&model, response);
        yield provider::LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
}
