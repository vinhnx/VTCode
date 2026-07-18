//! Shared helper functions for tool dispatch, eliminating duplication between
//! `tool_exec.rs` (tool execution) and `execute.rs` (turn loop).

use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runner::types::ToolFailureContext;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::session::AgentSessionState;

/// Drain all pending runtime lifecycle events and record them in the event
/// recorder.  This two-step pattern (`take_emitted_events` →
/// `record_thread_events`) is repeated across tool execution paths; the helper
/// keeps call-sites concise and consistent.
pub(super) fn drain_and_record_runtime_events(runtime: &mut AgentRuntime, event_recorder: &mut ExecEventRecorder) {
    let events = runtime.take_emitted_events();
    event_recorder.record_thread_events(events);
}

/// Build a [`ToolFailureContext`] from the common set of parameters that
/// thread through every failure-handling path.  This eliminates the two nearly
/// identical struct-literal constructions in `tool_exec.rs`.
pub(super) fn build_tool_failure_context<'a>(
    agent_prefix: &'a str,
    session_state: &'a mut AgentSessionState,
    event_recorder: &'a mut ExecEventRecorder,
    tool_call_id: &'a str,
    call_item_id: Option<&'a str>,
    is_gemini: bool,
) -> ToolFailureContext<'a> {
    ToolFailureContext {
        agent_prefix,
        session_state,
        event_recorder,
        tool_call_id,
        call_item_id,
        is_gemini,
    }
}
