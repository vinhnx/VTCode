//! Streaming decoders for OpenAI Chat Completions and Responses APIs.

use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::providers::ReasoningBuffer;
use crate::llm::providers::shared::StreamTelemetry;
use crate::llm::providers::shared::{
    StreamAssemblyError, append_reasoning_segments, extract_data_payload, find_sse_boundary,
};
use crate::llm::providers::tag_sanitizer::TagStreamSanitizer;
use async_stream::try_stream;
use futures::StreamExt;
use serde_json::Value;
use std::time::Instant;
#[cfg(debug_assertions)]
use tracing::debug;

use super::responses_api::parse_responses_payload;
use super::streaming::OpenAIStreamTelemetry;

pub(crate) fn create_chat_stream(response: reqwest::Response) -> provider::LLMStream {
    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregated_content = String::new();
        let mut reasoning_buffer = ReasoningBuffer::default();
        let mut sanitizer = TagStreamSanitizer::new();
        let mut tool_builders = Vec::new();
        let mut finish_reason = provider::FinishReason::Stop;
        let mut accumulated_usage = None;
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

                    if let Some(usage_val) = payload.get("usage") {
                        if let Ok(u) = serde_json::from_value::<provider::Usage>(usage_val.clone()) {
                            accumulated_usage = Some(u);
                        }
                    }

                    if let Some(choices) = payload.get("choices").and_then(|v| v.as_array()) {
                        if let Some(choice) = choices.first() {
                            if let Some(delta) = choice.get("delta") {
                                if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                    aggregated_content.push_str(content);
                                    telemetry.on_content_delta(content);
                                    for event in sanitizer.process_chunk(content) {
                                        match &event {
                                            provider::LLMStreamEvent::Token { delta } => {
                                                yield provider::LLMStreamEvent::Token { delta: delta.clone() };
                                            }
                                            provider::LLMStreamEvent::Reasoning { delta } => {
                                                yield provider::LLMStreamEvent::Reasoning { delta: delta.clone() };
                                            }
                                            _ => {}
                                        }
                                    }
                                }

                                if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                                    for fragment in append_reasoning_segments(&mut reasoning_buffer, reasoning, &telemetry) {
                                        yield provider::LLMStreamEvent::Reasoning { delta: fragment };
                                    }
                                }

                                if let Some(tool_deltas) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                    crate::llm::providers::shared::update_tool_calls(&mut tool_builders, tool_deltas);
                                    telemetry.on_tool_call_delta();
                                }
                            }

                            if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                finish_reason = match reason {
                                    "stop" => provider::FinishReason::Stop,
                                    "length" => provider::FinishReason::Length,
                                    "tool_calls" => provider::FinishReason::ToolCalls,
                                    "content_filter" => provider::FinishReason::ContentFilter,
                                    _ => provider::FinishReason::Stop,
                                };
                            }
                        }
                    }
                }
            }
        }

        for event in sanitizer.finalize() {
            yield event;
        }

        let response = provider::LLMResponse {
            content: if aggregated_content.is_empty() { None } else { Some(aggregated_content) },
            tool_calls: crate::llm::providers::shared::finalize_tool_calls(tool_builders),
            usage: accumulated_usage,
            finish_reason,
            reasoning: reasoning_buffer.finalize(),
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        };

        yield provider::LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
}

pub(crate) fn create_responses_stream(
    response: reqwest::Response,
    include_metrics: bool,
    _debug_model: Option<String>,
    _request_timer: Option<Instant>,
) -> provider::LLMStream {
    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregated_content = String::new();
        let mut reasoning_buffer = ReasoningBuffer::default();
        let mut final_response: Option<Value> = None;
        let mut done = false;
        let mut sanitizer = TagStreamSanitizer::new();
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
                                aggregated_content.push_str(delta);
                                telemetry.on_content_delta(delta);

                                for event in sanitizer.process_chunk(delta) {
                                    match &event {
                                        provider::LLMStreamEvent::Token { delta } => {
                                            yield provider::LLMStreamEvent::Token { delta: delta.clone() };
                                        }
                                        provider::LLMStreamEvent::Reasoning { delta } => {
                                            yield provider::LLMStreamEvent::Reasoning { delta: delta.clone() };
                                        }
                                        _ => {}
                                    }
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
                                aggregated_content.push_str(delta);
                                telemetry.on_content_delta(delta);
                            }
                            "response.reasoning_text.delta" => {
                                let delta = payload
                                    .get("delta")
                                    .and_then(|value| value.as_str())
                                    .ok_or_else(|| {
                                        StreamAssemblyError::MissingField("delta")
                                            .into_llm_error("OpenAI")
                                    })?;
                                for fragment in append_reasoning_segments(&mut reasoning_buffer, delta, &telemetry) {
                                    yield provider::LLMStreamEvent::Reasoning { delta: fragment };
                                }
                            }
                            "response.reasoning_summary_text.delta" => {
                                let delta = payload
                                    .get("delta")
                                    .and_then(|value| value.as_str())
                                    .ok_or_else(|| {
                                        StreamAssemblyError::MissingField("delta")
                                            .into_llm_error("OpenAI")
                                    })?;
                                for fragment in append_reasoning_segments(&mut reasoning_buffer, delta, &telemetry) {
                                    yield provider::LLMStreamEvent::Reasoning { delta: fragment };
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

        for event in sanitizer.finalize() {
            yield event;
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

        let mut response = parse_responses_payload(response_value, include_metrics)?;

        if response.content.is_none() && !aggregated_content.is_empty() {
            response.content = Some(aggregated_content.clone());
        }

        if let Some(reasoning_text) = reasoning_buffer.finalize() {
            response.reasoning = Some(reasoning_text);
        }

        #[cfg(debug_assertions)]
        if let (Some(debug_model), Some(request_timer)) = (_debug_model.as_ref(), _request_timer.as_ref()) {
            debug!(
                target = "vtcode::llm::openai",
                model = %debug_model,
                elapsed_ms = request_timer.elapsed().as_millis(),
                events = streamed_events_counter,
                content_len = aggregated_content.len(),
                "Completed streaming response"
            );
        }

        yield provider::LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
}
