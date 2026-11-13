//! Turn-loop helpers for recovering after tool output when the follow-up LLM phase fails.

use anyhow::Result;
use vtcode_commons::ErrorCategory;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::responses::{
    MAX_POST_TOOL_RECOVERY_CYCLES, PLANNING_BUDGET_EXHAUSTED_NO_DRAFT_NOTICE, PLANNING_BUDGET_EXHAUSTED_USER_NOTICE,
    PLANNING_INTERVIEW_DENIED_NO_DRAFT_NOTICE, PLANNING_RECOVERY_EXHAUSTED_NO_DRAFT_NOTICE,
    PLANNING_RECOVERY_EXHAUSTED_USER_NOTICE, PLANNING_RECOVERY_SYNTHESIS_FALLBACK,
    PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_INTERVIEW, PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_RETRY,
    RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER,
};
use super::{
    POST_TOOL_RECOVERY_REASON, POST_TOOL_RECOVERY_REASON_PLAN_MODE, POST_TOOL_RESUME_DIRECTIVE,
    POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE, RECOVERY_CONTRACT_VIOLATION_REASON,
};
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::plan_blocks::extract_any_plan;
use crate::agent::runloop::unified::planning_workflow::{
    PlanningWorkflowState, emit_plan_ready_events, persist_plan_draft, persisted_plan_is_ready, validate_plan_content,
};
use crate::agent::runloop::unified::planning_workflow_state::{
    PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT, PlanningWorkflowSessionState, short_confirmation_hint,
};
use crate::agent::runloop::unified::run_loop_context::HarnessTurnState;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_processing::is_unmatched_tool_result_error;

mod failure;
mod fallback;
use fallback::{build_recovery_fallback, plan_mode_recovery_fallback};

pub(super) use failure::{
    PlanRecoveryEventContext, PostToolFailureRecovery, has_tool_response_since, is_misconfiguration_error,
    is_provider_context_capacity_failure, is_transient_recovery_error, misconfiguration_guidance,
};

const POST_TOOL_TOOL_ENABLED_RETRY_FAILED_REASON: &str = "Post-tool recovery could not confirm the requested work after one bounded tool-enabled retry. The completed tool outputs and resume handoff were retained; retry from the pending step.";
const UNMATCHED_TOOL_RESULT_BLOCK_REASON: &str = "Provider rejected an unmatched tool result after one bounded request-history repair. The turn is blocked with a resumable handoff.";
pub(super) const POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON: &str = "The provider rejected the follow-up because the context exceeded its capacity, and the bounded recovery compaction could not reduce the request. Completed tool outputs were retained; retry after reducing context or switching model.";
const MAX_PLANNING_SYNTHESIS_RECOVERY_RETRIES: u8 = 1;

fn planning_synthesis_retry_allowed(
    tool_free_recovery: bool,
    planning_active: bool,
    err: &anyhow::Error,
    plan_session: Option<&PlanningWorkflowSessionState>,
    harness_state: &HarnessTurnState,
) -> bool {
    tool_free_recovery
        && planning_active
        && vtcode_commons::classify_anyhow_error(err).is_retryable()
        && !is_misconfiguration_error(err)
        && harness_state.recovery_retry_count() < MAX_PLANNING_SYNTHESIS_RECOVERY_RETRIES
        && !harness_state.wall_clock_exhausted_emitted
        && !harness_state.wall_clock_exhausted()
        && plan_session.is_some_and(|session| !session.is_budget_exhausted() && !session.is_recovery_exhausted())
}

fn ensure_recent_system_message(working_history: &mut Vec<uni::Message>, content: &str) {
    let already_present = working_history
        .iter()
        .rev()
        .take(3)
        .any(|message| message.role == uni::MessageRole::System && message.content.as_text() == content);
    if already_present {
        return;
    }

    working_history.push(uni::Message::system(content.to_string()));
}

fn ready_plan_text(text: &str) -> Option<String> {
    let extraction = extract_any_plan(text);
    if let Some(plan_text) = extraction.plan_text {
        return validate_plan_content(&plan_text).is_ready().then_some(plan_text);
    }

    validate_plan_content(text).is_ready().then(|| text.trim().to_string())
}

pub(super) fn ensure_post_tool_resume_directive(working_history: &mut Vec<uni::Message>) {
    ensure_recent_system_message(working_history, POST_TOOL_RESUME_DIRECTIVE);
}

pub(crate) fn prepare_post_tool_tool_free_recovery(working_history: &mut Vec<uni::Message>, reason: &str) {
    // Deliberately do NOT push POST_TOOL_RESUME_DIRECTIVE here: it instructs
    // the model to follow tool-output guidance (`next_action`, `fallback_tool`,
    // `rerun_hint`), which contradicts the tool-free recovery contract and
    // encourages emitting tool-call markup (observed in checkpoint turn_621,
    // where three stacked, conflicting system directives preceded a failed
    // synthesis). Only the tools-disabled recovery reason is injected.
    ensure_recent_system_message(working_history, reason);
}

#[cfg(test)]
pub(super) fn maybe_recover_after_post_tool_llm_failure(
    renderer: &mut AnsiRenderer,
    working_history: &mut Vec<uni::Message>,
    err: &anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    failure_stage: &'static str,
    allow_tool_free_retry: bool,
    allow_tool_enabled_retry: bool,
    planning_active: bool,
) -> Result<PostToolFailureRecovery> {
    maybe_recover_after_post_tool_llm_failure_with_progress(PostToolLlmRecoveryInputs {
        renderer,
        working_history,
        err,
        step_count,
        turn_history_start_len,
        failure_stage,
        allow_tool_free_retry,
        allow_tool_enabled_retry,
        planning_active,
        out_of_band_tool_progress: false,
    })
}

/// Bundled inputs for the post-tool LLM-failure recovery decision. Follows the
/// `PostToolRecoveryContext` guard-rail pattern: one named struct instead of
/// ten positional parameters at the call sites.
struct PostToolLlmRecoveryInputs<'a> {
    renderer: &'a mut AnsiRenderer,
    working_history: &'a mut Vec<uni::Message>,
    err: &'a anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    failure_stage: &'static str,
    allow_tool_free_retry: bool,
    allow_tool_enabled_retry: bool,
    planning_active: bool,
    out_of_band_tool_progress: bool,
}

fn maybe_recover_after_post_tool_llm_failure_with_progress(
    PostToolLlmRecoveryInputs {
        renderer,
        working_history,
        err,
        step_count,
        turn_history_start_len,
        failure_stage,
        allow_tool_free_retry,
        allow_tool_enabled_retry,
        planning_active,
        out_of_band_tool_progress,
    }: PostToolLlmRecoveryInputs<'_>,
) -> Result<PostToolFailureRecovery> {
    let config_guidance = misconfiguration_guidance(err);

    if is_unmatched_tool_result_error(&err.to_string()) {
        // A repaired retry has already been attempted, or the request was
        // already clean. Preserve the existing resume handoff and do not
        // schedule another provider or tool-free retry with the same wire
        // shape.
        ensure_post_tool_resume_directive(working_history);
        if let Some(guidance) = config_guidance.as_ref() {
            renderer.line(MessageStyle::Warning, &guidance.user_message())?;
        }
        renderer.line(
            MessageStyle::Warning,
            "The provider rejected tool history after a bounded repair check; the turn is paused for resume.",
        )?;
        return Ok(PostToolFailureRecovery::StopAfterDirective);
    }

    let has_partial_tool_progress =
        out_of_band_tool_progress || has_tool_response_since(working_history, turn_history_start_len);
    if !has_partial_tool_progress {
        return Ok(PostToolFailureRecovery::NotApplicable);
    }

    // If we are in planning mode and the assistant has already successfully written
    // a non-empty text response (e.g. a plan draft) during this turn before the LLM
    // call failed (e.g. streaming disconnected at the very end), do not trigger recovery.
    // Completing the turn preserves the generated plan for the user to confirm/edit.
    let has_plan_in_history = planning_active
        && working_history.get(turn_history_start_len..).is_some_and(|recent| {
            recent
                .iter()
                .any(|msg| msg.role == uni::MessageRole::Assistant && ready_plan_text(&msg.content.as_text()).is_some())
        });
    if has_plan_in_history {
        return Ok(PostToolFailureRecovery::StopAfterDirective);
    }

    let err_cat = vtcode_commons::classify_anyhow_error(err);
    let misconfiguration = is_misconfiguration_error(err);
    let context_capacity_failure = is_provider_context_capacity_failure(failure_stage, err);
    let retry_scheduled = allow_tool_free_retry || allow_tool_enabled_retry;
    let transient_hint = if err_cat.is_retryable() && !misconfiguration && !context_capacity_failure {
        if retry_scheduled {
            " (transient; bounded retry scheduled)"
        } else {
            " (transient; retry budget exhausted)"
        }
    } else {
        ""
    };
    let summary = match config_guidance.as_ref() {
        Some(guidance) => format!("{} {}", follow_up_failure_status_line(transient_hint), guidance.user_message()),
        None => follow_up_failure_status_line(transient_hint),
    };
    let should_retry_tool_enabled =
        !misconfiguration && allow_tool_enabled_retry && (err_cat.is_retryable() || context_capacity_failure);
    let should_retry_tool_free = !misconfiguration
        && allow_tool_free_retry
        && (err_cat.is_retryable() || matches!(err_cat, ErrorCategory::ExecutionError));
    // Strict 2-line TUI diagnostic contract: status + one action. Category and
    // tip text fold into the action or drop; forensics stay out of the
    // transcript.
    renderer.line(MessageStyle::Info, &summary)?;
    if should_retry_tool_enabled {
        ensure_recent_system_message(working_history, POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE);
    } else if should_retry_tool_free {
        // Tool-free recovery: inject only the tools-disabled recovery reason.
        // The resume directive would contradict it (see
        // `prepare_post_tool_tool_free_recovery`). In plan mode use the
        // plan-aware reason so the model finalizes the `<proposed_plan>` from
        // gathered research instead of re-attempting tool calls.
        let reason = if planning_active {
            POST_TOOL_RECOVERY_REASON_PLAN_MODE
        } else {
            POST_TOOL_RECOVERY_REASON
        };
        prepare_post_tool_tool_free_recovery(working_history, reason);
    } else {
        // Turn ends here; the resume directive guides the *next* turn to
        // reuse this turn's tool outputs instead of re-running exploration.
        ensure_post_tool_resume_directive(working_history);
    }
    let action = if should_retry_tool_enabled {
        PostToolFailureRecovery::RetryToolEnabled
    } else if should_retry_tool_free {
        PostToolFailureRecovery::RetryToolFree
    } else {
        PostToolFailureRecovery::StopAfterDirective
    };
    let action_text = follow_up_failure_action_line(
        action,
        context_capacity_failure,
        planning_active,
        !misconfiguration && !err_cat.is_retryable(),
    );
    renderer.line(MessageStyle::Info, &format!("  • {action_text}"))?;

    tracing::warn!(
        error = %err,
        step = step_count,
        stage = failure_stage,
        category = ?err_cat,
        retryable = err_cat.is_retryable() && !misconfiguration,
        context_capacity_failure,
        recovery_action = ?action,
        "Recovered turn after post-tool LLM phase failure"
    );
    Ok(action)
}

/// Status line for a post-tool model follow-up failure (2-line diagnostic contract).
fn follow_up_failure_status_line(transient_hint: &str) -> String {
    format!("Tool execution completed, but the model follow-up failed{transient_hint}. Output above is valid.")
}

/// Action line body (no bullet prefix) for a post-tool follow-up failure.
/// Pure so the 2-line TUI contract is unit-testable without a renderer.
/// `tip_eligible` is `!misconfiguration && !err_retryable` at the call site.
fn follow_up_failure_action_line(
    recovery: PostToolFailureRecovery,
    context_capacity_failure: bool,
    planning_active: bool,
    tip_eligible: bool,
) -> &'static str {
    match recovery {
        PostToolFailureRecovery::RetryToolEnabled => {
            if context_capacity_failure {
                "Context capacity exceeded; compacting and scheduling one tool-enabled recovery pass."
            } else {
                "Scheduling one tool-enabled recovery pass."
            }
        }
        PostToolFailureRecovery::RetryToolFree => "Scheduling a final tool-free recovery pass.",
        PostToolFailureRecovery::StopAfterDirective if tip_eligible => {
            if planning_active {
                "Planning evidence is preserved; type `keep planning` to continue, or switch provider/model if this repeats."
            } else {
                "Retry with a narrower prompt or switch provider/model."
            }
        }
        PostToolFailureRecovery::StopAfterDirective | PostToolFailureRecovery::NotApplicable => {
            "Type 'continue' to resume with retained tool outputs."
        }
    }
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

/// The exhaustion state of the planning session that determines which
/// final-answer notice the recovery path emits. Extracted from the nested
/// if/else chain that previously lived inline in
/// `complete_turn_after_failed_tool_free_recovery_with_events` so all six
/// combinations are independently testable without async harness state
/// (checkpoint turn_902 was caused by the wrong combination firing — the
/// ready-draft prompt was shown when no draft existed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlanningRecoveryOutcome {
    BudgetExhausted { plan_ready: bool },
    RecoveryExhausted { plan_ready: bool },
    InterviewDenied { plan_ready: bool },
}

impl PlanningRecoveryOutcome {
    /// Build the outcome from the live session state and readiness flag.
    /// Reads only (`&self` methods on the session) — no mutation, no I/O.
    fn from_session(session: &PlanningWorkflowSessionState, plan_ready: bool) -> Self {
        if session.is_budget_exhausted() {
            Self::BudgetExhausted { plan_ready }
        } else if session.is_recovery_exhausted() {
            Self::RecoveryExhausted { plan_ready }
        } else {
            Self::InterviewDenied { plan_ready }
        }
    }
}

/// Pure mapping from a planning-recovery outcome to the user-facing notice.
/// No side effects, no async, no I/O — the message policy lives here so the
/// async orchestration function only decides *whether* to finalize, not *what*
/// message to show. The critical invariant: a ready-draft prompt (promising
/// "Review the plan below" with `yes`/`implement`/`no`/`edit` choices) is
/// returned ONLY when `plan_ready == true`. When no draft was persisted the
/// no-draft variants are used instead, which are consistent with the appended
/// `PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT`.
fn planning_finalize_notice(outcome: PlanningRecoveryOutcome) -> &'static str {
    match outcome {
        PlanningRecoveryOutcome::BudgetExhausted { plan_ready: true } => PLANNING_BUDGET_EXHAUSTED_USER_NOTICE,
        PlanningRecoveryOutcome::BudgetExhausted { plan_ready: false } => PLANNING_BUDGET_EXHAUSTED_NO_DRAFT_NOTICE,
        PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: true } => PLANNING_RECOVERY_EXHAUSTED_USER_NOTICE,
        PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: false } => PLANNING_RECOVERY_EXHAUSTED_NO_DRAFT_NOTICE,
        PlanningRecoveryOutcome::InterviewDenied { plan_ready: true } => {
            PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_INTERVIEW
        }
        PlanningRecoveryOutcome::InterviewDenied { plan_ready: false } => PLANNING_INTERVIEW_DENIED_NO_DRAFT_NOTICE,
    }
}

/// Outcome of the best-effort salvaged-plan persistence in plan-mode recovery.
struct SalvagedPlanPersistence {
    /// The salvaged prose, kept only when persistence passed the readiness gate.
    persisted_salvage: Option<String>,
    /// The recovered `<proposed_plan>` text, when one was persisted.
    recovered_plan_text: Option<String>,
    /// Whether the session plan file holds a validated, ready draft.
    persisted_plan_ready: bool,
}

/// Best-effort persistence of a salvaged `<proposed_plan>`: write failures are
/// logged and never dead-end the turn, and the salvaged prose is only surfaced
/// when persistence passed the readiness gate.
async fn persist_salvaged_plan(
    plan_state: Option<&PlanningWorkflowState>,
    salvaged_text: Option<&str>,
) -> SalvagedPlanPersistence {
    let mut persisted_salvage = None;
    let mut recovered_plan_text = None;
    if let (Some(state), Some(salvaged)) = (plan_state, salvaged_text)
        && let Some(plan_text) = ready_plan_text(salvaged)
    {
        match persist_plan_draft(state, &plan_text).await {
            Ok(persisted) if persisted.validation.is_ready() && persisted_plan_is_ready(state).await => {
                persisted_salvage = Some(plan_text.clone());
                recovered_plan_text = Some(plan_text);
            }
            Ok(_) => {
                tracing::warn!("plan-mode recovery: persisted salvage failed the readiness gate");
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "plan-mode recovery: failed to persist salvaged plan to session plan file"
                );
            }
        }
    }
    let persisted_plan_ready = if let Some(state) = plan_state {
        persisted_plan_is_ready(state).await
    } else {
        false
    };
    SalvagedPlanPersistence {
        persisted_salvage,
        recovered_plan_text,
        persisted_plan_ready,
    }
}

#[cfg(test)]
pub(super) async fn complete_turn_after_failed_tool_free_recovery(
    working_history: &mut Vec<uni::Message>,
    failure_stage: &str,
    err: Option<&anyhow::Error>,
    salvaged_text: Option<String>,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
    plan_state: Option<&PlanningWorkflowState>,
) -> TurnLoopResult {
    complete_turn_after_failed_tool_free_recovery_with_events(
        working_history,
        failure_stage,
        err,
        salvaged_text,
        plan_session,
        plan_state,
        None,
        None,
    )
    .await
}

pub(super) async fn complete_turn_after_failed_tool_free_recovery_with_events(
    working_history: &mut Vec<uni::Message>,
    failure_stage: &str,
    err: Option<&anyhow::Error>,
    salvaged_text: Option<String>,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
    plan_state: Option<&PlanningWorkflowState>,
    events: Option<PlanRecoveryEventContext<'_>>,
    mut harness_state: Option<&mut HarnessTurnState>,
) -> TurnLoopResult {
    // This helper always concludes with a deterministic fallback answer, never
    // a confirmed model synthesis. Mark the fallback flag explicitly so the
    // outer turn loop converts the `Completed` into a resumable `Blocked`
    // without relying on the `!rendered` heuristic in
    // `ensure_completed_turn_response` (which mislabels valid streamed model
    // answers that simply missed the render flag as fallbacks).
    if let Some(state) = harness_state.as_mut() {
        state.mark_final_response_fallback();
    }
    // In plan mode, the recovery salvage (the inline `<proposed_plan>` the model
    // produced) must be persisted to the session plan file even though tools
    // were disabled during the tool-free recovery pass. Otherwise the plan
    // exists only in chat history while the user-facing notices promise it is
    // "preserved in the session plan file (.vtcode/plans/)". Best-effort: a
    // write failure is logged and must never dead-end the turn.
    let mut plan_session = plan_session;
    let SalvagedPlanPersistence {
        persisted_salvage,
        recovered_plan_text,
        persisted_plan_ready,
    } = persist_salvaged_plan(plan_state, salvaged_text.as_deref()).await;

    if let Some(event_context) = events
        && let (Some(session), Some(state), Some(plan_text)) =
            (plan_session.as_deref_mut(), plan_state, recovered_plan_text.as_deref())
    {
        if let Err(error) = emit_plan_ready_events(
            session,
            state,
            event_context.emitter,
            event_context.thread_id,
            event_context.turn_id,
            plan_text,
        )
        .await
        {
            tracing::warn!(%error, "failed to persist planning approval state");
            session.recovery.outcome = vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput;
            return TurnLoopResult::Blocked {
                reason: Some("planning persistence failed; awaiting input".into()),
            };
        }
    }

    // Plan mode: never dead-end. Preserve the planning session and re-force
    // the interview on the next turn (unless budget/recovery is exhausted).
    // A rejected synthesis usually leaves only a garbled recovery monologue
    // with tool-call markup stripped out — not a plan. We therefore only
    // surface the salvaged prose when it actually contains a `<proposed_plan>`
    // (a real, if partial, plan); otherwise we fall back to the structured
    // plan-aware message, which still lists the files read this turn, so we
    // never inject garbage as the proposed plan. See `plan_mode_recovery_fallback`.
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
    let is_transient_error = err.is_some_and(is_transient_recovery_error);
    if let Some(plan_session) = plan_session {
        if plan_session.is_budget_exhausted()
            || plan_session.is_recovery_exhausted()
            || plan_session.is_interview_denied()
        {
            // NOTE: use the USER-facing notices here, not the `*_FINALIZE`
            // model directives. No LLM call follows this path, so a model
            // directive pushed as the final answer just shows the user a bare
            // instruction and dead-ends the turn (checkpoint turn_655). The
            // planning session stays alive, so append the confirmation hint —
            // the user can type `implement` to execute the drafted plan or
            // `keep planning` to revise it.
            // Select the user-facing notice through the pure policy function
            // so all six (exhaustion × draft-ready) combinations are
            // independently testable and the invariant — a ready-draft prompt
            // is shown ONLY when a plan was actually persisted — is enforced
            // in one place (turn_902).
            let outcome = PlanningRecoveryOutcome::from_session(plan_session, persisted_plan_ready);
            let finalize_message = planning_finalize_notice(outcome);
            let mut planning_fallback =
                plan_mode_recovery_fallback(persisted_salvage, finalize_message, working_history);
            planning_fallback.push_str("\n\n");
            if persisted_plan_ready {
                planning_fallback.push_str(short_confirmation_hint());
            } else {
                planning_fallback.push_str(PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT);
            }
            push_final_answer_if_absent(working_history, &planning_fallback);
            tracing::warn!(
                stage = failure_stage,
                budget_exhausted = plan_session.is_budget_exhausted(),
                recovery_exhausted = plan_session.is_recovery_exhausted(),
                interview_denied = plan_session.is_interview_denied(),
                "Plan-mode tool-free recovery failed; finalizing plan from gathered evidence."
            );
            return TurnLoopResult::Completed { plan_approved_execution_pending: false };
        }
        if recovered_plan_text.is_some() && persisted_plan_ready {
            plan_session.clear_interview_pending();
        } else {
            plan_session.mark_interview_pending();
        }
        let fallback_notice = if is_transient_error {
            PLANNING_RECOVERY_SYNTHESIS_FALLBACK
        } else {
            PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_RETRY
        };
        let mut planning_fallback = plan_mode_recovery_fallback(persisted_salvage, fallback_notice, working_history);
        planning_fallback.push_str("\n\n");
        if persisted_plan_ready {
            planning_fallback.push_str(short_confirmation_hint());
        } else {
            planning_fallback.push_str(PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT);
        }
        push_final_answer_if_absent(working_history, &planning_fallback);
        tracing::warn!(
            stage = failure_stage,
            transient_error = is_transient_error,
            "Plan-mode tool-free recovery failed; marking interview pending for next turn."
        );
        return TurnLoopResult::Completed { plan_approved_execution_pending: false };
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
        return TurnLoopResult::Completed { plan_approved_execution_pending: false };
    }

    let fallback = build_recovery_fallback(working_history, RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER);
    push_final_answer_if_absent(working_history, &fallback);

    tracing::warn!(
        stage = failure_stage,
        error = ?err,
        "Final tool-free recovery pass failed; concluding turn with deterministic fallback answer."
    );

    TurnLoopResult::Completed { plan_approved_execution_pending: false }
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
        working_history
            .push(uni::Message::assistant(text.to_string()).with_phase(Some(uni::AssistantPhase::FinalAnswer)));
    }
}

#[cfg(test)]
pub(super) async fn normalize_tool_free_recovery_break_outcome(
    working_history: &mut Vec<uni::Message>,
    outcome_result: TurnLoopResult,
    tool_free_recovery: bool,
    salvaged_text: Option<String>,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
    plan_state: Option<&PlanningWorkflowState>,
) -> TurnLoopResult {
    normalize_tool_free_recovery_break_outcome_with_events(
        working_history,
        outcome_result,
        tool_free_recovery,
        salvaged_text,
        plan_session,
        plan_state,
        None,
        None,
    )
    .await
}

pub(super) async fn normalize_tool_free_recovery_break_outcome_with_events(
    working_history: &mut Vec<uni::Message>,
    outcome_result: TurnLoopResult,
    tool_free_recovery: bool,
    salvaged_text: Option<String>,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
    plan_state: Option<&PlanningWorkflowState>,
    events: Option<PlanRecoveryEventContext<'_>>,
    harness_state: Option<&mut HarnessTurnState>,
) -> TurnLoopResult {
    let should_fallback = tool_free_recovery
        && matches!(
            outcome_result,
            TurnLoopResult::Blocked {
                reason: Some(ref reason)
            } if reason == RECOVERY_CONTRACT_VIOLATION_REASON
        );

    if should_fallback {
        return complete_turn_after_failed_tool_free_recovery_with_events(
            working_history,
            "handle_turn_processing_result.tool_free_recovery_contract_violation",
            None,
            salvaged_text,
            plan_session,
            plan_state,
            events,
            harness_state,
        )
        .await;
    }

    outcome_result
}

/// Action the turn loop should take after dispatching a post-tool failure.
#[derive(Debug)]
pub(super) enum PostToolFailureAction {
    /// Continue the loop for a bounded recovery retry.
    Continue,
    /// Break with the given result after recovery is exhausted or a cycle cap.
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
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub plan_session: Option<&'a mut PlanningWorkflowSessionState>,
    pub plan_state: Option<&'a PlanningWorkflowState>,
    pub err: &'a anyhow::Error,
    pub step_count: usize,
    pub turn_history_start_len: usize,
    pub stage: &'static str,
    pub tool_free_recovery: bool,
}

/// Shared tail of the tool-free salvage paths: compose the stage label and
/// hand the rejected synthesis to `complete_turn_after_failed_tool_free_recovery_with_events`
/// with plan persistence and recovery events attached.
async fn complete_turn_with_salvage(
    working_history: &mut Vec<uni::Message>,
    stage: &str,
    stage_suffix: &str,
    err: Option<&anyhow::Error>,
    salvaged: Option<String>,
    plan_session: Option<&mut PlanningWorkflowSessionState>,
    plan_state: Option<&PlanningWorkflowState>,
    event_context: PlanRecoveryEventContext<'_>,
    harness_state: Option<&mut HarnessTurnState>,
) -> TurnLoopResult {
    let composite_stage = concat_compact(stage, stage_suffix);
    complete_turn_after_failed_tool_free_recovery_with_events(
        working_history,
        &composite_stage,
        err,
        salvaged,
        plan_session,
        plan_state,
        Some(event_context),
        harness_state,
    )
    .await
}

/// Dispatch the post-tool failure recovery match block, deduplicating the
/// near-identical 3× match in `run_turn_loop`.
///
/// Returns the action the caller should take: continue the loop, break with a
/// result, or fall through to error display.
pub(super) async fn dispatch_post_tool_failure(ctx: PostToolRecoveryContext<'_>) -> Result<PostToolFailureAction> {
    let PostToolRecoveryContext {
        renderer,
        working_history,
        harness_state,
        harness_emitter,
        mut plan_session,
        plan_state,
        err,
        step_count,
        turn_history_start_len,
        stage,
        tool_free_recovery,
    } = ctx;
    let event_thread_id = harness_state.run_id.0.clone();
    let event_turn_id = harness_state.turn_id.0.clone();
    let event_context = PlanRecoveryEventContext {
        emitter: harness_emitter,
        thread_id: &event_thread_id,
        turn_id: &event_turn_id,
    };
    if is_unmatched_tool_result_error(&err.to_string()) {
        ensure_post_tool_resume_directive(working_history);
        renderer.line(
            MessageStyle::Warning,
            "The provider rejected tool history after a bounded repair check; the turn is blocked for resume.",
        )?;
        return Ok(PostToolFailureAction::Break(TurnLoopResult::Blocked {
            reason: Some(UNMATCHED_TOOL_RESULT_BLOCK_REASON.to_string()),
        }));
    }
    let planning_active = plan_session.is_some();
    let context_capacity_failure = is_provider_context_capacity_failure(stage, err);
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
    let post_tool_retry_exhausted =
        !tool_free_recovery && harness_state.post_tool_tool_enabled_retry_used() && harness_state.recovery_pass_used();
    let planning_synthesis_retry = planning_synthesis_retry_allowed(
        tool_free_recovery,
        planning_active,
        err,
        plan_session.as_deref(),
        harness_state,
    );
    let recovery = maybe_recover_after_post_tool_llm_failure_with_progress(PostToolLlmRecoveryInputs {
        renderer: &mut *renderer,
        working_history: &mut *working_history,
        err,
        step_count,
        turn_history_start_len,
        failure_stage: stage,
        allow_tool_free_retry: (!tool_free_recovery && !post_tool_retry_exhausted) || planning_synthesis_retry,
        allow_tool_enabled_retry: !tool_free_recovery
            && !harness_state.post_tool_tool_enabled_retry_used()
            && !harness_state.recovery_pass_used(),
        planning_active,
        out_of_band_tool_progress: harness_state.has_out_of_band_tool_progress(),
    })?;

    match recovery {
        PostToolFailureRecovery::NotApplicable => {
            // Block A only: when tool_free_recovery is true and recovery is
            // not applicable, the turn still fails with a deterministic
            // fallback. Blocks B and C never reach this path.
            if tool_free_recovery {
                let salvaged = harness_state.take_recovery_rejected_synthesis();
                let result = complete_turn_with_salvage(
                    working_history,
                    stage,
                    ".direct_tool_free_failure",
                    Some(err),
                    salvaged,
                    plan_session,
                    plan_state,
                    event_context,
                    Some(&mut *harness_state),
                )
                .await;
                Ok(PostToolFailureAction::Break(result))
            } else {
                Ok(PostToolFailureAction::Fallthrough)
            }
        }
        PostToolFailureRecovery::RetryToolEnabled => {
            if harness_state.post_tool_tool_enabled_retry_used()
                || !harness_state
                    .arm_post_tool_tool_enabled_retry("post-tool follow-up failure", context_capacity_failure)
            {
                ensure_post_tool_resume_directive(working_history);
                return Ok(PostToolFailureAction::Break(TurnLoopResult::Blocked {
                    reason: Some(if context_capacity_failure {
                        POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON.to_string()
                    } else {
                        POST_TOOL_TOOL_ENABLED_RETRY_FAILED_REASON.to_string()
                    }),
                }));
            }
            Ok(PostToolFailureAction::Continue)
        }
        PostToolFailureRecovery::RetryToolFree => {
            if planning_synthesis_retry && harness_state.retry_recovery_pass() {
                prepare_post_tool_tool_free_recovery(working_history, POST_TOOL_RECOVERY_REASON_PLAN_MODE);
                renderer.line(
                    MessageStyle::Warning,
                    "[!] Final plan synthesis failed transiently; retrying once from the gathered research with tools disabled.",
                )?;
                tracing::warn!(
                    retry = harness_state.recovery_retry_count(),
                    max_retries = MAX_PLANNING_SYNTHESIS_RECOVERY_RETRIES,
                    "Retrying transient plan synthesis failure without re-enabling tools"
                );
                return Ok(PostToolFailureAction::Continue);
            }
            let salvaged = harness_state.take_recovery_rejected_synthesis();
            let cycle_stage = concat_compact(stage, ".recovery_cycle_cap");
            if let Some(r) = check_recovery_cycle_cap(
                harness_state.post_tool_recovery_cycles(),
                working_history,
                &cycle_stage,
                err,
                salvaged,
                plan_session,
                plan_state,
                Some(event_context),
                Some(&mut *harness_state),
            )
            .await
            {
                return Ok(PostToolFailureAction::Break(r));
            }
            harness_state.increment_post_tool_recovery_cycle();
            harness_state.switch_to_tool_free_recovery();
            Ok(PostToolFailureAction::Continue)
        }
        PostToolFailureRecovery::StopAfterDirective => {
            let tool_enabled_retry_exhausted = !tool_free_recovery
                && harness_state.post_tool_tool_enabled_retry_used()
                && harness_state.recovery_pass_used();
            let result = if tool_enabled_retry_exhausted {
                TurnLoopResult::Blocked {
                    reason: Some(if harness_state.post_tool_context_compaction_failed() {
                        POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON.to_string()
                    } else {
                        POST_TOOL_TOOL_ENABLED_RETRY_FAILED_REASON.to_string()
                    }),
                }
            } else if tool_free_recovery {
                let salvaged = harness_state.take_recovery_rejected_synthesis();
                complete_turn_with_salvage(
                    working_history,
                    stage,
                    ".stop_after_directive",
                    Some(err),
                    salvaged,
                    plan_session,
                    plan_state,
                    event_context,
                    Some(&mut *harness_state),
                )
                .await
            } else {
                TurnLoopResult::Completed { plan_approved_execution_pending: false }
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
async fn check_recovery_cycle_cap(
    cycles: u8,
    working_history: &mut Vec<uni::Message>,
    stage: &str,
    err: &anyhow::Error,
    salvaged_text: Option<String>,
    mut plan_session: Option<&mut PlanningWorkflowSessionState>,
    plan_state: Option<&PlanningWorkflowState>,
    events: Option<PlanRecoveryEventContext<'_>>,
    harness_state: Option<&mut HarnessTurnState>,
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
        return Some(
            complete_turn_after_failed_tool_free_recovery_with_events(
                working_history,
                stage,
                Some(err),
                salvaged_text,
                plan_session,
                plan_state,
                events,
                harness_state,
            )
            .await,
        );
    }
    None
}

#[cfg(test)]
mod tests;
