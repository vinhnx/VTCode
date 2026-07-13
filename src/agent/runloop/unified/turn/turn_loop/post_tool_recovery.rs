//! Turn-loop helpers for recovering after tool output when the follow-up LLM phase fails.

use anyhow::Result;
use vtcode_commons::ErrorCategory;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::{
    MAX_POST_TOOL_RECOVERY_CYCLES, PLANNING_RECOVERY_SYNTHESIS_FALLBACK, POST_TOOL_RECOVERY_REASON,
    POST_TOOL_RESUME_DIRECTIVE, RECOVERY_CONTRACT_VIOLATION_REASON,
    RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER,
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

/// Build the deterministic recovery fallback, optionally appending the list of
/// files already read this turn so the next turn can reuse them instead of
/// re-exploring. `lead_in` is the provider-agnostic message shown first.
fn build_recovery_fallback(working_history: &[uni::Message], lead_in: &str) -> String {
    let files_read = gather_files_read_this_turn(working_history);
    if files_read.is_empty() {
        lead_in.to_string()
    } else {
        format!(
            "{lead_in}\n\nFiles already read this turn (do NOT re-read):\n{}",
            files_read
                .iter()
                .map(|f| format!("  - {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
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
    // available; otherwise fall back to the plan-aware message. Either way,
    // keep the research already gathered this turn (the files-read list) so
    // nothing useful is lost — the generic dead-end message does this, and
    // plan mode must be at least as informative.
    //
    // EXCEPTION:
    // If the budget is exhausted, do NOT mark interview as pending because no
    // further LLM calls are possible and re-forcing the interview would loop
    // forever. Instead, finalize the plan from gathered evidence.
    //
    // Transient (retryable) errors are intentionally NOT finalized here. The
    // interview-synthesis call now retries internally and falls back to an
    // adaptive interview, so re-forcing the interview on the next turn makes
    // forward progress instead of dead-ending. We keep the planning session
    // alive and preserve the research gathered this turn.
    let is_transient_error = err
        .map(|e| vtcode_commons::classify_anyhow_error(e).is_retryable())
        .unwrap_or(false);
    if let Some(plan_session) = plan_session {
        if plan_session.is_budget_exhausted() || plan_session.is_recovery_exhausted() {
            let finalize_message = if plan_session.is_budget_exhausted() {
                super::PLANNING_BUDGET_EXHAUSTED_FINALIZE
            } else {
                super::PLANNING_RECOVERY_EXHAUSTED_FINALIZE
            };
            let planning_fallback = salvaged_text
                .filter(|text| !text.trim().is_empty())
                .unwrap_or_else(|| build_recovery_fallback(working_history, finalize_message));
            push_final_answer_if_absent(working_history, &planning_fallback);
            tracing::warn!(
                stage = failure_stage,
                budget_exhausted = plan_session.is_budget_exhausted(),
                recovery_exhausted = plan_session.is_recovery_exhausted(),
                "Plan-mode tool-free recovery failed; finalizing plan from gathered evidence."
            );
            return TurnLoopResult::Completed;
        }
        plan_session.mark_interview_pending();
        let planning_fallback = salvaged_text
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| {
                build_recovery_fallback(working_history, PLANNING_RECOVERY_SYNTHESIS_FALLBACK)
            });
        push_final_answer_if_absent(working_history, &planning_fallback);
        tracing::warn!(
            stage = failure_stage,
            transient_error = is_transient_error,
            "Plan-mode tool-free recovery failed; marking interview pending for next turn."
        );
        return TurnLoopResult::Completed;
    }

    // Prefer prose salvaged from a rejected synthesis response over the
    // canned fallback string: a partially cleaned answer still reflects the
    // tool outputs gathered this turn, while the canned string discards them.
    if let Some(salvaged) = salvaged_text.filter(|text| !text.trim().is_empty()) {
        let answer = format!(
            "[!] Recovery synthesis was interrupted; best-effort answer below \
             (tool-call markup removed):\n\n{salvaged}"
        );
        push_final_answer_if_absent(working_history, &answer);
        tracing::warn!(
            stage = failure_stage,
            "Tool-free recovery failed; concluding turn with salvaged synthesis prose."
        );
        return TurnLoopResult::Completed;
    }

    let fallback =
        build_recovery_fallback(working_history, RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER);
    push_final_answer_if_absent(working_history, &fallback);

    tracing::warn!(
        stage = failure_stage,
        error = ?err,
        "Final tool-free recovery pass failed; concluding turn with deterministic fallback answer."
    );

    TurnLoopResult::Completed
}

/// Push an `Assistant` `FinalAnswer` message only if the tail of
/// `working_history` does not already contain the same fallback text, so
/// repeated recovery attempts don't stack duplicate final answers.
fn push_final_answer_if_absent(working_history: &mut Vec<uni::Message>, text: &str) {
    let already_present = working_history.iter().rev().take(3).any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == text
    });
    if !already_present {
        working_history.push(
            uni::Message::assistant(text.to_string())
                .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
        );
    }
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

/// Bundled inputs for post-tool failure recovery. Replaces the nine positional
/// borrows that previously reached directly into the turn context, giving the
/// recovery module a single, stable interface (guard rail) and making it
/// independently testable without the full turn-loop context.
pub(super) struct PostToolRecoveryContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub working_history: &'a mut Vec<uni::Message>,
    pub harness_state: &'a mut HarnessTurnState,
    pub plan_session: Option<&'a mut PlanningWorkflowSessionState>,
    pub err: &'a anyhow::Error,
    pub step_count: usize,
    pub turn_history_start_len: usize,
    pub stage: &'static str,
    pub tool_free_recovery: bool,
}

/// Dispatch the post-tool failure recovery match block, deduplicating the
/// near-identical 3× match in `run_turn_loop`.
///
/// Returns the action the caller should take: continue the loop, break with a
/// result, or fall through to error display.
pub(super) fn dispatch_post_tool_failure(
    ctx: PostToolRecoveryContext<'_>,
) -> Result<PostToolFailureAction> {
    let PostToolRecoveryContext {
        renderer,
        working_history,
        harness_state,
        mut plan_session,
        err,
        step_count,
        turn_history_start_len,
        stage,
        tool_free_recovery,
    } = ctx;
    // Plan-mode: if this turn's tool wall-clock budget was exhausted, the
    // planning context is saturated — the model spent the entire budget on
    // research and the synthesis still failed. Mark the session
    // recovery-exhausted so the failure path below finalizes the plan from
    // gathered evidence instead of re-forcing the interview, which would
    // re-research the still-huge context for another full wall-clock budget
    // and loop forever across turns (observed in checkpoint turn_647).
    // `wall_clock_exhausted()` (time-based) also covers exhaustion without a
    // rejected tool call, e.g. a provider error right after a long tool batch.
    if (harness_state.wall_clock_exhausted_emitted || harness_state.wall_clock_exhausted())
        && let Some(session) = plan_session.as_deref_mut()
    {
        session.mark_recovery_exhausted();
    }
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

/// Shared logic for the `PostToolFailureRecovery::RetryToolFree` arm.
///
/// Checks the post-tool recovery cycle cap. If the cap is reached, completes
/// the turn with a deterministic fallback answer and returns `Some(result)`.
/// Otherwise returns `None`. The caller should increment the cycle counter,
/// switch to tool-free recovery, and `continue` the turn loop.
fn check_recovery_cycle_cap(
    cycles: u8,
    working_history: &mut Vec<uni::Message>,
    stage: &str,
    err: &anyhow::Error,
    salvaged_text: Option<String>,
    mut plan_session: Option<&mut PlanningWorkflowSessionState>,
) -> Option<TurnLoopResult> {
    if cycles >= MAX_POST_TOOL_RECOVERY_CYCLES {
        tracing::warn!(
            cycles,
            "Post-tool recovery cycle cap reached; concluding turn \
             with deterministic fallback answer"
        );
        // In plan mode, repeated tool-free synthesis failures mean the
        // planning context is saturated. Mark the session recovery-exhausted
        // so the next turn does NOT re-force the interview (which would
        // re-research the still-huge context and loop forever). The call
        // below then finalizes the plan from gathered evidence.
        if let Some(plan_session) = plan_session.as_deref_mut() {
            plan_session.mark_recovery_exhausted();
        }
        return Some(complete_turn_after_failed_tool_free_recovery(
            working_history,
            stage,
            Some(err),
            salvaged_text,
            plan_session,
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
    use vtcode_commons::llm::LLMError;

    fn transient_err() -> anyhow::Error {
        anyhow::Error::new(LLMError::Network {
            message: "simulated network blip".to_string(),
            metadata: None,
        })
    }

    #[test]
    fn tool_free_recovery_keeps_planning_alive_on_transient_error() {
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            None,
            Some(&mut plan_session),
        );

        assert!(matches!(result, TurnLoopResult::Completed));
        assert!(
            plan_session.interview_pending(),
            "transient error must keep planning alive by re-forcing the interview"
        );
    }

    #[test]
    fn tool_free_recovery_keeps_planning_alive_on_non_transient_error() {
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        let err = anyhow::Error::new(LLMError::InvalidRequest {
            message: "bad request".to_string(),
            metadata: None,
        });

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&err),
            None,
            Some(&mut plan_session),
        );

        assert!(matches!(result, TurnLoopResult::Completed));
        assert!(
            plan_session.interview_pending(),
            "any tool-free recovery failure must keep planning alive (not dead-end)"
        );
    }

    #[test]
    fn dispatch_marks_recovery_exhausted_when_wall_clock_exhausted_in_plan_mode() {
        use crate::agent::runloop::unified::run_loop_context::{
            HarnessTurnState, TurnId, TurnRunId,
        };
        use vtcode_core::utils::ansi::AnsiRenderer;

        let mut renderer = AnsiRenderer::stdout();
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut harness_state = HarnessTurnState::new(
            TurnRunId("test-run".to_string()),
            TurnId("test-turn".to_string()),
            4,
            600,
            0,
        );
        harness_state.wall_clock_exhausted_emitted = true;
        let mut plan_session = PlanningWorkflowSessionState::default();
        let err = transient_err();

        let action = dispatch_post_tool_failure(PostToolRecoveryContext {
            renderer: &mut renderer,
            working_history: &mut working_history,
            harness_state: &mut harness_state,
            plan_session: Some(&mut plan_session),
            err: &err,
            step_count: 1,
            turn_history_start_len: 0,
            stage: "stage",
            tool_free_recovery: true,
        })
        .expect("dispatch must not error");

        assert!(
            plan_session.is_recovery_exhausted(),
            "wall-clock exhaustion during planning must mark the session \
             recovery-exhausted so the plan finalizes instead of looping"
        );
        assert!(
            !plan_session.interview_pending(),
            "must not re-force the interview after wall-clock exhaustion"
        );
        assert!(matches!(action, PostToolFailureAction::Break(_)));
    }

    #[test]
    fn tool_free_recovery_finalizes_when_budget_exhausted() {
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.mark_budget_exhausted();

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            None,
            Some(&mut plan_session),
        );

        assert!(matches!(result, TurnLoopResult::Completed));
        assert!(
            !plan_session.interview_pending(),
            "budget-exhausted must not re-force the interview (would loop forever)"
        );
        assert!(
            working_history
                .iter()
                .any(|m| m.role == uni::MessageRole::Assistant),
            "budget-exhausted must finalize the plan with a fallback answer"
        );
    }
}
