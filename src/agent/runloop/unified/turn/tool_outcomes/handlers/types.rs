use serde_json::Value;

use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingContext};

/// Result of a tool call validation phase.
pub(crate) enum ValidationResult {
    /// Proceed with execution
    Proceed(PreparedToolCall),
    /// Tool was blocked by policy/guardrails, skip execution and track blocked-call guard
    Blocked,
    /// Tool call was intentionally handled and should not count as blocked
    Handled,
    /// Stop turn/loop with a specific outcome (e.g. Exit or Cancel)
    Outcome(TurnHandlerOutcome),
}

/// Canonicalized validation data reused across the execution path.
pub(crate) struct PreparedToolCall {
    pub canonical_name: String,
    pub readonly_classification: bool,
    pub parallel_safe_after_preflight: bool,
    pub effective_args: Value,
}

/// Consolidated state for tool outcomes to reduce signature bloat and keep handlers DRY.
pub(crate) struct ToolOutcomeContext<'a, 'b> {
    pub ctx: &'b mut TurnProcessingContext<'a>,
    pub repeated_tool_attempts: &'b mut super::super::helpers::LoopTracker,
    pub turn_modified_files: &'b mut std::collections::BTreeSet<std::path::PathBuf>,
}
