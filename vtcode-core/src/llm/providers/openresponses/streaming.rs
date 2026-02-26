//! OpenResponses streaming event types.
//!
//! This module defines the semantic streaming events used by the OpenResponses specification.
//! See <https://www.openresponses.org/specification#streaming> for details.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Streaming Event Types
// ============================================================================

/// All possible streaming event types in OpenResponses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamEventType {
    // Response lifecycle events
    #[serde(rename = "response.created")]
    ResponseCreated,
    #[serde(rename = "response.in_progress")]
    ResponseInProgress,
    #[serde(rename = "response.completed")]
    ResponseCompleted,
    #[serde(rename = "response.failed")]
    ResponseFailed,
    #[serde(rename = "response.incomplete")]
    ResponseIncomplete,

    // Output item events
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded,
    #[serde(rename = "response.output_item.done")]
    OutputItemDone,

    // Text delta events
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta,
    #[serde(rename = "response.output_text.done")]
    OutputTextDone,

    // Content part events
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded,
    #[serde(rename = "response.content_part.done")]
    ContentPartDone,

    // Function call events
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta,
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone,

    // Reasoning events
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta,
    #[serde(rename = "response.reasoning_summary_text.done")]
    ReasoningSummaryTextDone,

    // Reasoning content events
    #[serde(rename = "response.reasoning_content.delta")]
    ReasoningContentDelta,
    #[serde(rename = "response.reasoning_content.done")]
    ReasoningContentDone,

    // Error event
    #[serde(rename = "error")]
    Error,
}

/// A streaming event from the OpenResponses API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub sequence_number: u32,
    #[serde(flatten)]
    pub data: StreamEventData,
}

/// Data payload for different streaming events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StreamEventData {
    /// Response lifecycle event data.
    Response(ResponseEventData),
    /// Output item event data.
    OutputItem(OutputItemEventData),
    /// Text delta event data.
    TextDelta(TextDeltaEventData),
    /// Function call arguments delta.
    FunctionCallDelta(FunctionCallDeltaEventData),
    /// Reasoning content delta.
    ReasoningContentDelta(ReasoningContentDeltaEventData),
    /// Error event data.
    Error(ErrorEventData),
    /// Generic/unknown event data.
    Generic(Value),
}

/// Data for response lifecycle events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEventData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

/// Data for output item events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputItemEventData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
}

/// Data for text delta events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDeltaEventData {
    pub delta: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_index: Option<u32>,
}

/// Data for function call argument delta events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallDeltaEventData {
    pub delta: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
}

/// Data for reasoning content delta events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningContentDeltaEventData {
    pub delta: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_index: Option<u32>,
}

/// Data for error events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEventData {
    pub error: StreamError,
}

/// Error details in streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

// ============================================================================
// Stream Parsing Utilities
// ============================================================================

/// Parse a Server-Sent Events (SSE) line into a stream event.
pub fn parse_sse_event(line: &str) -> Option<StreamEvent> {
    // SSE format: "data: {...}"
    let line = line.trim();
    if line.is_empty() || line == "[DONE]" {
        return None;
    }

    if let Some(data) = line.strip_prefix("data: ") {
        if data == "[DONE]" {
            return None;
        }
        serde_json::from_str(data).ok()
    } else if line.starts_with('{') {
        // Some implementations send raw JSON
        serde_json::from_str(line).ok()
    } else {
        None
    }
}

/// Extract the event type from an SSE event line.
pub fn extract_event_type(line: &str) -> Option<String> {
    let line = line.trim();
    line.strip_prefix("event: ")
        .map(|event_type| event_type.to_string())
}

/// Accumulator for building responses from streaming events.
#[derive(Debug, Default)]
pub struct StreamAccumulator {
    pub text_content: String,
    pub reasoning_content: String,
    pub function_calls: Vec<AccumulatedFunctionCall>,
    pub current_function_call: Option<AccumulatingFunctionCall>,
    pub output_items: Vec<Value>,
    pub response_id: Option<String>,
    pub model: Option<String>,
    pub usage: Option<Value>,
    pub is_complete: bool,
    pub error: Option<StreamError>,
}

/// A function call being accumulated from streaming deltas.
#[derive(Debug, Clone, Default)]
pub struct AccumulatingFunctionCall {
    pub id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

/// A completed accumulated function call.
#[derive(Debug, Clone)]
pub struct AccumulatedFunctionCall {
    pub id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

impl StreamAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a streaming event and update the accumulator state.
    pub fn process_event(&mut self, event: &StreamEvent) {
        match event.event_type.as_str() {
            "response.created" | "response.in_progress" => {
                if let StreamEventData::Response(data) = &event.data
                    && let Some(response) = &data.response
                {
                    self.response_id = response
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    self.model = response
                        .get("model")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                }
            }
            "response.output_text.delta" => {
                if let StreamEventData::TextDelta(data) = &event.data {
                    self.text_content.push_str(&data.delta);
                }
            }
            "response.reasoning_summary_text.delta" => {
                if let StreamEventData::TextDelta(data) = &event.data {
                    self.reasoning_content.push_str(&data.delta);
                }
            }
            "response.reasoning_content.delta" => {
                if let StreamEventData::ReasoningContentDelta(data) = &event.data {
                    self.reasoning_content.push_str(&data.delta);
                }
            }
            "response.function_call_arguments.delta" => {
                if let StreamEventData::FunctionCallDelta(data) = &event.data
                    && let Some(ref mut fc) = self.current_function_call
                {
                    fc.arguments.push_str(&data.delta);
                }
            }
            "response.output_item.added" => {
                if let StreamEventData::OutputItem(data) = &event.data
                    && let Some(item) = &data.item
                {
                    // Check if this is a function call item
                    if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                        let fc = AccumulatingFunctionCall {
                            id: item
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            call_id: item
                                .get("call_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            name: item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            arguments: String::new(),
                        };
                        self.current_function_call = Some(fc);
                    }
                    self.output_items.push(item.clone());
                }
            }
            "response.output_item.done" => {
                // Finalize current function call if any
                if let Some(fc) = self.current_function_call.take() {
                    self.function_calls.push(AccumulatedFunctionCall {
                        id: fc.id,
                        call_id: fc.call_id,
                        name: fc.name,
                        arguments: fc.arguments,
                    });
                }
            }
            "response.completed" => {
                self.is_complete = true;
                if let StreamEventData::Response(data) = &event.data
                    && let Some(response) = &data.response
                {
                    self.usage = response.get("usage").cloned();
                }
            }
            "response.failed" => {
                self.is_complete = true;
            }
            "error" => {
                if let StreamEventData::Error(data) = &event.data {
                    self.error = Some(data.error.clone());
                }
                self.is_complete = true;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_text_delta() {
        let line =
            r#"data: {"type":"response.output_text.delta","sequence_number":1,"delta":"Hello"}"#;
        let event = parse_sse_event(line).unwrap();
        assert_eq!(event.event_type, "response.output_text.delta");
    }

    #[test]
    fn test_parse_done_signal() {
        assert!(parse_sse_event("[DONE]").is_none());
        assert!(parse_sse_event("data: [DONE]").is_none());
    }

    #[test]
    fn test_stream_accumulator_text() {
        let mut acc = StreamAccumulator::new();

        let event1 = StreamEvent {
            event_type: "response.output_text.delta".to_string(),
            sequence_number: 1,
            data: StreamEventData::TextDelta(TextDeltaEventData {
                delta: "Hello, ".to_string(),
                item_id: None,
                output_index: None,
                content_index: None,
            }),
        };

        let event2 = StreamEvent {
            event_type: "response.output_text.delta".to_string(),
            sequence_number: 2,
            data: StreamEventData::TextDelta(TextDeltaEventData {
                delta: "world!".to_string(),
                item_id: None,
                output_index: None,
                content_index: None,
            }),
        };

        acc.process_event(&event1);
        acc.process_event(&event2);

        assert_eq!(acc.text_content, "Hello, world!");
    }
}
