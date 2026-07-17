//! Guard for duplicate task tracker creates.
//!
//! Prevents the model from creating duplicate task tracker entries in the same
//! turn. Uses a signature-based dedup approach.

use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;

use super::super::ValidationResult;
use super::super::looping::task_tracker_create_signature;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Enforce the duplicate task tracker create guard.
///
/// Returns `Some(ValidationResult::Blocked)` when a duplicate create is detected,
/// or `None` when the guard passes.
pub(crate) fn enforce_duplicate_task_tracker_create_guard<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &Value,
) -> Option<ValidationResult> {
    let signature = task_tracker_create_signature(canonical_tool_name, effective_args)?;

    if ctx.harness_state.record_task_tracker_create_signature(signature) {
        return None;
    }

    let content = super::super::super::execution_result::build_error_content(
        "Duplicate task_tracker.create detected in this turn. Use task_tracker.update/list to continue tracking progress."
            .to_string(),
        Some(tool_names::TASK_TRACKER.to_string()),
        Some(serde_json::json!({ "action": "list" })),
        "duplicate_task_tracker_create",
    )
    .to_string();
    ctx.push_tool_response(tool_call_id, Some(canonical_tool_name), content);
    Some(ValidationResult::Blocked)
}
