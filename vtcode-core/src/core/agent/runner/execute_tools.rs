use super::AgentRunner;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::state::TaskRunState;
use crate::llm::provider::ToolCall;
use anyhow::Result;

impl AgentRunner {
    pub(super) async fn handle_tool_calls(
        &self,
        tool_calls: Vec<ToolCall>,
        task_state: &mut TaskRunState,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<()> {
        let can_parallelize = tool_calls.len() > 1
            && tool_calls.iter().all(|call| {
                if let Some(func) = &call.function {
                    matches!(
                        func.name.as_str(),
                        "list_files" | "read_file" | "grep_file" | "search_tools"
                    )
                } else {
                    false
                }
            });

        if can_parallelize {
            self.execute_parallel_tool_calls(
                tool_calls,
                task_state,
                event_recorder,
                agent_prefix,
                is_gemini,
            )
            .await?;
        } else {
            self.execute_sequential_tool_calls(
                tool_calls,
                task_state,
                event_recorder,
                agent_prefix,
                is_gemini,
            )
            .await?;
        }

        Ok(())
    }
}
