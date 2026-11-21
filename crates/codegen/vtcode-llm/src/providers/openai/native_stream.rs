//! Bound computer-use route over Responses SSE.
//!
//! The ordinary Responses stream consumers reject native computer items.
//! This bound consumer is the exception: it forwards text and reasoning
//! deltas, tolerates computer items riding the rig-gap events (the completed
//! response is authoritative), and parses the final `response.completed`
//! object with the same non-streaming parser the HTTP route uses, so
//! streamed and non-streamed decode match by construction.

use crate::error_display;
use crate::provider::{LLMError, NativeStream, NativeStreamEvent};
use crate::providers::shared::{
    ResponsesStreamAdapter, ResponsesStreamEvent, Utf8StreamDecoder, drain_and_bound_sse_tail, extract_data_payload,
    find_sse_boundary_bytes,
};
use async_stream::try_stream;
use futures::{StreamExt, pin_mut};

use super::native::parse_response_native;

/// Consume one bound Responses SSE body and accumulate the native events.
pub(super) struct NativeResponsesStreamProcessor {
    model: String,
    completed: bool,
}

impl NativeResponsesStreamProcessor {
    fn new(model: String) -> Self {
        Self { model, completed: false }
    }

    fn is_done(&self) -> bool {
        self.completed
    }

    fn handle_payload_data(&mut self, payload: &str) -> Result<Vec<NativeStreamEvent>, LLMError> {
        match ResponsesStreamAdapter::parse_sse_data_for_provider("OpenAI", payload)? {
            ResponsesStreamEvent::TextDelta { delta } => Ok((!delta.is_empty())
                .then_some(NativeStreamEvent::TextDelta { delta })
                .into_iter()
                .collect()),
            ResponsesStreamEvent::ReasoningDelta { delta, .. } => Ok((!delta.is_empty())
                .then_some(NativeStreamEvent::ReasoningDelta { delta })
                .into_iter()
                .collect()),
            ResponsesStreamEvent::CompletedResponse { response } => {
                let native = parse_response_native(response, self.model.clone())?;
                self.completed = true;
                Ok(vec![NativeStreamEvent::Completed(Box::new(native))])
            }
            ResponsesStreamEvent::Error { message } => Err(stream_error(&message)),
            // Ordinary tool-call deltas stay out of the native event stream:
            // the completed response already carries their decoded forms.
            // Computer items ride the rig-gap events; no per-item handling
            // (and no rejection) happens here because the final response is
            // authoritative on the bound route.
            ResponsesStreamEvent::FunctionCallNameDelta { .. }
            | ResponsesStreamEvent::FunctionCallArgumentsDelta { .. }
            | ResponsesStreamEvent::CompletedToolCall { .. }
            | ResponsesStreamEvent::ProviderValueBearingRigGap { .. }
            | ResponsesStreamEvent::Lifecycle { .. }
            | ResponsesStreamEvent::RefusalDelta { .. }
            | ResponsesStreamEvent::Unknown => Ok(Vec::new()),
        }
    }

    fn finish(&self) -> Result<Vec<NativeStreamEvent>, LLMError> {
        if self.completed {
            Ok(Vec::new())
        } else {
            Err(stream_error("stream ended without a completed response"))
        }
    }
}

/// Stream one bound Responses HTTP response body to native events.
pub(super) fn create_native_stream(response: reqwest::Response, model: String) -> NativeStream {
    let body = response.bytes_stream().map(|chunk| {
        chunk
            .map_err(|err| stream_error(format!("streaming error: {err}")))
            .map(|chunk| chunk.to_vec())
    });
    pump_native_stream(body, model)
}

/// Frame an SSE byte stream into native events. Split from the HTTP entry
/// point so tests can drive the framing loop with plain byte chunks.
fn pump_native_stream<S>(body: S, model: String) -> NativeStream
where
    S: futures::Stream<Item = Result<Vec<u8>, LLMError>> + Send + 'static,
{
    let stream = try_stream! {
        let mut processor = NativeResponsesStreamProcessor::new(model);
        pin_mut!(body);
        let mut buf: Vec<u8> = Vec::new();
        let mut offset = 0usize;
        let mut decoder = Utf8StreamDecoder::new();

        while let Some(chunk_result) = body.next().await {
            let chunk = chunk_result?;
            decoder.push_bytes(&chunk, &mut buf);

            while let Some((split_idx, delimiter_len)) = find_sse_boundary_bytes(&buf, offset) {
                let event = std::str::from_utf8(&buf[offset..split_idx])
                    .map_err(|error| stream_error(format!("invalid utf-8 stream data: {error}")))?;
                offset = split_idx + delimiter_len;

                if let Some(data_payload) = extract_data_payload(event) {
                    let trimmed_payload = data_payload.trim();
                    if trimmed_payload.is_empty() || trimmed_payload == "[DONE]" {
                        continue;
                    }

                    for event in processor.handle_payload_data(trimmed_payload)? {
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
        message: error_display::format_llm_error("OpenAI", &message.to_string()),
        metadata: None,
    }
}

#[cfg(test)]
mod tests;
