use serde_json::{Map, Value};

use crate::llm::provider::{FinishReason, LLMResponse, Usage};

use super::super::{
    ReasoningBuffer, extract_reasoning_trace,
    shared::{
        StreamDelta, StreamTelemetry, ToolCallBuilder, append_text_with_reasoning,
        apply_tool_call_delta_from_content, finalize_tool_calls, update_tool_calls,
    },
};

#[cfg(debug_assertions)]
use tracing::debug;

pub(super) struct OpenRouterStreamTelemetry;

impl StreamTelemetry for OpenRouterStreamTelemetry {
    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn on_content_delta(&self, delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openrouter::stream",
            length = delta.len(),
            "content delta received"
        );
    }

    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn on_reasoning_delta(&self, delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openrouter::stream",
            length = delta.len(),
            "reasoning delta received"
        );
    }

    fn on_tool_call_delta(&self) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openrouter::stream",
            "tool call delta received"
        );
    }
}

pub(super) fn process_content_object(
    map: &Map<String, Value>,
    aggregated_content: &mut String,
    reasoning: &mut ReasoningBuffer,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    if let Some(content_type) = map.get("type").and_then(|value| value.as_str()) {
        match content_type {
            "reasoning" | "thinking" | "analysis" => {
                if let Some(text_value) = map.get("text").and_then(|value| value.as_str()) {
                    if let Some(delta) = reasoning.push(text_value) {
                        telemetry.on_reasoning_delta(&delta);
                        deltas.push_reasoning(&delta);
                    }
                } else if let Some(text_value) =
                    map.get("output_text").and_then(|value| value.as_str())
                {
                    if let Some(delta) = reasoning.push(text_value) {
                        telemetry.on_reasoning_delta(&delta);
                        deltas.push_reasoning(&delta);
                    }
                }
                return;
            }
            "tool_call_delta" | "tool_call" => {
                apply_tool_call_delta_from_content(tool_call_builders, map, telemetry);
                return;
            }
            _ => {}
        }
    }

    if let Some(tool_call_value) = map.get("tool_call").and_then(|value| value.as_object()) {
        apply_tool_call_delta_from_content(tool_call_builders, tool_call_value, telemetry);
        return;
    }

    if let Some(text_value) = map.get("text").and_then(|value| value.as_str()) {
        append_text_with_reasoning(text_value, aggregated_content, reasoning, deltas, telemetry);
        return;
    }

    if let Some(text_value) = map.get("output_text").and_then(|value| value.as_str()) {
        append_text_with_reasoning(text_value, aggregated_content, reasoning, deltas, telemetry);
        return;
    }

    if let Some(text_value) = map
        .get("output_text_delta")
        .and_then(|value| value.as_str())
    {
        append_text_with_reasoning(text_value, aggregated_content, reasoning, deltas, telemetry);
        return;
    }

    for key in ["content", "items", "output", "outputs", "delta"] {
        if let Some(inner) = map.get(key) {
            process_content_value(
                inner,
                aggregated_content,
                reasoning,
                tool_call_builders,
                deltas,
                telemetry,
            );
        }
    }
}

fn process_content_part(
    value: &Value,
    aggregated_content: &mut String,
    reasoning: &mut ReasoningBuffer,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    if let Some(content_obj) = value.as_object() {
        process_content_object(
            content_obj,
            aggregated_content,
            reasoning,
            tool_call_builders,
            deltas,
            telemetry,
        );
        return;
    }

    if let Some(content_array) = value.as_array() {
        for item in content_array {
            process_content_value(
                item,
                aggregated_content,
                reasoning,
                tool_call_builders,
                deltas,
                telemetry,
            );
        }
    }
}

pub(super) fn process_content_value(
    value: &Value,
    aggregated_content: &mut String,
    reasoning: &mut ReasoningBuffer,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    if let Some(content_array) = value.as_array() {
        for item in content_array {
            process_content_part(
                item,
                aggregated_content,
                reasoning,
                tool_call_builders,
                deltas,
                telemetry,
            );
        }
        return;
    }

    if let Some(content_obj) = value.as_object() {
        process_content_object(
            content_obj,
            aggregated_content,
            reasoning,
            tool_call_builders,
            deltas,
            telemetry,
        );
    }
}

pub(super) fn parse_usage_value(value: &Value) -> Usage {
    let cache_read_tokens = value
        .get("prompt_cache_read_tokens")
        .or_else(|| value.get("cache_read_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let cache_creation_tokens = value
        .get("prompt_cache_write_tokens")
        .or_else(|| value.get("cache_creation_input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    Usage {
        prompt_tokens: value
            .get("prompt_tokens")
            .and_then(|pt| pt.as_u64())
            .unwrap_or(0) as u32,
        completion_tokens: value
            .get("completion_tokens")
            .and_then(|ct| ct.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: value
            .get("total_tokens")
            .and_then(|tt| tt.as_u64())
            .unwrap_or(0) as u32,
        cached_prompt_tokens: cache_read_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    }
}

pub(super) fn map_finish_reason(reason: &str) -> FinishReason {
    super::super::common::map_finish_reason_common(reason)
}

fn push_reasoning_value(
    reasoning: &mut ReasoningBuffer,
    value: &Value,
    deltas: &mut StreamDelta,
    telemetry: &impl StreamTelemetry,
) {
    if let Some(reasoning_text) = extract_reasoning_trace(value) {
        if let Some(delta) = reasoning.push(&reasoning_text) {
            telemetry.on_reasoning_delta(&delta);
            deltas.push_reasoning(&delta);
        }
    } else if let Some(text_value) = value.get("text").and_then(|v| v.as_str()) {
        if let Some(delta) = reasoning.push(text_value) {
            telemetry.on_reasoning_delta(&delta);
            deltas.push_reasoning(&delta);
        }
    }
}

fn parse_chat_completion_chunk(
    payload: &Value,
    aggregated_content: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    reasoning: &mut ReasoningBuffer,
    finish_reason: &mut FinishReason,
    telemetry: &impl StreamTelemetry,
) -> StreamDelta {
    let mut deltas = StreamDelta::default();

    if let Some(choices) = payload.get("choices").and_then(|c| c.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(delta) = choice.get("delta") {
                if let Some(content_value) = delta.get("content") {
                    process_content_value(
                        content_value,
                        aggregated_content,
                        reasoning,
                        tool_call_builders,
                        &mut deltas,
                        telemetry,
                    );
                }

                if let Some(reasoning_value) = delta.get("reasoning") {
                    push_reasoning_value(reasoning, reasoning_value, &mut deltas, telemetry);
                }

                if let Some(tool_calls_value) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    update_tool_calls(tool_call_builders, tool_calls_value);
                }
            }

            if let Some(reasoning_value) = choice.get("reasoning") {
                push_reasoning_value(reasoning, reasoning_value, &mut deltas, telemetry);
            }

            if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                *finish_reason = map_finish_reason(reason);
            }
        }
    }

    deltas
}

fn parse_response_chunk(
    payload: &Value,
    aggregated_content: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    reasoning: &mut ReasoningBuffer,
    finish_reason: &mut FinishReason,
    telemetry: &impl StreamTelemetry,
) -> StreamDelta {
    let mut deltas = StreamDelta::default();

    if let Some(delta_value) = payload.get("delta") {
        process_content_value(
            delta_value,
            aggregated_content,
            reasoning,
            tool_call_builders,
            &mut deltas,
            telemetry,
        );
    }

    if let Some(event_type) = payload.get("type").and_then(|v| v.as_str()) {
        match event_type {
            "response.reasoning.delta" => {
                if let Some(delta_value) = payload.get("delta") {
                    push_reasoning_value(reasoning, delta_value, &mut deltas, telemetry);
                }
            }
            "response.tool_call.delta" => {
                if let Some(delta_object) = payload.get("delta").and_then(|v| v.as_object()) {
                    apply_tool_call_delta_from_content(tool_call_builders, delta_object, telemetry);
                }
            }
            "response.completed" | "response.done" | "response.finished" => {
                if let Some(response_obj) = payload.get("response") {
                    if aggregated_content.is_empty() {
                        process_content_value(
                            response_obj,
                            aggregated_content,
                            reasoning,
                            tool_call_builders,
                            &mut deltas,
                            telemetry,
                        );
                    }

                    if let Some(reason) = response_obj
                        .get("stop_reason")
                        .and_then(|value| value.as_str())
                        .or_else(|| response_obj.get("status").and_then(|value| value.as_str()))
                    {
                        *finish_reason = map_finish_reason(reason);
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(response_obj) = payload.get("response") {
        if aggregated_content.is_empty() {
            if let Some(content_value) = response_obj
                .get("output_text")
                .or_else(|| response_obj.get("output"))
                .or_else(|| response_obj.get("content"))
            {
                process_content_value(
                    content_value,
                    aggregated_content,
                    reasoning,
                    tool_call_builders,
                    &mut deltas,
                    telemetry,
                );
            }
        }
    }

    if let Some(reasoning_value) = payload.get("reasoning") {
        push_reasoning_value(reasoning, reasoning_value, &mut deltas, telemetry);
    }

    deltas
}

fn update_usage_from_value(source: &Value, usage: &mut Option<Usage>) {
    if let Some(usage_value) = source.get("usage") {
        *usage = Some(parse_usage_value(usage_value));
    }
}

pub(crate) fn parse_stream_payload(
    payload: &Value,
    aggregated_content: &mut String,
    tool_call_builders: &mut Vec<ToolCallBuilder>,
    reasoning: &mut ReasoningBuffer,
    usage: &mut Option<Usage>,
    finish_reason: &mut FinishReason,
    telemetry: &impl StreamTelemetry,
) -> Option<StreamDelta> {
    let mut emitted_delta = StreamDelta::default();

    let chat_delta = parse_chat_completion_chunk(
        payload,
        aggregated_content,
        tool_call_builders,
        reasoning,
        finish_reason,
        telemetry,
    );
    emitted_delta.extend(chat_delta);

    let response_delta = parse_response_chunk(
        payload,
        aggregated_content,
        tool_call_builders,
        reasoning,
        finish_reason,
        telemetry,
    );
    emitted_delta.extend(response_delta);

    update_usage_from_value(payload, usage);
    if let Some(response_obj) = payload.get("response") {
        update_usage_from_value(response_obj, usage);
        if let Some(reason) = response_obj
            .get("finish_reason")
            .and_then(|value| value.as_str())
        {
            *finish_reason = map_finish_reason(reason);
        }
    }

    if emitted_delta.is_empty() {
        None
    } else {
        Some(emitted_delta)
    }
}

pub(super) fn finalize_stream_response(
    aggregated_content: String,
    tool_call_builders: Vec<ToolCallBuilder>,
    usage: Option<Usage>,
    finish_reason: FinishReason,
    reasoning: ReasoningBuffer,
) -> LLMResponse {
    let content = if aggregated_content.is_empty() {
        None
    } else {
        Some(aggregated_content)
    };

    let reasoning = reasoning.finalize();

    LLMResponse {
        content,
        tool_calls: finalize_tool_calls(tool_call_builders),
        usage,
        finish_reason,
        reasoning,
        reasoning_details: None,
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
    }
}
