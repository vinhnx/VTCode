//! Server-Sent Events (SSE) stream decoder for Anthropic Claude API
//!
//! Handles streaming responses from the Anthropic API, decoding SSE events
//! and accumulating partial content into a complete LLMResponse.

use crate::llm::provider::LLMError;
use crate::llm::provider::{LLMStreamEvent, Usage};
use crate::llm::providers::anthropic_types::{
    AnthropicContentBlock, AnthropicStreamDelta, AnthropicStreamEvent,
};
use crate::llm::providers::error_handling::format_network_error;
use crate::llm::providers::shared;

use async_stream::try_stream;
use futures::StreamExt;
use serde_json::{Map, Value};

use super::response_parser::parse_finish_reason;

pub fn create_stream(
    response: reqwest::Response,
    model: String,
    request_id: Option<String>,
    organization_id: Option<String>,
) -> crate::llm::provider::LLMStream {
    let stream = try_stream! {
        let mut body_stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut aggregator = shared::StreamAggregator::new(model);

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
                            aggregator.set_usage(Usage {
                                prompt_tokens: message.usage.input_tokens,
                                completion_tokens: 0,
                                total_tokens: message.usage.input_tokens,
                                cached_prompt_tokens: message.usage.cache_read_input_tokens,
                                cache_creation_tokens: message.usage.cache_creation_input_tokens,
                                cache_read_tokens: message.usage.cache_read_input_tokens,
                            });
                        }
                        AnthropicStreamEvent::ContentBlockStart { index, content_block } => {
                            if let AnthropicContentBlock::ToolUse { id, name, .. } = content_block {
                                if aggregator.tool_builders.len() <= index {
                                    aggregator.tool_builders.resize_with(index + 1, shared::ToolCallBuilder::default);
                                }
                                let mut delta = Map::new();
                                delta.insert("id".to_string(), Value::String(id));
                                let mut func = Map::new();
                                func.insert("name".to_string(), Value::String(name));
                                delta.insert("function".to_string(), Value::Object(func));
                                aggregator.tool_builders[index].apply_delta(&Value::Object(delta));
                            }
                        }
                        AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                            match delta {
                                AnthropicStreamDelta::TextDelta { text } => {
                                    for event in aggregator.handle_content(&text) {
                                        yield event;
                                    }
                                }
                                AnthropicStreamDelta::ThinkingDelta { thinking } => {
                                    if let Some(delta) = aggregator.handle_reasoning(&thinking) {
                                        yield LLMStreamEvent::Reasoning { delta };
                                    }
                                }
                                AnthropicStreamDelta::InputJsonDelta { partial_json } => {
                                    if aggregator.tool_builders.len() <= index {
                                        aggregator.tool_builders.resize_with(index + 1, shared::ToolCallBuilder::default);
                                    }
                                    let mut delta_map = Map::new();
                                    let mut func = Map::new();
                                    func.insert("arguments".to_string(), Value::String(partial_json));
                                    delta_map.insert("function".to_string(), Value::Object(func));
                                    aggregator.tool_builders[index].apply_delta(&Value::Object(delta_map));
                                }
                                _ => {}
                            }
                        }
                        AnthropicStreamEvent::MessageDelta { delta, usage } => {
                            if let Some(u) = usage {
                                if let Some(mut current_usage) = aggregator.usage {
                                    current_usage.completion_tokens = u.output_tokens;
                                    current_usage.total_tokens = u.input_tokens + u.output_tokens;
                                    aggregator.usage = Some(current_usage);
                                }
                            }
                            if let Some(reason) = delta.stop_reason {
                                aggregator.set_finish_reason(parse_finish_reason(&reason));
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

        let mut response = aggregator.finalize();
        response.request_id = request_id.clone();
        response.organization_id = organization_id.clone();

        yield LLMStreamEvent::Completed { response: Box::new(response) };
    };

    Box::pin(stream)
}