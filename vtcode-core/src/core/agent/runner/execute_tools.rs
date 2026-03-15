use super::AgentRunner;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::llm::provider::ToolCall;
use crate::tools::tool_intent;
use anyhow::Result;

impl AgentRunner {
    pub(super) async fn handle_tool_calls(
        &mut self,
        tool_calls: Vec<ToolCall>,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
        previous_response_chain_present: bool,
    ) -> Result<()> {
        let can_parallelize = tool_calls.len() > 1
            && tool_calls.iter().all(|call| {
                call.function.as_ref().is_some_and(|func| {
                    call.parsed_arguments()
                        .ok()
                        .as_ref()
                        .is_some_and(|args| tool_intent::is_parallel_safe_call(&func.name, args))
                })
            });

        self.emit_tool_batch(
            &self.get_selected_model(),
            session_state.stats.turns_executed,
            tool_calls.len(),
            can_parallelize,
            previous_response_chain_present,
        );

        if can_parallelize {
            self.execute_parallel_tool_calls(
                tool_calls,
                session_state,
                event_recorder,
                agent_prefix,
                is_gemini,
            )
            .await?;
        } else {
            self.execute_sequential_tool_calls(
                tool_calls,
                session_state,
                event_recorder,
                agent_prefix,
                is_gemini,
            )
            .await?;
        }

        Ok(())
    }
}
