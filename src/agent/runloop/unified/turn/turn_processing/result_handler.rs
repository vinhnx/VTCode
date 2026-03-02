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
        tools::RUN_PTY_CMD | "shell" => true,
        tools::UNIFIED_EXEC | "exec_pty_cmd" | "exec" => {
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

fn record_assistant_tool_calls(
    history: &mut Vec<uni::Message>,
    tool_calls: &[uni::ToolCall],
    history_len_before_assistant: usize,
) {
    if tool_calls.is_empty() {
        return;
    }

    let appended_assistant_message = history.len() > history_len_before_assistant
        && history.last().is_some_and(|message| {
            message.role == uni::MessageRole::Assistant && message.tool_calls.is_none()
        });

    if appended_assistant_message {
        if let Some(last) = history.last_mut() {
            last.tool_calls = Some(tool_calls.to_vec());
        }
        return;
    }

    // Preserve call/output pairing even when the assistant text was merged into
    // a prior message or omitted; OpenAI-compatible providers require tool call IDs.
    history.push(uni::Message::assistant_with_tools(
        String::new(),
        tool_calls.to_vec(),
    ));
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
            reasoning_details,
        } => {
            let assistant_text =
                if should_suppress_pre_tool_result_claim(&assistant_text, &tool_calls) {
                    String::new()
                } else {
                    assistant_text
                };
            let history_len_before_assistant = params.ctx.working_history.len();
            params.ctx.handle_assistant_response(
                assistant_text,
                reasoning,
                reasoning_details,
                params.response_streamed,
            )?;
            record_assistant_tool_calls(
                params.ctx.working_history,
                &tool_calls,
                history_len_before_assistant,
            );

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
            reasoning_details,
            proposed_plan,
        } => {
            params
                .ctx
                .handle_text_response(
                    text.clone(),
                    reasoning.clone(),
                    reasoning_details.clone(),
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
    use super::{record_assistant_tool_calls, should_suppress_pre_tool_result_claim};
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

    #[test]
    fn records_tool_calls_on_newly_added_assistant_message() {
        let mut history = vec![uni::Message::user("u".to_string())];
        let tool_calls = vec![uni::ToolCall::function(
            "call_1".to_string(),
            "unified_search".to_string(),
            r#"{"action":"grep","pattern":"foo"}"#.to_string(),
        )];

        let len_before_assistant = history.len();
        history.push(uni::Message::assistant("Searching now.".to_string()));

        record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

        assert_eq!(history.len(), 2);
        let last = history.last().expect("assistant message");
        assert_eq!(last.role, uni::MessageRole::Assistant);
        assert_eq!(
            last.tool_calls
                .as_ref()
                .map(|calls| calls[0].id.clone())
                .as_deref(),
            Some("call_1")
        );
    }

    #[test]
    fn appends_tool_call_message_when_no_assistant_message_was_added() {
        let mut history = vec![uni::Message::user("u".to_string())];
        let tool_calls = vec![uni::ToolCall::function(
            "call_1".to_string(),
            "unified_search".to_string(),
            r#"{"action":"grep","pattern":"foo"}"#.to_string(),
        )];

        let len_before_assistant = history.len();
        record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

        assert_eq!(history.len(), 2);
        let last = history
            .last()
            .expect("synthetic assistant tool call message");
        assert_eq!(last.role, uni::MessageRole::Assistant);
        assert_eq!(last.content.as_text(), "");
        assert_eq!(
            last.tool_calls
                .as_ref()
                .map(|calls| calls[0].id.clone())
                .as_deref(),
            Some("call_1")
        );
    }
}
