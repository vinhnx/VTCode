use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;

pub(super) struct ToolFailureContext<'a> {
    pub(super) agent_prefix: &'a str,
    pub(super) session_state: &'a mut AgentSessionState,
    pub(super) event_recorder: &'a mut ExecEventRecorder,
    pub(super) tool_call_id: &'a str,
    pub(super) call_item_id: Option<&'a str>,
    pub(super) is_gemini: bool,
}
