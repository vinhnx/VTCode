use serde_json::Value;

use crate::llm::error_display;
use crate::llm::provider::{FinishReason, LLMError, LLMResponse, ToolCall};

use super::OpenRouterProvider;
use super::response_helpers::{
    append_reasoning_segment, extract_reasoning_from_message_content,
    extract_tool_calls_from_content,
};
use super::stream_decoder::{
    OpenRouterStreamTelemetry, map_finish_reason, parse_usage_value, process_content_value,
};
use super::super::{ReasoningBuffer, extract_reasoning_trace, split_reasoning_from_text};
use super::super::shared::{StreamDelta, ToolCallBuilder, finalize_tool_calls};

impl OpenRouterProvider {
    pub(super) fn parse_openrouter_response(
        &self,
        response_json: Value,
    ) -> Result<LLMResponse, LLMError> {
        if let Some(choices) = response_json
            .get("choices")
            .and_then(|value| value.as_array())
        {
            if choices.is_empty() {
                let formatted_error =
                    error_display::format_llm_error("OpenRouter", "No choices in response");
                return Err(LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                });
            }

            let choice = &choices[0];
            let message = choice.get("message").ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenRouter",
                    "Invalid response format: missing message",
                );
                LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

            let mut content = match message.get("content") {
                Some(Value::String(text)) => Some(text.to_string()),
                Some(Value::Array(parts)) => {
                    let text = parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("");
                    if text.is_empty() { None } else { Some(text) }
                }
                _ => None,
            };

            let tool_calls = message
                .get("tool_calls")
                .and_then(|tc| tc.as_array())
                .map(|calls| {
                    calls
                        .iter()
                        .filter_map(|call| {
                            let id = call.get("id").and_then(|v| v.as_str())?;
                            let function = call.get("function")?;
                            let name = function.get("name").and_then(|v| v.as_str())?;
                            let arguments = function.get("arguments");
                            let serialized = arguments.map_or("{}".to_string(), |value| {
                                if value.is_string() {
                                    value.as_str().unwrap_or("").to_string()
                                } else {
                                    value.to_string()
                                }
                            });
                            Some(ToolCall::function(
                                id.to_string(),
                                name.to_string(),
                                serialized,
                            ))
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|calls| !calls.is_empty());

            let reasoning_details = message
                .get("reasoning_details")
                .and_then(|rd| rd.as_array())
                .cloned();

            let mut reasoning_segments: Vec<String> = Vec::new();

            // If reasoning_details are present, prioritize them over text extraction
            // Models that support reasoning_details should have clean content without markup
            if reasoning_details.is_none() {
                if let Some(initial) = message
                    .get("reasoning")
                    .and_then(extract_reasoning_trace)
                    .or_else(|| choice.get("reasoning").and_then(extract_reasoning_trace))
                {
                    append_reasoning_segment(&mut reasoning_segments, &initial);
                }

                if reasoning_segments.is_empty() {
                    if let Some(from_content) = extract_reasoning_from_message_content(message) {
                        append_reasoning_segment(&mut reasoning_segments, &from_content);
                    }
                } else if let Some(extra) = extract_reasoning_from_message_content(message) {
                    append_reasoning_segment(&mut reasoning_segments, &extra);
                }

                if let Some(original_content) = content.take() {
                    let (markup_segments, cleaned) = split_reasoning_from_text(&original_content);
                    for segment in markup_segments {
                        append_reasoning_segment(&mut reasoning_segments, &segment);
                    }
                    content = match cleaned {
                        Some(cleaned_text) => {
                            if cleaned_text.is_empty() {
                                None
                            } else {
                                Some(cleaned_text)
                            }
                        }
                        None => Some(original_content),
                    };
                }
            }

            let reasoning = if reasoning_segments.is_empty() {
                None
            } else {
                Some(reasoning_segments.join("\n"))
            };

            let finish_reason = choice
                .get("finish_reason")
                .and_then(|fr| fr.as_str())
                .map(map_finish_reason)
                .unwrap_or(FinishReason::Stop);

            let usage = response_json.get("usage").map(parse_usage_value);

            return Ok(LLMResponse {
                content,
                tool_calls,
                usage,
                finish_reason,
                reasoning,
                reasoning_details,
                tool_references: Vec::new(),
                request_id: None,
                organization_id: None,
            });
        }

        self.parse_responses_api_response(&response_json)
    }

    fn parse_responses_api_response(&self, payload: &Value) -> Result<LLMResponse, LLMError> {
        let response_container = payload.get("response").unwrap_or(payload);

        let outputs = response_container
            .get("output")
            .or_else(|| response_container.get("outputs"))
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                let formatted_error = error_display::format_llm_error(
                    "OpenRouter",
                    "Invalid response format: missing output",
                );
                LLMError::Provider {
                    message: formatted_error,
                    metadata: None,
                }
            })?;

        if outputs.is_empty() {
            let formatted_error =
                error_display::format_llm_error("OpenRouter", "No output in response");
            return Err(LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }

        let message = outputs
            .iter()
            .find(|value| {
                value
                    .get("role")
                    .and_then(|role| role.as_str())
                    .map(|role| role == "assistant")
                    .unwrap_or(true)
            })
            .unwrap_or(&outputs[0]);

        let mut aggregated_content = String::new();
        let mut reasoning_buffer = ReasoningBuffer::default();
        let mut tool_call_builders: Vec<ToolCallBuilder> = Vec::new();
        let mut deltas = StreamDelta::default();
        let telemetry = OpenRouterStreamTelemetry;

        if let Some(content_value) = message.get("content") {
            process_content_value(
                content_value,
                &mut aggregated_content,
                &mut reasoning_buffer,
                &mut tool_call_builders,
                &mut deltas,
                &telemetry,
            );
        } else {
            process_content_value(
                message,
                &mut aggregated_content,
                &mut reasoning_buffer,
                &mut tool_call_builders,
                &mut deltas,
                &telemetry,
            );
        }

        let mut tool_calls = finalize_tool_calls(tool_call_builders);
        if tool_calls.is_none() {
            tool_calls = extract_tool_calls_from_content(message);
        }

        let reasoning_details = message
            .get("reasoning_details")
            .and_then(|rd| rd.as_array())
            .cloned();

        let mut reasoning_segments: Vec<String> = Vec::new();

        let mut content = if aggregated_content.is_empty() {
            message
                .get("output_text")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        } else {
            Some(aggregated_content)
        };

        // If reasoning_details are present, prioritize them over text extraction
        if reasoning_details.is_none() {
            if let Some(buffer_reasoning) = reasoning_buffer.finalize() {
                append_reasoning_segment(&mut reasoning_segments, &buffer_reasoning);
            }

            let fallback_reasoning = extract_reasoning_from_message_content(message)
                .or_else(|| message.get("reasoning").and_then(extract_reasoning_trace))
                .or_else(|| payload.get("reasoning").and_then(extract_reasoning_trace));

            if reasoning_segments.is_empty() {
                if let Some(segment) = fallback_reasoning {
                    append_reasoning_segment(&mut reasoning_segments, &segment);
                }
            } else if let Some(segment) = fallback_reasoning {
                append_reasoning_segment(&mut reasoning_segments, &segment);
            }

            if let Some(original_content) = content.take() {
                let (markup_segments, cleaned) = split_reasoning_from_text(&original_content);
                for segment in markup_segments {
                    append_reasoning_segment(&mut reasoning_segments, &segment);
                }
                content = cleaned.or_else(|| Some(original_content));
            }
        }

        let reasoning = if reasoning_segments.is_empty() {
            None
        } else {
            Some(reasoning_segments.join("\n"))
        };

        let finish_reason = message
            .get("finish_reason")
            .and_then(|value| value.as_str())
            .map(map_finish_reason)
            .unwrap_or(FinishReason::Stop);

        let mut usage = payload.get("usage").map(parse_usage_value);
        if usage.is_none() {
            usage = response_container.get("usage").map(parse_usage_value);
        }

        Ok(LLMResponse {
            content,
            tool_calls,
            usage,
            finish_reason,
            reasoning,
            reasoning_details,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        })
    }
}
