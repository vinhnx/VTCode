//! Server-Sent Events (SSE) stream decoder for Anthropic Claude API
//!
//! Handles streaming responses from the Anthropic API, decoding SSE events
//! and accumulating partial content into a complete LLMResponse.

use crate::llm::provider::LLMError;
use crate::llm::provider::{FinishReason, LLMResponse, LLMStreamEvent, Usage};
use crate::llm::providers::anthropic_types::{
    AnthropicContentBlock, AnthropicStreamDelta, AnthropicStreamEvent,
};
use crate::llm::providers::error_handling::format_network_error;
use crate::llm::providers::{ReasoningBuffer, shared};

use async_stream::try_stream;
use futures::StreamExt;
use serde_json::{Map, Value};

use super::response_parser::parse_finish_reason;

pub fn create_stream(
    response: reqwest::Response,
    request_id: Option<String>,
    organization_id: Option<String>,
) -> crate::llm::provider::LLMStream {
    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregated_content = String::new();
        let mut reasoning_buffer = ReasoningBuffer::default();
        let mut tool_builders = Vec::<shared::ToolCallBuilder>::new();
        let mut finish_reason = FinishReason::Stop;
        let mut accumulated_usage = Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        };

        while let Some(chunk_result) = body_stream.next().await {
            let chunk = chunk_result.map_err(|err| {
                format_network_error("Anthropic", &anyhow::Error::new(err))
            })?;

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some((split_idx, delimiter_len)) = shared::find_sse_boundary(&buffer) {
                let event_text = buffer[..split_idx].to_string();
                buffer.drain(..split_idx + delimiter_len);

                if let Some(data_payload) = shared::extract_data_payload(&event_text) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload.is_empty() {
                        continue;
                    }

                    let event: AnthropicStreamEvent = serde_json::from_str(trimmed_payload).map_err(|err| {
                        LLMError::Provider {
                            message: format!("Failed to parse stream event: {}", err),
                            metadata: None,
                        }
                    })?;

                    match event {
                        AnthropicStreamEvent::MessageStart { message } => {
                            accumulated_usage.prompt_tokens = message.usage.input_tokens;
                            accumulated_usage.cached_prompt_tokens = message.usage.cache_read_input_tokens;
                            accumulated_usage.cache_creation_tokens = message.usage.cache_creation_input_tokens;
                            accumulated_usage.cache_read_tokens = message.usage.cache_read_input_tokens;
                        }
                        AnthropicStreamEvent::ContentBlockStart { index, content_block } => {
                            if let AnthropicContentBlock::ToolUse { id, name, .. } = content_block {
                                if tool_builders.len() <= index {
                                    tool_builders.resize_with(index + 1, shared::ToolCallBuilder::default);
                                }
                                let mut delta = Map::new();
                                delta.insert("id".to_string(), Value::String(id));
                                let mut func = Map::new();
                                func.insert("name".to_string(), Value::String(name));
                                delta.insert("function".to_string(), Value::Object(func));
                                tool_builders[index].apply_delta(&Value::Object(delta));
                            }
                        }
                        AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                            match delta {
                                AnthropicStreamDelta::TextDelta { text } => {
                                    aggregated_content.push_str(&text);
                                    yield LLMStreamEvent::Token { delta: text };
                                }
                                AnthropicStreamDelta::ThinkingDelta { thinking } => {
                                    if let Some(delta) = reasoning_buffer.push(&thinking) {
                                        yield LLMStreamEvent::Reasoning { delta };
                                    }
                                }
                                AnthropicStreamDelta::InputJsonDelta { partial_json } => {
                                    if tool_builders.len() <= index {
                                        tool_builders.resize_with(index + 1, shared::ToolCallBuilder::default);
                                    }
                                    let mut delta_map = Map::new();
                                    let mut func = Map::new();
                                    func.insert("arguments".to_string(), Value::String(partial_json));
                                    delta_map.insert("function".to_string(), Value::Object(func));
                                    tool_builders[index].apply_delta(&Value::Object(delta_map));
                                }
                                _ => {}
                            }
                        }
                        AnthropicStreamEvent::MessageDelta { delta, usage } => {
                            if let Some(u) = usage {
                                accumulated_usage.completion_tokens = u.output_tokens;
                                accumulated_usage.total_tokens = u.input_tokens + u.output_tokens;
                            }
                            if let Some(reason) = delta.stop_reason {
                                finish_reason = parse_finish_reason(&reason);
                            }
                        }
                        AnthropicStreamEvent::Error { error } => {
                            Err(LLMError::Provider {
                                message: error.message,
                                metadata: None,
                            })?
                        }
                        _ => {}
                    }
                }
            }
        }

        let response = LLMResponse {
            content: if aggregated_content.is_empty() { None } else { Some(aggregated_content) },
            tool_calls: shared::finalize_tool_calls(tool_builders),
            usage: Some(accumulated_usage),
            finish_reason,
            reasoning: reasoning_buffer.finalize(),
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: request_id.clone(),
            organization_id: organization_id.clone(),
        };

        yield LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
}
