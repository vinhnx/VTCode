use super::AgentRunner;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::AgentRuntime;
use crate::llm::provider::ToolCall;
use anyhow::Result;

impl AgentRunner {
    pub(super) async fn handle_tool_calls(
        &mut self,
        tool_calls: Vec<ToolCall>,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
        previous_response_chain_present: bool,
    ) -> Result<()> {
        self.execute_tool_call_batches(
            tool_calls,
            runtime,
            event_recorder,
            agent_prefix,
            is_gemini,
            previous_response_chain_present,
        )
        .await
    }
}
