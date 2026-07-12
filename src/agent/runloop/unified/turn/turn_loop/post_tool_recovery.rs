//! Turn-loop helpers for recovering after tool output when the follow-up LLM phase fails.

use anyhow::Result;
use vtcode_commons::ErrorCategory;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::{
    PLANNING_RECOVERY_SYNTHESIS_FALLBACK, POST_TOOL_RECOVERY_REASON, POST_TOOL_RESUME_DIRECTIVE,
    RECOVERY_CONTRACT_VIOLATION_REASON, RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER,
    check_recovery_cycle_cap,
};
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::run_loop_context::HarnessTurnState;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PostToolFailureRecovery {
    NotApplicable,
    RetryToolFree,
    StopAfterDirective,
}

pub(super) fn has_tool_response_since(messages: &[uni::Message], baseline_len: usize) -> bool {
    messages
        .get(baseline_len..)
        .is_some_and(|recent| recent.iter().any(|msg| msg.role == uni::MessageRole::Tool))
}

fn ensure_recent_system_message(working_history: &mut Vec<uni::Message>, content: &str) {
    let already_present = working_history.iter().rev().take(3).any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text() == content
    });
    if already_present {
        return;
    }

    working_history.push(uni::Message::system(content.to_string()));
}

pub(super) fn ensure_post_tool_resume_directive(working_history: &mut Vec<uni::Message>) {
    ensure_recent_system_message(working_history, POST_TOOL_RESUME_DIRECTIVE);
}

pub(crate) fn prepare_post_tool_tool_free_recovery(
    working_history: &mut Vec<uni::Message>,
    reason: &str,
) {
    // Deliberately do NOT push POST_TOOL_RESUME_DIRECTIVE here: it instructs
    // the model to follow tool-output guidance (`next_action`, `fallback_tool`,
    // `rerun_hint`), which contradicts the tool-free recovery contract and
    // encourages emitting tool-call markup (observed in checkpoint turn_621,
    // where three stacked, conflicting system directives preceded a failed
    // synthesis). Only the tools-disabled recovery reason is injected.
    ensure_recent_system_message(working_history, reason);
}

pub(super) fn maybe_recover_after_post_tool_llm_failure(
    renderer: &mut AnsiRenderer,
    working_history: &mut Vec<uni::Message>,
    err: &anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    failure_stage: &'static str,
    allow_tool_free_retry: bool,
) -> Result<PostToolFailureRecovery> {
    let has_partial_tool_progress =
        has_tool_response_since(working_history, turn_history_start_len);
    if !has_partial_tool_progress {
        return Ok(PostToolFailureRecovery::NotApplicable);
    }

    let err_cat = vtcode_commons::classify_anyhow_error(err);
    let transient_hint = if err_cat.is_retryable() {
        " (transient — may resolve on retry)"
    } else {
        ""
    };
    let summary = format!(
        "Tool execution completed, but the model follow-up failed{transient_hint}. Output above is valid.",
    );
    renderer.line(MessageStyle::Info, &summary)?;
    renderer.line(
        MessageStyle::Info,
        &format!("Follow-up error category: {}", err_cat.user_label()),
    )?;
    if !err_cat.is_retryable() {
        renderer.line(
            MessageStyle::Info,
            "Tip: rerun with a narrower prompt or switch provider/model for the follow-up.",
        )?;
    }
    let should_retry = allow_tool_free_retry
        && (err_cat.is_retryable() || matches!(err_cat, ErrorCategory::ExecutionError));
    let action = if should_retry {
        // Tool-free recovery: inject only the tools-disabled recovery reason.
        // The resume directive would contradict it (see
        // `prepare_post_tool_tool_free_recovery`).
        prepare_post_tool_tool_free_recovery(working_history, POST_TOOL_RECOVERY_REASON);
        renderer.line(
            MessageStyle::Info,
            "[!] Follow-up failed after tool execution; scheduling a final tool-free recovery pass.",
        )?;
        PostToolFailureRecovery::RetryToolFree
    } else {
        // Turn ends here; the resume directive guides the *next* turn to
        // reuse this turn's tool outputs instead of re-running exploration.
        ensure_post_tool_resume_directive(working_history);
        PostToolFailureRecovery::StopAfterDirective
    };

    tracing::warn!(
        error = %err,
        step = step_count,
        stage = failure_stage,
        category = ?err_cat,
        retryable = err_cat.is_retryable(),
        recovery_action = ?action,
        "Recovered turn after post-tool LLM phase failure"
    );
    Ok(action)
}

/// Extract file paths from tool responses in the working history.
/// Looks for JSON tool outputs that contain a `path` field, which indicates
/// a file read operation. Returns deduplicated paths.
fn gather_files_read_this_turn(working_history: &[uni::Message]) -> Vec<String> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for msg in working_history.iter() {
        if msg.role != uni::MessageRole::Tool {
            continue;
        }
        let text = msg.content.as_text();
        // Tool outputs are JSON with a `path` field for file reads.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(path) = val.get("path").and_then(serde_json::Value::as_str) {
                if seen.insert(path.to_string()) {
                    files.push(path.to_string());
                }
            }
        }
    }
    files
}

pub(super) fn complete_turn_after_failed_tool_free_recovery(
    working_history: &mut Vec<uni::Message>,
    failure_stage: &str,
    err: Option<&anyhow::Error>,
    salvaged_text: Option<String>,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
) -> TurnLoopResult {
    // Plan mode: never dead-end. Preserve the planning session and re-force
    // the interview on the next turn. Surface the model's salvaged prose if
    // available, otherwise the plan-aware fallback message. The generic
    // "Recovery synthesis failed" message would leave planning stuck.
    if let Some(plan_session) = plan_session {
        plan_session.mark_interview_pending();
        let planning_fallback = salvaged_text
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| PLANNING_RECOVERY_SYNTHESIS_FALLBACK.to_string());
        let has_recent_fallback = working_history.iter().rev().take(3).any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text().starts_with(&planning_fallback)
        });
        if !has_recent_fallback {
            working_history.push(
                uni::Message::assistant(planning_fallback)
                    .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
            );
        }
        tracing::warn!(
            stage = failure_stage,
            "Plan-mode tool-free recovery failed; marking interview pending for next turn."
        );
        return TurnLoopResult::Completed;
    }

    // Prefer prose salvaged from a rejected synthesis response over the
    // canned fallback string: a partially cleaned answer still reflects the
    // tool outputs gathered this turn, while the canned string discards them.
    if let Some(salvaged) = salvaged_text.filter(|text| !text.trim().is_empty()) {
        working_history.push(
            uni::Message::assistant(format!(
                "[!] Recovery synthesis was interrupted; best-effort answer below \
                 (tool-call markup removed):\n\n{salvaged}"
            ))
            .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
        );
        tracing::warn!(
            stage = failure_stage,
            "Tool-free recovery failed; concluding turn with salvaged synthesis prose."
        );
        return TurnLoopResult::Completed;
    }

    // Build a richer fallback that lists files already read, so the next
    // turn can reuse them instead of re-exploring.
    let files_read = gather_files_read_this_turn(working_history);
    let fallback = if files_read.is_empty() {
        RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER.to_string()
    } else {
        format!(
            "{RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER}\n\n\
             Files already read this turn (do NOT re-read):\n{}",
            files_read
                .iter()
                .map(|f| format!("  - {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    let has_recent_fallback = working_history.iter().rev().take(3).any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message
                .content
                .as_text()
                .starts_with(RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER)
    });
    if !has_recent_fallback {
        working_history.push(
            uni::Message::assistant(fallback).with_phase(Some(uni::AssistantPhase::FinalAnswer)),
        );
    }

    if let Some(err) = err {
        tracing::warn!(
            stage = failure_stage,
            error = %err,
            "Final tool-free recovery pass failed; concluding turn with deterministic fallback answer."
        );
    } else {
        tracing::warn!(
            stage = failure_stage,
            "Final tool-free recovery pass failed; concluding turn with deterministic fallback answer."
        );
    }

    TurnLoopResult::Completed
}

pub(super) fn normalize_tool_free_recovery_break_outcome(
    working_history: &mut Vec<uni::Message>,
    outcome_result: TurnLoopResult,
    tool_free_recovery: bool,
    salvaged_text: Option<String>,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
) -> TurnLoopResult {
    let should_fallback = tool_free_recovery
        && matches!(
            outcome_result,
            TurnLoopResult::Blocked {
                reason: Some(ref reason)
            } if reason == RECOVERY_CONTRACT_VIOLATION_REASON
        );

    if should_fallback {
        return complete_turn_after_failed_tool_free_recovery(
            working_history,
            "handle_turn_processing_result.tool_free_recovery_contract_violation",
            None,
            salvaged_text,
            plan_session,
        );
    }

    outcome_result
}

/// Action the turn loop should take after dispatching a post-tool failure.
#[derive(Debug)]
pub(super) enum PostToolFailureAction {
    /// Continue the loop (after RetryToolFree).
    Continue,
    /// Break with the given result (after StopAfterDirective or cycle cap).
    Break(TurnLoopResult),
    /// Fall through to error display and abort (block A only).
    Fallthrough,
}

/// Dispatch the post-tool failure recovery match block, deduplicating the
/// near-identical 3× match in `run_turn_loop`.
///
/// Returns the action the caller should take: continue the loop, break with a
/// result, or fall through to error display.
pub(super) fn dispatch_post_tool_failure(
    renderer: &mut AnsiRenderer,
    working_history: &mut Vec<uni::Message>,
    harness_state: &mut HarnessTurnState,
    err: &anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    stage: &'static str,
    tool_free_recovery: bool,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
) -> Result<PostToolFailureAction> {
    let recovery = maybe_recover_after_post_tool_llm_failure(
        renderer,
        working_history,
        err,
        step_count,
        turn_history_start_len,
        stage,
        !tool_free_recovery,
    )?;

    match recovery {
        PostToolFailureRecovery::NotApplicable => {
            // Block A only: when tool_free_recovery is true and recovery is
            // not applicable, the turn still fails with a deterministic
            // fallback. Blocks B and C never reach this path.
            if tool_free_recovery {
                let salvaged = harness_state.take_recovery_rejected_synthesis();
                let direct_stage = concat_compact(stage, ".direct_tool_free_failure");
                let result = complete_turn_after_failed_tool_free_recovery(
                    working_history,
                    &direct_stage,
                    Some(err),
                    salvaged,
                    plan_session,
                );
                Ok(PostToolFailureAction::Break(result))
            } else {
                Ok(PostToolFailureAction::Fallthrough)
            }
        }
        PostToolFailureRecovery::RetryToolFree => {
            let salvaged = harness_state.take_recovery_rejected_synthesis();
            let cycle_stage = concat_compact(stage, ".recovery_cycle_cap");
            if let Some(r) = check_recovery_cycle_cap(
                harness_state.post_tool_recovery_cycles(),
                working_history,
                &cycle_stage,
                err,
                salvaged,
                plan_session,
            ) {
                return Ok(PostToolFailureAction::Break(r));
            }
            harness_state.increment_post_tool_recovery_cycle();
            harness_state.switch_to_tool_free_recovery();
            Ok(PostToolFailureAction::Continue)
        }
        PostToolFailureRecovery::StopAfterDirective => {
            let result = if tool_free_recovery {
                let salvaged = harness_state.take_recovery_rejected_synthesis();
                let directive_stage = concat_compact(stage, ".stop_after_directive");
                complete_turn_after_failed_tool_free_recovery(
                    working_history,
                    &directive_stage,
                    Some(err),
                    salvaged,
                    plan_session,
                )
            } else {
                TurnLoopResult::Completed
            };
            Ok(PostToolFailureAction::Break(result))
        }
    }
}

/// Concatenate two `&str` into a `String` for composite stage labels.
fn concat_compact(a: &str, b: &str) -> String {
    let mut buf = String::with_capacity(a.len() + b.len());
    buf.push_str(a);
    buf.push_str(b);
    buf
}
