//! Tool outcome handlers for the agent turn loop.
//!
//! This module contains the functions for handling tool execution outcomes:
//! - Permission checking (prepare)
//! - Execution with caching
//! - Success/failure/timeout/cancelled handling

use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;

mod apply;
mod dispatch;
pub(crate) mod error_handling;
mod execution_result;
pub(crate) mod handlers;
pub(crate) mod helpers;
pub(crate) mod read_extent;
mod response_content;
mod subagent_memory;

pub(crate) use apply::apply_turn_outcome;
pub(crate) use dispatch::handle_tool_calls;
pub(crate) use execution_result::{
    ToolFailureDiagnosis, bounded_diagnostic_field, bounded_error_evidence, bounded_output_evidence,
    deterministic_error_diagnosis, deterministic_output_diagnosis, escape_untrusted_evidence, render_diagnosis,
};
pub(crate) use handlers::ToolOutcomeContext;

/// Return whether a grep-style command's non-zero result is the ordinary
/// no-match signal rather than an execution error.
///
/// Grep and ripgrep reserve exit code 1 for no matches and use exit code 2 for
/// syntax, argument, or other execution errors. Keep this predicate shared by
/// loop detection and deterministic failure diagnosis so those paths cannot
/// disagree about whether a failed command is low-signal.
pub(crate) fn is_grep_style_no_match(tool_name: &str, args: &Value, output: &Value) -> bool {
    if !matches!(tool_name, tool_names::UNIFIED_EXEC | tool_names::EXEC_COMMAND)
        || output.get("exit_code").and_then(Value::as_i64) != Some(1)
    {
        return false;
    }

    let command_args = if args.get("command").is_none() && args.get("cmd").is_none() {
        output
    } else {
        args
    };
    vtcode_core::tools::tool_intent::shell_command_is_standalone_grep_search(command_args)
        && search_output_is_fully_visible(output)
        && search_output_is_empty(output)
}

/// Empty successful search pipelines still carry no new navigation evidence.
/// Do not confuse a hidden preview, an in-progress capture, or a quiet probe
/// with a completed empty search. This is a loop signal, never a success or
/// verification verdict for an earlier pipeline stage.
pub(crate) fn is_empty_shell_search(tool_name: &str, args: &Value, output: &Value) -> bool {
    matches!(tool_name, tool_names::UNIFIED_EXEC | tool_names::EXEC_COMMAND)
        && vtcode_core::tools::tool_intent::is_command_run_tool_call(tool_name, args)
        && output.get("exit_code").and_then(Value::as_i64) == Some(0)
        && vtcode_core::tools::tool_intent::shell_command_is_output_search(args)
        && search_output_is_fully_visible(output)
        && search_output_is_empty(output)
}

fn search_output_is_fully_visible(output: &Value) -> bool {
    ![
        "truncated",
        "output_truncated",
        "is_truncated",
        "result_ref_only",
        "spool_ref_only",
    ]
    .iter()
    .any(|key| output.get(*key).and_then(Value::as_bool) == Some(true))
        && ["total_output_bytes", "spooled_bytes", "byte_count"]
            .iter()
            .all(|key| output.get(*key).is_none_or(|value| value.as_u64() == Some(0)))
        && output.get("spool_path").is_none_or(Value::is_null)
}

fn search_output_is_empty(output: &Value) -> bool {
    [
        "stdout",
        "output",
        "preview",
        "content",
        "stderr",
        "stderr_preview",
        "error",
        "message",
        "critical_note",
        "warning",
        "hint",
    ]
    .iter()
    .all(|key| output_field_is_empty(output.get(*key)))
}

pub(crate) fn output_field_is_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(Value::Array(values)) => values.is_empty(),
        Some(Value::Bool(_) | Value::Number(_) | Value::Object(_)) => false,
    }
}
