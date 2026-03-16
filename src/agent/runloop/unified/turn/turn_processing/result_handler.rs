use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
    TurnProcessingResult,
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
    /// Pre-computed max tool loops limit for efficiency.
    pub max_tool_loops: usize,
    /// Pre-computed tool repeat limit for efficiency.
    pub tool_repeat_limit: usize,
}

fn should_suppress_pre_tool_result_claim(
    assistant_text: &str,
    tool_calls: &[PreparedAssistantToolCall],
) -> bool {
    if assistant_text.trim().is_empty() {
        return false;
    }
    if !tool_calls
        .iter()
        .any(PreparedAssistantToolCall::is_command_execution)
    {
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
    tool_calls: &[PreparedAssistantToolCall],
    history_len_before_assistant: usize,
) {
    if tool_calls.is_empty() {
        return;
    }

    let raw_tool_calls = tool_calls
        .iter()
        .map(|tool_call| tool_call.raw_call().clone())
        .collect::<Vec<_>>();

    let appended_assistant_message = history.len() > history_len_before_assistant
        && history.last().is_some_and(|message| {
            message.role == uni::MessageRole::Assistant && message.tool_calls.is_none()
        });

    if appended_assistant_message {
        if let Some(last) = history.last_mut() {
            last.tool_calls = Some(raw_tool_calls);
            last.phase = Some(uni::AssistantPhase::Commentary);
        }
        return;
    }

    // Preserve call/output pairing even when the assistant text was merged into
    // a prior message or omitted; OpenAI-compatible providers require tool call IDs.
    history.push(
        uni::Message::assistant_with_tools(String::new(), raw_tool_calls)
            .with_phase(Some(uni::AssistantPhase::Commentary)),
    );
}

fn has_recent_tool_activity(history: &[uni::Message]) -> bool {
    history.iter().rev().take(16).any(|message| {
        message.role == uni::MessageRole::Tool
            || message.tool_call_id.is_some()
            || message.tool_calls.is_some()
    })
}

fn empty_response_recovery_mode(history: &[uni::Message]) -> RecoveryMode {
    if has_recent_tool_activity(history) {
        RecoveryMode::ToolFreeSynthesis
    } else {
        RecoveryMode::ToolEnabledRetry
    }
}

fn empty_response_recovery_reason(mode: RecoveryMode) -> &'static str {
    match mode {
        RecoveryMode::ToolEnabledRetry => {
            "Model returned no answer. Continue autonomously with the next concrete action now. Tools remain available if needed; do not stop with a status update."
        }
        RecoveryMode::ToolFreeSynthesis => {
            "Model returned no answer after tool activity. Tools are disabled on the next pass; provide a direct textual response from the current context."
        }
    }
}

fn empty_response_notice(mode: RecoveryMode) -> &'static str {
    match mode {
        RecoveryMode::ToolEnabledRetry => {
            "[!] Empty model response detected; scheduling a retry pass with tools still enabled."
        }
        RecoveryMode::ToolFreeSynthesis => {
            "[!] Empty model response detected; scheduling a final recovery pass."
        }
    }
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
            if params.ctx.is_recovery_active()
                && params.ctx.recovery_pass_used()
                && params.ctx.recovery_is_tool_free()
            {
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                    reason: Some(
                        "Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls."
                            .to_string(),
                    ),
                }));
            }

            let assistant_text =
                if should_suppress_pre_tool_result_claim(&assistant_text, &tool_calls) {
                    String::new()
                } else {
                    assistant_text
                };
            let assistant_text_len = assistant_text.len();
            let reasoning_segments = reasoning.len();
            let reasoning_details_count = reasoning_details.as_ref().map_or(0, Vec::len);
            let history_len_before_assistant = params.ctx.working_history.len();
            params.ctx.handle_assistant_response(
                assistant_text,
                reasoning,
                reasoning_details,
                params.response_streamed,
                Some(uni::AssistantPhase::Commentary),
            )?;
            record_assistant_tool_calls(
                params.ctx.working_history,
                &tool_calls,
                history_len_before_assistant,
            );
            tracing::info!(
                target: "vtcode.turn.metrics",
                metric = "tool_call_turn_start",
                run_id = %params.ctx.harness_state.run_id.0,
                turn_id = %params.ctx.harness_state.turn_id.0,
                tool_calls = tool_calls.len(),
                assistant_text_len,
                reasoning_segments,
                reasoning_details = reasoning_details_count,
                history_len = params.ctx.working_history.len(),
                "turn metric"
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
                tracing::info!(
                    target: "vtcode.turn.metrics",
                    metric = "tool_call_turn_outcome",
                    run_id = %params.ctx.harness_state.run_id.0,
                    turn_id = %params.ctx.harness_state.turn_id.0,
                    outcome = "direct_break",
                    "turn metric"
                );
                return Ok(res);
            }

            let balancer_outcome = handle_turn_balancer(
                &mut *params.ctx,
                params.step_count,
                &mut *params.repeated_tool_attempts,
                params.max_tool_loops,
                params.tool_repeat_limit,
            )
            .await;
            tracing::info!(
                target: "vtcode.turn.metrics",
                metric = "tool_call_turn_outcome",
                run_id = %params.ctx.harness_state.run_id.0,
                turn_id = %params.ctx.harness_state.turn_id.0,
                outcome = match &balancer_outcome {
                    TurnHandlerOutcome::Continue => "continue",
                    TurnHandlerOutcome::Break(_) => "break",
                },
                "turn metric"
            );
            Ok(balancer_outcome)
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
                    text,
                    reasoning,
                    reasoning_details,
                    proposed_plan,
                    params.response_streamed,
                )
                .await
        }
        TurnProcessingResult::Empty => {
            if params.ctx.is_recovery_active() && params.ctx.recovery_pass_used() {
                let recovery_reason = if params.ctx.recovery_is_tool_free() {
                    "Recovery mode requested a final synthesis pass, but the model returned no answer."
                } else {
                    "Recovery retry requested another autonomous pass, but the model still returned no answer."
                };
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                    reason: Some(recovery_reason.to_string()),
                }));
            }

            let recovery_mode = empty_response_recovery_mode(params.ctx.working_history);
            let recovery_reason = empty_response_recovery_reason(recovery_mode).to_string();
            params
                .ctx
                .activate_recovery_with_mode(recovery_reason.clone(), recovery_mode);
            params
                .ctx
                .renderer
                .line(MessageStyle::Info, empty_response_notice(recovery_mode))
                .unwrap_or(());
            params
                .ctx
                .working_history
                .push(uni::Message::system(recovery_reason));

            Ok(TurnHandlerOutcome::Continue)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        HandleTurnProcessingResultParams, handle_turn_processing_result,
        record_assistant_tool_calls, should_suppress_pre_tool_result_claim,
    };
    use crate::agent::runloop::unified::turn::context::{
        PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingResult,
    };
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    use vtcode_core::llm::provider as uni;

    fn prepared_command_tool_call() -> PreparedAssistantToolCall {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "unified_exec".to_string(),
            r#"{"action":"run","command":"cargo clippy"}"#.to_string(),
        ))
    }

    #[test]
    fn suppresses_result_claims_before_run_tool_output() {
        let tool_calls = vec![prepared_command_tool_call()];
        assert!(should_suppress_pre_tool_result_claim(
            "Found 3 clippy warnings. Let me fix them.",
            &tool_calls
        ));
    }

    #[test]
    fn keeps_non_result_preamble_for_run_tools() {
        let tool_calls = vec![prepared_command_tool_call()];
        assert!(!should_suppress_pre_tool_result_claim(
            "Running cargo clippy now.",
            &tool_calls
        ));
    }

    #[test]
    fn records_tool_calls_on_newly_added_assistant_message() {
        let mut history = vec![uni::Message::user("u".to_string())];
        let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "unified_search".to_string(),
            r#"{"action":"grep","pattern":"foo"}"#.to_string(),
        ))];

        let len_before_assistant = history.len();
        history.push(uni::Message::assistant("Searching now.".to_string()));

        record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

        assert_eq!(history.len(), 2);
        let last = history.last().expect("assistant message");
        assert_eq!(last.role, uni::MessageRole::Assistant);
        assert_eq!(last.phase, Some(uni::AssistantPhase::Commentary));
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
        let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "unified_search".to_string(),
            r#"{"action":"grep","pattern":"foo"}"#.to_string(),
        ))];

        let len_before_assistant = history.len();
        record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

        assert_eq!(history.len(), 2);
        let last = history
            .last()
            .expect("synthetic assistant tool call message");
        assert_eq!(last.role, uni::MessageRole::Assistant);
        assert_eq!(last.content.as_text(), "");
        assert_eq!(last.phase, Some(uni::AssistantPhase::Commentary));
        assert_eq!(
            last.tool_calls
                .as_ref()
                .map(|calls| calls[0].id.clone())
                .as_deref(),
            Some("call_1")
        );
    }

    #[tokio::test]
    async fn recovery_tool_calls_break_turn_as_blocked() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
            "call_1".to_string(),
            "unified_search".to_string(),
            r#"{"action":"grep","pattern":"loop"}"#.to_string(),
        ))];
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::ToolCalls {
                tool_calls,
                assistant_text: String::new(),
                reasoning: Vec::new(),
                reasoning_details: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery tool calls should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
            if reason.contains("tool-free synthesis pass")
        ));
    }

    #[tokio::test]
    async fn recovery_empty_response_breaks_turn_as_blocked() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());

        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::Empty,
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("recovery empty response should be handled");

        assert!(matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
            if reason.contains("returned no answer")
        ));
    }

    #[tokio::test]
    async fn empty_response_schedules_tool_enabled_retry_without_prior_tool_activity() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = {
            let mut ctx = backing.turn_processing_context();
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty,
                response_streamed: true,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("empty response should schedule recovery")
        };

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(!backing.recovery_is_tool_free());
        assert!(backing.last_history_message_contains("Tools remain available"));
    }

    #[tokio::test]
    async fn empty_response_after_tool_activity_schedules_tool_free_recovery() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();

        let outcome = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(
                uni::Message::assistant("Running cargo fmt now.".to_string()).with_tool_calls(
                    vec![uni::ToolCall::function(
                        "call_1".to_string(),
                        "unified_exec".to_string(),
                        r#"{"action":"run","command":"cargo fmt"}"#.to_string(),
                    )],
                ),
            );
            ctx.working_history.push(uni::Message::tool_response(
                "call_1".to_string(),
                "formatted".to_string(),
            ));

            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::Empty,
                response_streamed: true,
                step_count: 1,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
                max_tool_loops: 4,
                tool_repeat_limit: 4,
            })
            .await
            .expect("empty response after tool activity should schedule synthesis recovery")
        };

        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(backing.recovery_is_tool_free());
    }
}
