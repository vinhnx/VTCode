//! Tool call rejection handling.
//!
//! Functions for rejecting tool calls with appropriate error messages and
//! lifecycle events. These handle invalid arguments, denied tools, and
//! policy violations.

use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::AgentRuntime;
use crate::exec::events::{ItemCompletedEvent, ThreadEvent, ThreadItemDetails, ToolCallStatus, ToolOutcome};
use tracing::{error, warn};

/// Reject a tool call with a detail message.
///
/// If the tool call has already been registered in the runtime, marks it as
/// failed and emits the appropriate lifecycle events. Otherwise, records a
/// tool rejection event directly.
pub(super) fn reject_tool_call(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    tool_name: &str,
    args: Option<&serde_json::Value>,
    tool_call_id: &str,
    detail: &str,
    outcome: ToolOutcome,
) {
    if runtime.tool_call_item_id(tool_call_id).is_some() {
        runtime.complete_tool_call(tool_call_id, ToolCallStatus::Failed, Some(outcome));
        let lifecycle_events = runtime.take_emitted_events();
        event_recorder.record_thread_events(lifecycle_events.clone());
        emit_failed_tool_outputs_for_completed_invocations(event_recorder, &lifecycle_events, detail);
        event_recorder.warning(detail);
        return;
    }

    event_recorder.tool_rejected(tool_name, args, Some(tool_call_id), detail);
}

/// Reject a tool call whose arguments could not be parsed or admitted.
///
/// Logs at error level, pushes a structured tool error onto the conversation,
/// and emits the rejection lifecycle events.
pub(super) fn reject_invalid_args(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    agent_prefix: &str,
    tool_name: &str,
    tool_call_id: &str,
    args: Option<&serde_json::Value>,
    err: &dyn std::fmt::Display,
    is_gemini: bool,
    log_msg: &'static str,
) {
    let detail = format!("Invalid arguments for tool '{tool_name}': {err}");
    error!(agent = %agent_prefix, tool = %tool_name, error = %err, "{log_msg}");
    reject_tool_call(runtime, event_recorder, tool_name, args, tool_call_id, &detail, ToolOutcome::InvalidTool);
    runtime
        .state
        .push_tool_error(tool_call_id.to_string(), tool_name, &serde_json::Value::String(detail), is_gemini);
}

/// Reject a tool call that policy or feature gating disallows.
///
/// Records the warning on the session, logs at warn level (unless quiet), and
/// emits the rejection lifecycle events.
pub(super) fn reject_denied_tool(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    agent_prefix: &str,
    tool_name: &str,
    tool_call_id: &str,
    args: Option<&serde_json::Value>,
    is_gemini: bool,
    quiet: bool,
) {
    let detail = format!("Tool execution denied: {tool_name}");
    if !quiet {
        warn!(agent = %agent_prefix, tool = %tool_name, message = %detail);
    }
    runtime.state.warnings.push(detail.clone());
    runtime.state.push_tool_error(
        tool_call_id.to_string(),
        tool_name,
        &serde_json::Value::String(detail.clone()),
        is_gemini,
    );
    reject_tool_call(runtime, event_recorder, tool_name, args, tool_call_id, &detail, ToolOutcome::HookDenied);
}

/// Emit failed tool output events for completed invocations.
pub(super) fn emit_failed_tool_outputs_for_completed_invocations(
    event_recorder: &mut ExecEventRecorder,
    lifecycle_events: &[ThreadEvent],
    detail: &str,
) {
    for event in lifecycle_events {
        let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
            continue;
        };
        let ThreadItemDetails::ToolInvocation(details) = &item.details else {
            continue;
        };
        event_recorder.tool_output_started(&item.id, details.tool_call_id.as_deref());
        event_recorder.tool_output_finished(
            &item.id,
            details.tool_call_id.as_deref(),
            details.status.clone(),
            None,
            detail,
            None,
        );
    }
}
