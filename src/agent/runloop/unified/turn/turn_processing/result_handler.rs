use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;
use crate::agent::runloop::unified::turn::tool_outcomes::ToolOutcomeContext;

/// Result of processing a single turn.
pub(crate) struct HandleTurnProcessingResultParams<'a> {
    pub ctx: &'a mut TurnProcessingContext<'a>,
    pub processing_result: TurnProcessingResult,
    pub response_streamed: bool,
    pub step_count: usize,
    pub repeated_tool_attempts:
        &'a mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
    pub turn_modified_files: &'a mut BTreeSet<PathBuf>,
    pub session_end_reason: &'a mut crate::hooks::lifecycle::SessionEndReason,
    /// Pre-computed max tool loops limit for efficiency.
    pub max_tool_loops: usize,
    /// Pre-computed tool repeat limit for efficiency.
    pub tool_repeat_limit: usize,
}

fn is_command_execution_tool_call(tool_call: &uni::ToolCall) -> bool {
    let Some(function) = tool_call.function.as_ref() else {
        return false;
    };
    let tool_name = function.name.as_str();
    let args_val = tool_call
        .parsed_arguments()
        .unwrap_or_else(|_| serde_json::json!({}));

    match tool_name {
        tools::RUN_PTY_CMD | tools::SHELL => true,
        tools::UNIFIED_EXEC | tools::EXEC_PTY_CMD | tools::EXEC => {
            tool_intent::unified_exec_action(&args_val).unwrap_or("run") == "run"
        }
        _ => false,
    }
}

fn should_suppress_pre_tool_result_claim(
    assistant_text: &str,
    tool_calls: &[uni::ToolCall],
) -> bool {
    if assistant_text.trim().is_empty() {
        return false;
    }
    if !tool_calls.iter().any(is_command_execution_tool_call) {
        return false;
    }

    let lower = assistant_text.to_ascii_lowercase();
    [
        "found ",
        "warning",
        "warnings",
        "error",
        "errors",
        "passed",
        "failed",
        "no issues",
        "completed successfully",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// Dispatch the appropriate response handler based on the processing result.
pub(crate) async fn handle_turn_processing_result<'a>(
    params: HandleTurnProcessingResultParams<'a>,
) -> Result<TurnHandlerOutcome> {
    match params.processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
        } => {
            let assistant_text =
                if should_suppress_pre_tool_result_claim(&assistant_text, &tool_calls) {
                    String::new()
                } else {
                    assistant_text
                };
            params.ctx.handle_assistant_response(
                assistant_text,
                reasoning,
                params.response_streamed,
            )?;

            let outcome = {
                let mut t_ctx_inner = ToolOutcomeContext {
                    ctx: &mut *params.ctx,
                    repeated_tool_attempts: &mut *params.repeated_tool_attempts,
                    turn_modified_files: &mut *params.turn_modified_files,
                };

                crate::agent::runloop::unified::turn::tool_outcomes::handle_tool_calls(
                    &mut t_ctx_inner,
                    &tool_calls,
                )
                .await?
            };

            if let Some(res) = outcome {
                return Ok(res);
            }

            Ok(handle_turn_balancer(
                &mut *params.ctx,
                params.step_count,
                &mut *params.repeated_tool_attempts,
                params.max_tool_loops,
                params.tool_repeat_limit,
            )
            .await)
        }
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            proposed_plan,
        } => {
            params
                .ctx
                .handle_text_response(
                    text.clone(),
                    reasoning.clone(),
                    proposed_plan.clone(),
                    params.response_streamed,
                )
                .await
        }
        TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
        }
        TurnProcessingResult::Cancelled => {
            *params.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled))
        }
        TurnProcessingResult::Aborted => Ok(TurnHandlerOutcome::Break(TurnLoopResult::Aborted)),
    }
}

#[cfg(test)]
mod tests {
    use super::should_suppress_pre_tool_result_claim;
    use vtcode_core::llm::provider as uni;

    #[test]
    fn suppresses_result_claims_before_run_tool_output() {
        let tool_calls = vec![uni::ToolCall::function(
            "call_1".to_string(),
            "run_pty_cmd".to_string(),
            r#"{"command":"cargo clippy"}"#.to_string(),
        )];
        assert!(should_suppress_pre_tool_result_claim(
            "Found 3 clippy warnings. Let me fix them.",
            &tool_calls
        ));
    }

    #[test]
    fn keeps_non_result_preamble_for_run_tools() {
        let tool_calls = vec![uni::ToolCall::function(
            "call_1".to_string(),
            "run_pty_cmd".to_string(),
            r#"{"command":"cargo clippy"}"#.to_string(),
        )];
        assert!(!should_suppress_pre_tool_result_claim(
            "Running cargo clippy now.",
            &tool_calls
        ));
    }
}
