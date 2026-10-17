use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::unified::state::CtrlCState;

mod cache;

/// Bundles the Ctrl+C state and notification handle together to reduce
/// parameter counts in internal tool-pipeline functions.
#[derive(Clone)]
pub(super) struct CancellationTokens {
    pub state: Arc<CtrlCState>,
    pub notify: Arc<Notify>,
}

impl CancellationTokens {
    pub(super) fn new(state: &Arc<CtrlCState>, notify: &Arc<Notify>) -> Self {
        Self {
            state: Arc::clone(state),
            notify: Arc::clone(notify),
        }
    }
}

mod execution;
pub(crate) mod execution_attempts;
mod execution_events;
mod execution_helpers;
mod execution_run;
mod execution_runtime;
mod file_conflict_prompt;
mod file_conflict_runtime;
mod hitl;
mod pty_stream;
pub(crate) mod status;
#[cfg(test)]
mod tests;
mod timeout;
pub(crate) mod validation;

pub(crate) use execution::{run_tool_call, run_tool_call_with_args};
pub(crate) use execution_events::{
    emit_tool_completion_for_status, emit_tool_outcome_observation, emit_tool_start_if_needed,
};
pub(crate) use execution_run::exec_settlement_mode_for_tool_call;
pub(crate) use execution_run::resolve_harness_item_identity;
pub(crate) use execution_runtime::execute_prevalidated_read_only_with_cache;
pub(crate) use hitl::execute_hitl_tool;
pub(crate) use pty_stream::PtyStreamRuntime;
pub(crate) use status::{ToolBatchOutcome, ToolDisplayStatus, ToolExecutionStatus, ToolPipelineOutcome};

/// Returns whether the unified pipeline creates a live PTY output block for a
/// tool call. Keep this predicate alongside the pipeline so rendering and
/// execution cannot drift apart as legacy command aliases evolve.
pub(crate) fn streams_pty_output(tool_name: &str, args: &Value) -> bool {
    match tool_name {
        tools::SEND_PTY_INPUT => true,
        tools::RUN_PTY_CMD | tools::EXEC_COMMAND | tools::EXEC_PTY_CMD | tools::UNIFIED_EXEC => {
            tool_intent::is_command_run_tool_call(tool_name, args)
        }
        _ => false,
    }
}

/// The tool names that address an already-running exec session.
///
/// Shared by the transcript-summary folding and the bounded-session-body
/// rendering so the two cannot drift as legacy session aliases evolve. `UNIFIED_EXEC`
/// is excluded here because follow-up detection needs the call's `action`, which
/// [`is_exec_session_call`] layers on top of this set.
pub(crate) const EXEC_SESSION_FOLLOWUP_TOOLS: &[&str] =
    &[tools::WRITE_STDIN, tools::SEND_PTY_INPUT, tools::READ_PTY_SESSION];

/// Whether a tool name addresses an already-running exec session rather than
/// launching one.
pub(crate) fn is_exec_session_tool_name(tool_name: &str) -> bool {
    EXEC_SESSION_FOLLOWUP_TOOLS.contains(&tool_name)
}

/// Whether a call addresses an exec session, handling `unified_exec` follow-ups.
///
/// Every `unified_exec` action except `run` is a follow-up; `run` (explicit or
/// inferred from a command field) is the launch path. An actionless call also
/// falls on the follow-up side, which is harmless because it fails execution
/// before rendering.
pub(crate) fn is_exec_session_call(tool_name: &str, args: &Value) -> bool {
    is_exec_session_tool_name(tool_name)
        || (tool_name == tools::UNIFIED_EXEC && tool_intent::command_session_action(args) != Some("run"))
}

/// Returns whether the live PTY block contains the `• Ran ...` command
/// header. Input forwarding streams output but has no command header of its
/// own, so it must use the ordinary tool summary path for status display.
pub(crate) fn renders_pty_command_header(tool_name: &str, args: &Value) -> bool {
    tool_name != tools::SEND_PTY_INPUT && streams_pty_output(tool_name, args)
}

/// Default timeout for tool execution if no policy is configured.
/// Sourced from `vtcode_config::constants::execution::DEFAULT_TOOL_TIMEOUT_SECS`
/// so the value is tunable without touching this file.
const DEFAULT_TOOL_TIMEOUT: Duration =
    Duration::from_secs(vtcode_config::constants::execution::DEFAULT_TOOL_TIMEOUT_SECS);
/// Minimum buffer before cancelling a tool once a warning fires
const MIN_TIMEOUT_WARNING_HEADROOM: Duration = Duration::from_secs(5);
const RETRY_BACKOFF_BASE: Duration = Duration::from_millis(200);
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(3);
