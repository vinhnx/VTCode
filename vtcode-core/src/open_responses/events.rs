//! Semantic streaming events for Open Responses.
//!
//! Streaming is modeled as a series of semantic events, not raw text deltas.
//! Events describe meaningful transitions like state changes or content deltas.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use super::{ContentPart, OutputItem, Response, ResponseId};

/// Semantic streaming events per the Open Responses specification.
///
/// These events describe meaningful transitions during response generation,
/// enabling predictable, provider-agnostic streaming clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseStreamEvent {
    // ============================================================
    // Response lifecycle events
    // ============================================================
    /// Initial response creation event.
    #[serde(rename = "response.created")]
    ResponseCreated {
        /// The response object with initial state.
        response: Response,
    },

    /// Response has started processing.
    #[serde(rename = "response.in_progress")]
    ResponseInProgress {
        /// The response object.
        response: Response,
    },

    /// Response completed successfully.
    #[serde(rename = "response.completed")]
    ResponseCompleted {
        /// The final response object.
        response: Response,
    },

    /// Response failed with an error.
    #[serde(rename = "response.failed")]
    ResponseFailed {
        /// The response object with error details.
        response: Response,
    },

    /// Response is incomplete (e.g., token limit reached).
    #[serde(rename = "response.incomplete")]
    ResponseIncomplete {
        /// The response object with incomplete details.
        response: Response,
    },

    // ============================================================
    // Output item events
    // ============================================================
    /// New output item added to the response.
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        /// ID of the containing response.
        response_id: ResponseId,
        /// Index of the item in the output array.
        output_index: usize,
        /// The output item being added.
        item: OutputItem,
    },

    /// Output item is complete.
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        /// ID of the containing response.
        response_id: ResponseId,
        /// Index of the item in the output array.
        output_index: usize,
        /// The completed output item.
        item: OutputItem,
    },

    // ============================================================
    // Content part events
    // ============================================================
    /// New content part added to an output item.
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the containing output item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// Index of the content part within the item.
        content_index: usize,
        /// The content part being added.
        part: ContentPart,
    },

    /// Content part is complete.
    #[serde(rename = "response.content_part.done")]
    ContentPartDone {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the containing output item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// Index of the content part within the item.
        content_index: usize,
        /// The completed content part.
        part: ContentPart,
    },

    // ============================================================
    // Text streaming events
    // ============================================================
    /// Text content delta for incremental streaming.
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the containing output item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// Index of the content part within the item.
        content_index: usize,
        /// The text delta to append.
        delta: String,
    },

    /// Text content is complete.
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the containing output item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// Index of the content part within the item.
        content_index: usize,
        /// The complete text content.
        text: String,
    },

    // ============================================================
    // Function call streaming events
    // ============================================================
    /// Function call arguments delta.
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the function call item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// The arguments delta to append.
        delta: String,
    },

    /// Function call arguments are complete.
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the function call item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// The complete arguments JSON string.
        arguments: String,
    },

    // ============================================================
    // Reasoning events
    // ============================================================
    /// Reasoning content delta.
    #[serde(rename = "response.reasoning.delta")]
    ReasoningDelta {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the reasoning item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// The reasoning delta to append.
        delta: String,
    },

    /// Reasoning content is complete.
    #[serde(rename = "response.reasoning.done")]
    ReasoningDone {
        /// ID of the containing response.
        response_id: ResponseId,
        /// ID of the reasoning item.
        item_id: String,
        /// Index of the item in the output array.
        output_index: usize,
        /// The reasoning item with complete content.
        item: OutputItem,
    },

    // ============================================================
    // Extension events
    // ============================================================
    /// Custom/extension streaming event.
    ///
    /// Custom event types must be prefixed with the implementor slug
    /// (e.g., `vtcode.trace_event`).
    #[serde(rename = "response.custom_event")]
    CustomEvent {
        /// ID of the containing response.
        response_id: ResponseId,
        /// Custom event type (must be prefixed, e.g., `vtcode.telemetry`).
        event_type: String,
        /// Sequence number for ordering.
        sequence_number: u64,
        /// Custom event data.
        data: serde_json::Value,
    },
}

impl ResponseStreamEvent {
    /// Returns the response ID associated with this event.
    pub fn response_id(&self) -> &str {
        match self {
            Self::ResponseCreated { response, .. }
            | Self::ResponseInProgress { response, .. }
            | Self::ResponseCompleted { response, .. }
            | Self::ResponseFailed { response, .. }
            | Self::ResponseIncomplete { response, .. } => &response.id,

            Self::OutputItemAdded { response_id, .. }
            | Self::OutputItemDone { response_id, .. }
            | Self::ContentPartAdded { response_id, .. }
            | Self::ContentPartDone { response_id, .. }
            | Self::OutputTextDelta { response_id, .. }
            | Self::OutputTextDone { response_id, .. }
            | Self::FunctionCallArgumentsDelta { response_id, .. }
            | Self::FunctionCallArgumentsDone { response_id, .. }
            | Self::ReasoningDelta { response_id, .. }
            | Self::ReasoningDone { response_id, .. }
            | Self::CustomEvent { response_id, .. } => response_id,
        }
    }

    /// Returns the event type name.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::ResponseCreated { .. } => "response.created",
            Self::ResponseInProgress { .. } => "response.in_progress",
            Self::ResponseCompleted { .. } => "response.completed",
            Self::ResponseFailed { .. } => "response.failed",
            Self::ResponseIncomplete { .. } => "response.incomplete",
            Self::OutputItemAdded { .. } => "response.output_item.added",
            Self::OutputItemDone { .. } => "response.output_item.done",
            Self::ContentPartAdded { .. } => "response.content_part.added",
            Self::ContentPartDone { .. } => "response.content_part.done",
            Self::OutputTextDelta { .. } => "response.output_text.delta",
            Self::OutputTextDone { .. } => "response.output_text.done",
            Self::FunctionCallArgumentsDelta { .. } => "response.function_call_arguments.delta",
            Self::FunctionCallArgumentsDone { .. } => "response.function_call_arguments.done",
            Self::ReasoningDelta { .. } => "response.reasoning.delta",
            Self::ReasoningDone { .. } => "response.reasoning.done",
            Self::CustomEvent { .. } => "response.custom_event",
        }
    }

    /// Returns true if this is a response lifecycle event.
    pub fn is_response_event(&self) -> bool {
        matches!(
            self,
            Self::ResponseCreated { .. }
                | Self::ResponseInProgress { .. }
                | Self::ResponseCompleted { .. }
                | Self::ResponseFailed { .. }
                | Self::ResponseIncomplete { .. }
        )
    }

    /// Returns true if this is a terminal event.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::ResponseCompleted { .. }
                | Self::ResponseFailed { .. }
                | Self::ResponseIncomplete { .. }
        )
    }
}

/// Callback type for streaming events.
#[allow(dead_code)]
pub type StreamEventCallback = Arc<Mutex<Box<dyn FnMut(&ResponseStreamEvent) + Send>>>;

/// Trait for emitting Open Responses streaming events.
pub trait StreamEventEmitter: Send {
    /// Emit a streaming event.
    fn emit(&mut self, event: ResponseStreamEvent);

    /// Emit a response created event.
    fn response_created(&mut self, response: Response) {
        self.emit(ResponseStreamEvent::ResponseCreated { response });
    }

    /// Emit a response in progress event.
    fn response_in_progress(&mut self, response: Response) {
        self.emit(ResponseStreamEvent::ResponseInProgress { response });
    }

    /// Emit a response completed event.
    fn response_completed(&mut self, response: Response) {
        self.emit(ResponseStreamEvent::ResponseCompleted { response });
    }

    /// Emit a response failed event.
    fn response_failed(&mut self, response: Response) {
        self.emit(ResponseStreamEvent::ResponseFailed { response });
    }

    /// Emit an output item added event.
    fn output_item_added(&mut self, response_id: &str, output_index: usize, item: OutputItem) {
        self.emit(ResponseStreamEvent::OutputItemAdded {
            response_id: response_id.to_string(),
            output_index,
            item,
        });
    }

    /// Emit an output item done event.
    fn output_item_done(&mut self, response_id: &str, output_index: usize, item: OutputItem) {
        self.emit(ResponseStreamEvent::OutputItemDone {
            response_id: response_id.to_string(),
            output_index,
            item,
        });
    }

    /// Emit a text delta event.
    fn output_text_delta(
        &mut self,
        response_id: &str,
        item_id: &str,
        output_index: usize,
        content_index: usize,
        delta: &str,
    ) {
        self.emit(ResponseStreamEvent::OutputTextDelta {
            response_id: response_id.to_string(),
            item_id: item_id.to_string(),
            output_index,
            content_index,
            delta: delta.to_string(),
        });
    }

    /// Emit a reasoning delta event.
    fn reasoning_delta(
        &mut self,
        response_id: &str,
        item_id: &str,
        output_index: usize,
        delta: &str,
    ) {
        self.emit(ResponseStreamEvent::ReasoningDelta {
            response_id: response_id.to_string(),
            item_id: item_id.to_string(),
            output_index,
            delta: delta.to_string(),
        });
    }
}

/// Vector-based event emitter for collecting events.
#[derive(Debug, Default)]
pub struct VecStreamEmitter {
    events: Vec<ResponseStreamEvent>,
}

impl VecStreamEmitter {
    /// Creates a new vector emitter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the collected events.
    pub fn events(&self) -> &[ResponseStreamEvent] {
        &self.events
    }

    /// Consumes and returns the collected events.
    pub fn into_events(self) -> Vec<ResponseStreamEvent> {
        self.events
    }
}

impl StreamEventEmitter for VecStreamEmitter {
    fn emit(&mut self, event: ResponseStreamEvent) {
        self.events.push(event);
    }
}

/// Wrapper for streaming events with sequence number for ordering.
/// Used when serializing events for SSE transport.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct SequencedEvent<'a> {
    /// Monotonically increasing sequence number within the stream.
    pub sequence_number: u64,
    /// The underlying event.
    #[serde(flatten)]
    pub event: &'a ResponseStreamEvent,
}

impl<'a> SequencedEvent<'a> {
    /// Creates a new sequenced event.
    #[allow(dead_code)]
    pub fn new(sequence_number: u64, event: &'a ResponseStreamEvent) -> Self {
        Self {
            sequence_number,
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type() {
        let response = Response::new("resp_1", "gpt-4");
        let event = ResponseStreamEvent::ResponseCreated { response };
        assert_eq!(event.event_type(), "response.created");
    }

    #[test]
    fn test_terminal_events() {
        let response = Response::new("resp_1", "gpt-4");
        let created = ResponseStreamEvent::ResponseCreated {
            response: response.clone(),
        };
        let completed = ResponseStreamEvent::ResponseCompleted { response };
        assert!(!created.is_terminal());
        assert!(completed.is_terminal());
    }

    #[test]
    fn test_vec_emitter() {
        let mut emitter = VecStreamEmitter::new();
        let response = Response::new("resp_1", "gpt-4");
        emitter.response_created(response);
        assert_eq!(emitter.events().len(), 1);
    }
}
