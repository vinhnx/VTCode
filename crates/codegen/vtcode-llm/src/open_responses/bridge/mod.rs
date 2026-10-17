//! Bridge layer for converting VT Code events to Open Responses format.
//!
//! This module provides adapters to convert VT Code's internal `ThreadEvent`
//! and `ThreadItem` types to Open Responses-conformant structures, enabling
//! backwards compatibility during migration.

use serde_json::json;

use super::{
    ContentPart, CustomItem, FunctionCallItem, ItemStatus, MessageItem, MessageRole, OpenResponseError, OpenUsage,
    OutputItem, ReasoningItem, Response, ResponseStatus, ResponseStreamEvent, StreamEventEmitter,
    response::{generate_item_id, generate_response_id},
};
use crate::provider::{FinishReason, NormalizedStreamEvent, ToolCall};
use vtcode_exec_events::{
    CommandExecutionStatus, McpToolCallStatus, PatchApplyStatus, ThreadEvent, ThreadItem, ThreadItemDetails,
    ToolOutputItem,
};

/// Builder for constructing Open Responses `Response` objects from VT Code events.
///
/// Tracks streaming state and maintains the mapping between VT Code item IDs
/// and Open Responses output item indices.
#[derive(Debug)]
pub struct ResponseBuilder {
    response: Response,
    next_output_index: usize,
    item_id_to_index: hashbrown::HashMap<String, usize>,
    active_items: hashbrown::HashMap<String, ActiveItemState>,
    tool_call_correlation_ids: hashbrown::HashMap<String, String>,
    used_tool_call_ids: hashbrown::HashSet<String>,
    normalized: NormalizedBridgeState,
}

/// State for an active (in-progress) streaming item.
#[derive(Debug, Clone)]
struct ActiveItemState {
    output_index: usize,
    content_index: usize,
    /// Previous text content for safe delta computation (avoids UTF-8 slicing issues)
    prev_text: String,
}

#[derive(Debug, Clone)]
struct NormalizedFunctionCallState {
    item_id: String,
    output_index: usize,
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Default)]
struct NormalizedBridgeState {
    response_started: bool,
    message_item_id: Option<String>,
    reasoning_item_id: Option<String>,
    tool_calls: hashbrown::HashMap<String, NormalizedFunctionCallState>,
}
impl ResponseBuilder {
    /// Creates a new response builder with the given model.
    pub fn new(model: impl Into<String>) -> Self {
        let response = Response::new(generate_response_id(), model);
        Self {
            response,
            next_output_index: 0,
            item_id_to_index: hashbrown::HashMap::new(),
            active_items: hashbrown::HashMap::new(),
            tool_call_correlation_ids: hashbrown::HashMap::new(),
            used_tool_call_ids: hashbrown::HashSet::new(),
            normalized: NormalizedBridgeState::default(),
        }
    }

    /// Returns a reference to the current response.
    pub(crate) fn response(&self) -> &Response {
        &self.response
    }

    /// Returns a mutable reference to the current response.
    pub fn response_mut(&mut self) -> &mut Response {
        &mut self.response
    }

    /// Returns the response ID.
    pub fn response_id(&self) -> &str {
        &self.response.id
    }

    /// Consumes the builder and returns the final response.
    pub fn build(self) -> Response {
        self.response
    }
}

mod emitter;
mod items;
mod normalized;

pub use emitter::DualEventEmitter;

#[cfg(test)]
mod tests;
