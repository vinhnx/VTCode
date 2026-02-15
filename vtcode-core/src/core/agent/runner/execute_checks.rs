use super::AgentRunner;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::utils::colors::style;
use std::time::Instant;

impl AgentRunner {
    pub(super) fn handle_loop_detection(
        &self,
        response_content: &str,
        agent_prefix: &str,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        turn_started_at: &Instant,
        turn_recorded: &mut bool,
    ) -> bool {
        const LOOP_DETECTED_MESSAGE: &str = "A potential loop was detected";
        if response_content.contains(LOOP_DETECTED_MESSAGE) {
            let warning_message = "Provider halted execution after detecting a potential tool loop";
            self.record_warning(agent_prefix, session_state, event_recorder, warning_message);
            session_state.mark_tool_loop_limit_hit();
            session_state.record_turn(turn_started_at, turn_recorded);
            return true;
        }

        false
    }

    pub(super) fn warn_on_empty_response(
        &self,
        agent_prefix: &str,
        response_content: &str,
        has_tool_calls: bool,
    ) {
        if response_content.trim().is_empty() && !has_tool_calls {
            self.runner_println(format_args!(
                "{} {} received empty response with no tool calls",
                agent_prefix,
                style("(WARN)").red().bold()
            ));
        }
    }
}
