//! Shared utilities for guard modules.
//!
//! Provides common functions used across multiple guard implementations:
//! - `push_guard_failure_messages`: Pushes error and system messages for guard failures
//! - `is_read_action`: Checks if a tool call is a file read operation
//! - `extract_read_path`: Extracts the file path from read tool arguments

use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;

use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Push error and system messages for a guard failure.
///
/// This is the standard pattern for all guard failures:
/// 1. Push a tool response with the error content
/// 2. Push a system message with the block reason
#[cold]
pub(crate) fn push_guard_failure_messages(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    error_content: String,
    block_reason: &str,
) {
    ctx.push_tool_response(tool_call_id, Some(tool_name), error_content);
    ctx.push_system_message(block_reason.to_string());
}

/// Check if a tool call is a file read operation.
///
/// Returns `true` for `read_file` and `unified_file` with `action: read`.
pub(crate) fn is_read_action(canonical_tool_name: &str, args: &Value) -> bool {
    match canonical_tool_name {
        tool_names::READ_FILE => true,
        tool_names::UNIFIED_FILE => {
            let action = args.get("action").and_then(Value::as_str).unwrap_or("read");
            action.eq_ignore_ascii_case("read")
        }
        _ => false,
    }
}

/// Extract the file path from read tool arguments.
///
/// Returns `None` if the path is missing or empty.
pub(crate) fn extract_read_path(args: &Value) -> Option<String> {
    args.get("path")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}
