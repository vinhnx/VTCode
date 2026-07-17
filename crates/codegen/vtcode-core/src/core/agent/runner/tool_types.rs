//! Types for tool execution in the runner.
//!
//! Contains the core data structures used for batching, admitting, and
//! tracking tool calls through the execution pipeline.

use crate::core::agent::harness_kernel::{PreparedToolBatchKind, PreparedToolCall};

/// Reference to a tool call item in the conversation thread.
pub(super) struct ToolCallItemRef {
    pub(super) call_item_id: String,
    pub(super) synthetic_invocation: bool,
}

/// A prepared tool call with its associated tool call ID from the LLM.
#[derive(Clone)]
pub(super) struct PreparedRunnerToolCall {
    pub(super) tool_call_id: String,
    pub(super) prepared: PreparedToolCall,
}

/// A batch of prepared tool calls grouped by execution strategy.
pub(super) struct PreparedRunnerToolBatch {
    pub(super) kind: PreparedToolBatchKind,
    pub(super) calls: Vec<PreparedRunnerToolCall>,
}

/// Outcome of admitting a tool call for execution.
pub(super) enum RunnerCallAdmission {
    /// Tool call admitted and ready to execute.
    Prepared(Box<PreparedRunnerToolCall>),
    /// Tool call rejected (invalid args, denied, etc).
    Rejected,
    /// Tool call should stop the turn entirely.
    StopTurn,
}
