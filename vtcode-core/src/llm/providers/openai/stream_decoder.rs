//! Streaming decoders for OpenAI Chat Completions and Responses APIs.

use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::providers::shared::StreamTelemetry;
use crate::llm::providers::shared::{StreamAssemblyError, extract_data_payload, find_sse_boundary};
use async_stream::try_stream;
use futures::StreamExt;
use serde_json::Value;
use std::time::Instant;
#[cfg(debug_assertions)]
use tracing::debug;

use super::responses_api::parse_responses_payload;
use super::streaming::OpenAIStreamTelemetry;

pub(crate) fn create_chat_stream(
    response: reqwest::Response,
    model: String,
) -> provider::LLMStream {
    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregator = crate::llm::providers::shared::StreamAggregator::new(model);
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

                                if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str())
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
        yield provider::LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
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
        let mut aggregator = crate::llm::providers::shared::StreamAggregator::new(model.clone());
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
                                if let Some(delta) = aggregator.handle_reasoning(delta) {
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

        let mut response = parse_responses_payload(response_value, model, include_metrics)?;

        let final_aggregator_response = aggregator.finalize();

        if response.content.is_none() {
            response.content = final_aggregator_response.content;
        } else if let (Some(c), Some(agg_c)) = (&mut response.content, final_aggregator_response.content)
            && !c.contains(&agg_c) {
                c.push_str(&agg_c);
            }

        if response.reasoning.is_none() {
            response.reasoning = final_aggregator_response.reasoning;
        }

        #[cfg(debug_assertions)]
        if let (Some(debug_model), Some(request_timer)) = (_debug_model.as_ref(), _request_timer.as_ref()) {
            debug!(
                target = "vtcode::llm::openai",
                model = %debug_model,
                elapsed_ms = request_timer.elapsed().as_millis(),
                events = streamed_events_counter,
                content_len = response.content.as_ref().map(|c| c.len()).unwrap_or(0),
                "Completed streaming response"
            );
        }

        yield provider::LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
}
