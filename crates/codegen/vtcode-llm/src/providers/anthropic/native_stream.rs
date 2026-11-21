//! Bound computer-use route over Anthropic Messages SSE.
//!
//! The ordinary stream decoder rejects computer-toolset blocks. This bound
//! accumulator is the exception: it rebuilds content blocks from the typed
//! start/delta/stop events, then reuses the non-streaming parser on the
//! assembled response, so streamed and non-streamed decode match by
//! construction. Usage rides the same `parse_usage`/delta-merge path the
//! display decoder uses.

use std::collections::BTreeMap;

use crate::error_display;
use crate::provider::{LLMError, NativeResponse, NativeStream, NativeStreamEvent, Usage};
use crate::providers::anthropic_types::{
    AnthropicContentBlock, AnthropicStreamDelta, AnthropicStreamEvent, AnthropicToolUseBlock,
};
use crate::providers::shared::{
    Utf8StreamDecoder, drain_and_bound_sse_tail, extract_data_payload, find_sse_boundary_bytes,
};
use async_stream::try_stream;
use futures::{StreamExt, pin_mut};
use serde_json::{Map, Value};

use super::native::parse_response_native;
use super::response_parser::{declined_partial_positions, merge_message_delta_usage, parse_usage};

/// One in-flight content block of the bound response.
enum BlockState {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    ToolUse {
        toolset_name: Option<String>,
        id: String,
        name: String,
        input_json: String,
    },
    /// Any other typed block, kept verbatim for replay.
    Other(AnthropicContentBlock),
}

/// Accumulate one bound Messages SSE body into native events.
pub(super) struct NativeResponsesStreamProcessor {
    model: String,
    request_id: Option<String>,
    organization_id: Option<String>,
    blocks: BTreeMap<usize, BlockState>,
    usage: Option<Usage>,
    stop_reason: Option<String>,
    finished: bool,
}

impl NativeResponsesStreamProcessor {
    fn new(model: String, request_id: Option<String>, organization_id: Option<String>) -> Self {
        Self {
            model,
            request_id,
            organization_id,
            blocks: BTreeMap::new(),
            usage: None,
            stop_reason: None,
            finished: false,
        }
    }

    fn is_done(&self) -> bool {
        self.finished
    }

    fn handle_event(&mut self, event: AnthropicStreamEvent) -> Result<Vec<NativeStreamEvent>, LLMError> {
        match event {
            AnthropicStreamEvent::MessageStart { message } => {
                let usage_value = serde_json::to_value(&message.usage).unwrap_or_else(|_| Map::new().into());
                self.usage = Some(parse_usage(&usage_value));
                Ok(Vec::new())
            }
            AnthropicStreamEvent::ContentBlockStart { index, content_block } => {
                let state = match content_block {
                    AnthropicContentBlock::Text { text, .. } => BlockState::Text { text },
                    AnthropicContentBlock::Thinking { thinking, signature, .. } => {
                        BlockState::Thinking { thinking, signature }
                    }
                    AnthropicContentBlock::ToolUse(tool_use) => BlockState::ToolUse {
                        toolset_name: tool_use.toolset_name,
                        id: tool_use.id,
                        name: tool_use.name,
                        input_json: String::new(),
                    },
                    other => BlockState::Other(other),
                };
                self.blocks.insert(index, state);
                Ok(Vec::new())
            }
            AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                // The accumulator drives execution, so a delta for a block
                // that never started is a protocol violation, not a gap to
                // tolerate: silently dropping it would assemble a truncated
                // tool input (or an empty one via the malformed-input
                // fallback) and decode a response the model never sent. The
                // display decoder tolerates unknown indices because its
                // artifacts are never executed; here the stream fails closed.
                let Some(state) = self.blocks.get_mut(&index) else {
                    return Err(stream_error(format!("content block delta for unknown block index {index}")));
                };
                match (state, delta) {
                    (BlockState::Text { text }, AnthropicStreamDelta::TextDelta { text: delta_text }) => {
                        text.push_str(&delta_text);
                        Ok(vec![NativeStreamEvent::TextDelta { delta: delta_text }])
                    }
                    (
                        BlockState::Thinking { thinking, .. },
                        AnthropicStreamDelta::ThinkingDelta { thinking: delta_thinking },
                    ) => {
                        thinking.push_str(&delta_thinking);
                        Ok(vec![NativeStreamEvent::ReasoningDelta { delta: delta_thinking }])
                    }
                    (
                        BlockState::Thinking { signature, .. },
                        AnthropicStreamDelta::SignatureDelta { signature: delta_signature },
                    ) => {
                        *signature = Some(delta_signature);
                        Ok(Vec::new())
                    }
                    (BlockState::ToolUse { input_json, .. }, AnthropicStreamDelta::InputJsonDelta { partial_json }) => {
                        input_json.push_str(&partial_json);
                        Ok(Vec::new())
                    }
                    (
                        BlockState::Other(AnthropicContentBlock::Compaction { content, .. }),
                        AnthropicStreamDelta::CompactionDelta { content: delta_content, .. },
                    ) => {
                        if delta_content.is_some() {
                            *content = delta_content;
                        }
                        Ok(Vec::new())
                    }
                    _ => Ok(Vec::new()),
                }
            }
            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                if let Some(u) = usage {
                    let usage_value = serde_json::to_value(&u).unwrap_or_else(|_| Map::new().into());
                    let delta_usage = parse_usage(&usage_value);
                    let mut current_usage = self.usage.take().unwrap_or_default();
                    merge_message_delta_usage(&mut current_usage, delta_usage);
                    self.usage = Some(current_usage);
                }
                if let Some(reason) = delta.stop_reason {
                    self.stop_reason = Some(reason);
                }
                Ok(Vec::new())
            }
            AnthropicStreamEvent::MessageStop => {
                self.finished = true;
                let native = self.assemble()?;
                Ok(vec![NativeStreamEvent::Completed(Box::new(native))])
            }
            AnthropicStreamEvent::Error { error } => Err(LLMError::Provider { message: error.message, metadata: None }),
            AnthropicStreamEvent::Ping | AnthropicStreamEvent::Unknown => Ok(Vec::new()),
            AnthropicStreamEvent::ContentBlockStop { .. } => Ok(Vec::new()),
        }
    }

    /// Rebuild the content blocks in wire order and decode through the
    /// non-streaming parser.
    fn assemble(&mut self) -> Result<NativeResponse, LLMError> {
        let mut content = Vec::with_capacity(self.blocks.len());
        for state in self.blocks.values() {
            content.push(match state {
                BlockState::Text { text } => serde_json::to_value(&AnthropicContentBlock::Text {
                    text: text.clone(),
                    citations: None,
                    cache_control: None,
                })
                .map_err(|error| stream_error(format!("failed to serialize text block: {error}")))?,
                BlockState::Thinking { thinking, signature } => {
                    serde_json::to_value(&AnthropicContentBlock::Thinking {
                        thinking: thinking.clone(),
                        signature: signature.clone(),
                        cache_control: None,
                    })
                    .map_err(|error| stream_error(format!("failed to serialize thinking block: {error}")))?
                }
                BlockState::ToolUse { toolset_name, id, name, input_json } => {
                    // An empty accumulator is the legitimate zero-input call
                    // (no `input_json_delta` was sent); anything else must
                    // parse. Silently substituting `{}` would execute a
                    // native call with arguments the model never sent, so a
                    // corrupt partial JSON fails the response instead.
                    let input = if input_json.is_empty() {
                        serde_json::from_str::<Value>("{}")
                    } else {
                        serde_json::from_str::<Value>(input_json)
                    }
                    .map_err(|error| stream_error(format!("tool call {id} input JSON failed to parse: {error}")))?;
                    serde_json::to_value(AnthropicContentBlock::ToolUse(Box::new(AnthropicToolUseBlock {
                        toolset_name: toolset_name.clone(),
                        id: id.clone(),
                        name: name.clone(),
                        input,
                        cache_control: None,
                    })))
                    .map_err(|error| stream_error(format!("failed to serialize tool-use block: {error}")))?
                }
                BlockState::Other(block) => serde_json::to_value(block)
                    .map_err(|error| stream_error(format!("failed to serialize content block: {error}")))?,
            });
        }
        // Mirror the non-streaming parser: a mid-output fallback boundary
        // declines the refused model's thinking and tool calls before it.
        // Reuse the same predicate so streamed decode cannot keep a block —
        // or execute a native call — the non-streaming route would drop.
        let declined = declined_partial_positions(&content);
        let content: Vec<Value> = content
            .into_iter()
            .zip(declined)
            .filter_map(|(block, declined)| (!declined).then_some(block))
            .collect();
        let response_json = serde_json::json!({
            "content": content,
            "stop_reason": self.stop_reason.clone().unwrap_or_else(|| "end_turn".to_string()),
        });
        let mut native = parse_response_native(response_json, self.model.clone())?;
        native.usage = self.usage.take();
        native.request_id = self.request_id.take();
        native.organization_id = self.organization_id.take();
        Ok(native)
    }

    fn finish(&self) -> Result<Vec<NativeStreamEvent>, LLMError> {
        if self.finished {
            Ok(Vec::new())
        } else {
            Err(stream_error("stream ended without message_stop"))
        }
    }
}

/// Stream one bound Messages HTTP response body to native events.
pub(super) fn create_native_stream(
    response: reqwest::Response,
    model: String,
    request_id: Option<String>,
    organization_id: Option<String>,
) -> NativeStream {
    let body = response.bytes_stream().map(|chunk| {
        chunk
            .map_err(|err| stream_error(format!("streaming error: {err}")))
            .map(|chunk| chunk.to_vec())
    });
    pump_native_stream(body, model, request_id, organization_id)
}

/// Frame an SSE byte stream into native events. Split from the HTTP entry
/// point so tests can drive the framing loop with plain byte chunks.
fn pump_native_stream<S>(
    body: S,
    model: String,
    request_id: Option<String>,
    organization_id: Option<String>,
) -> NativeStream
where
    S: futures::Stream<Item = Result<Vec<u8>, LLMError>> + Send + 'static,
{
    let stream = try_stream! {
        let mut processor = NativeResponsesStreamProcessor::new(model, request_id, organization_id);
        pin_mut!(body);
        let mut buf: Vec<u8> = Vec::new();
        let mut offset = 0usize;
        let mut decoder = Utf8StreamDecoder::new();

        while let Some(chunk_result) = body.next().await {
            let chunk = chunk_result?;
            decoder.push_bytes(&chunk, &mut buf);

            while let Some((split_idx, delimiter_len)) = find_sse_boundary_bytes(&buf, offset) {
                let event = std::str::from_utf8(&buf[offset..split_idx]).expect("valid utf-8 stream data");
                offset = split_idx + delimiter_len;

                if let Some(data_payload) = extract_data_payload(event) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload.is_empty() || trimmed_payload == "[DONE]" {
                        continue;
                    }

                    let event: AnthropicStreamEvent = serde_json::from_str(trimmed_payload).map_err(|error| {
                        stream_error(format!("failed to parse stream event: {error}"))
                    })?;
                    for event in processor.handle_event(event)? {
                        yield event;
                    }
                    if processor.is_done() {
                        break;
                    }
                }
            }

            // Drain the consumed prefix, then fail closed when the retained
            // tail tops the bound: a peer that never emits an event boundary
            // must not grow memory without limit. Measured after the drain so
            // one large-but-complete event still parses.
            drain_and_bound_sse_tail(&mut buf, &mut offset).map_err(stream_error)?;

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

fn stream_error(message: impl std::fmt::Display) -> LLMError {
    LLMError::Provider {
        message: error_display::format_llm_error("Anthropic", &message.to_string()),
        metadata: None,
    }
}

#[cfg(test)]
mod tests;
