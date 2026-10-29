//! Repeat throttling and fallback-chain execution for runner tool calls.

use super::super::AgentRunner;
use super::super::constants::{LOOP_THROTTLE_BASE_MS, LOOP_THROTTLE_MAX_MS};
use super::super::tool_types::ToolCallItemRef;
use super::outcome::{complete_tool_invocation, record_successful_tool_result};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::harness_kernel::{FallbackRecommendation, project_model_tool_result, reduce_tool_result};
use crate::core::agent::runtime::AgentRuntime;
use crate::exec::events::ToolCallStatus;
use anyhow::Result;
use tokio::time::Duration;
use tracing::{info, warn};

impl AgentRunner {
    pub(super) async fn throttle_repeated_tool(&self, name: &str) {
        let repeat_count = self.loop_detector.lock().get_call_count(name);
        if repeat_count > 1 {
            let delay_ms = (LOOP_THROTTLE_BASE_MS * repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
            if !self.quiet {
                info!(
                    agent = %self.agent_type,
                    tool = %name,
                    repeat_count,
                    delay_ms,
                    "Throttling repeated tool call"
                );
            }
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    pub(super) async fn try_execute_fallback(
        &self,
        fallback: &FallbackRecommendation,
        runtime: &mut AgentRuntime,
        agent_prefix: &str,
        name: &str,
    ) -> Option<(serde_json::Value, String, serde_json::Value)> {
        if !fallback.is_valid() && fallback.chain.is_empty() {
            warn!(
                agent = %agent_prefix,
                tool = %name,
                "Skipping fallback recommendation with empty tool name"
            );
            return None;
        }

        info!(
            agent = %agent_prefix,
            tool = %name,
            fallback_steps = fallback.step_count(),
            "Main tool execution failed; attempting fallback chain"
        );

        for step in fallback.steps() {
            if step.tool_name.trim().is_empty() {
                continue;
            }
            info!(
                agent = %agent_prefix,
                tool = %name,
                fallback_tool = %step.tool_name,
                "Attempting fallback step"
            );

            match self.admit_tool_call(&step.tool_name, step.args.clone(), &mut runtime.state) {
                Ok(fallback_prepared) => match self.execute_prepared_tool_internal(&fallback_prepared).await.result {
                    Ok(res) => {
                        info!(
                            agent = %agent_prefix,
                            fallback_tool = %step.tool_name,
                            "Fallback step succeeded"
                        );
                        return Some((res, step.tool_name, step.args));
                    }
                    Err(fallback_err) => {
                        warn!(
                            agent = %agent_prefix,
                            tool = %step.tool_name,
                            error = %fallback_err.message,
                            "Fallback step failed; trying next"
                        );
                    }
                },
                Err(admit_err) => {
                    warn!(
                        agent = %agent_prefix,
                        tool = %step.tool_name,
                        error = %admit_err,
                        "Failed to admit fallback step; trying next"
                    );
                }
            }
        }

        warn!(
            agent = %agent_prefix,
            tool = %name,
            "All fallback steps exhausted"
        );
        None
    }

    /// Apply the side effects of a successful fallback tool invocation:
    /// reduce the result, push it into the conversation, finalize the
    /// invocation record, and emit the corresponding event recorder
    /// notifications. Shared between the parallel and sequential execution
    /// paths to keep fallback handling behaviorally identical.
    #[allow(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    pub(super) fn apply_fallback_success(
        &self,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        call_id: &str,
        tool_name: &str,
        tool_args: &serde_json::Value,
        result: serde_json::Value,
        tool_call_item: &ToolCallItemRef,
        is_gemini: bool,
    ) -> Result<()> {
        if !self.quiet {
            info!(
                agent = %agent_prefix,
                tool = %tool_name,
                "Fallback tool executed successfully"
            );
        }

        let optimized_result = reduce_tool_result(tool_name, result);

        self.update_last_paths_from_args(tool_name, tool_args, &mut runtime.state);
        // Strip TUI-only display fields before the result enters model context,
        // matching `apply_tool_success`. The TUI event below still uses
        // `optimized_result` (with `view`) so display rendering is unaffected.
        let llm_result = project_model_tool_result(tool_name, tool_args, &optimized_result);
        runtime
            .state
            .push_tool_result(call_id.to_string(), tool_name, &llm_result, is_gemini);
        complete_tool_invocation(
            runtime,
            event_recorder,
            call_id,
            tool_name,
            tool_args,
            tool_call_item,
            ToolCallStatus::Completed,
            None,
        );
        event_recorder.tool_output_started(&tool_call_item.call_item_id, Some(call_id));
        record_successful_tool_result(
            event_recorder,
            &tool_call_item.call_item_id,
            call_id,
            tool_name,
            tool_args,
            &optimized_result,
        );

        Ok(())
    }
}
