//! Streaming payload decoding helpers for OpenRouter.

use crate::llm::provider::{FinishReason, Usage};
use crate::llm::providers::ReasoningBuffer;
use crate::llm::providers::shared::{StreamDelta, StreamTelemetry, ToolCallBuilder};
use serde_json::Value;

#[cfg(test)]
#[cfg(test)]
pub(crate) fn parse_usage_value(value: &Value) -> Usage {
    let prompt_tokens = value
        .get("prompt_tokens")
        .and_then(|token| token.as_u64())
        .unwrap_or(0) as u32;
    let completion_tokens = value
        .get("completion_tokens")
        .and_then(|token| token.as_u64())
        .unwrap_or(0) as u32;
    let total_tokens = value
        .get("total_tokens")
        .and_then(|token| token.as_u64())
        .unwrap_or(0) as u32;
    let cache_read_tokens = value
        .get("prompt_cache_read_tokens")
        .and_then(|token| token.as_u64())
        .map(|token| token as u32);
    let cache_creation_tokens = value
        .get("prompt_cache_write_tokens")
        .and_then(|token| token.as_u64())
        .map(|token| token as u32);
    let cached_prompt_tokens = cache_read_tokens;

    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
        cached_prompt_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    }
}

#[cfg(test)]
#[cfg(test)]
pub(crate) fn parse_stream_payload(
    payload: &Value,
    aggregated: &mut String,
    builders: &mut Vec<ToolCallBuilder>,
    reasoning: &mut ReasoningBuffer,
    usage: &mut Option<Usage>,
    finish_reason: &mut FinishReason,
    telemetry: &impl StreamTelemetry,
) -> Option<StreamDelta> {
    if let Some(usage_value) = payload.get("usage") {
        *usage = Some(parse_usage_value(usage_value));
    }

    let mut delta = StreamDelta::default();

    if let Some(event_type) = payload.get("type").and_then(|value| value.as_str()) {
        if event_type == "response.delta" {
            if let Some(delta_value) = payload.get("delta") {
                if let Some(delta_type) = delta_value.get("type").and_then(|value| value.as_str()) {
                    match delta_type {
                        "output_text_delta" | "output_text" | "text_delta" => {
                            if let Some(text) =
                                delta_value.get("text").and_then(|value| value.as_str())
                            {
                                telemetry.on_content_delta(text);
                                aggregated.push_str(text);
                                delta.push_content(text);
                            }
                        }
                        "reasoning_text_delta" | "reasoning_summary_text_delta" => {
                            if let Some(text) =
                                delta_value.get("text").and_then(|value| value.as_str())
                            {
                                if let Some(reasoning_delta) = reasoning.push(text) {
                                    telemetry.on_reasoning_delta(&reasoning_delta);
                                    delta.push_reasoning(&reasoning_delta);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if let Some(choices) = payload.get("choices").and_then(|value| value.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(delta_value) = choice.get("delta") {
                if let Some(content_value) = delta_value.get("content") {
                    match content_value {
                        Value::String(text) => {
                            telemetry.on_content_delta(text);
                            aggregated.push_str(text);
                            delta.push_content(text);
                        }
                        Value::Array(parts) => {
                            for part in parts {
                                if let Some(text) =
                                    part.get("text").and_then(|value| value.as_str())
                                {
                                    telemetry.on_content_delta(text);
                                    aggregated.push_str(text);
                                    delta.push_content(text);
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(reasoning_value) = delta_value
                    .get("reasoning_content")
                    .and_then(|value| value.as_str())
                {
                    if let Some(reasoning_delta) = reasoning.push(reasoning_value) {
                        telemetry.on_reasoning_delta(&reasoning_delta);
                        delta.push_reasoning(&reasoning_delta);
                    }
                }

                if let Some(tool_calls) = delta_value
                    .get("tool_calls")
                    .and_then(|value| value.as_array())
                {
                    for (index, tool_delta) in tool_calls.iter().enumerate() {
                        if builders.len() <= index {
                            builders.resize_with(index + 1, ToolCallBuilder::default);
                        }
                        builders[index].apply_delta(tool_delta);
                    }
                    telemetry.on_tool_call_delta();
                }
            }

            if let Some(reason) = choice.get("finish_reason").and_then(|value| value.as_str()) {
                *finish_reason = match reason {
                    "stop" => FinishReason::Stop,
                    "length" => FinishReason::Length,
                    "tool_calls" => FinishReason::ToolCalls,
                    "content_filter" => FinishReason::ContentFilter,
                    other => FinishReason::Error(other.to_string()),
                };
            }
        }
    }

    if delta.is_empty() { None } else { Some(delta) }
}
