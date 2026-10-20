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

#[path = "post_tool_recovery/failure.rs"]
mod failure;

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
        && !harness_state.tool_budget_exhausted_emitted
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
            "The provider rejected an unmatched tool result after one bounded history repair; the turn is paused for resume.",
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

/// Plan-mode recovery fallback. When the tool-free synthesis fails, the
/// "salvaged" text is usually just a rambling recovery monologue with tool-call
/// markup stripped out — not a plan. Injecting that as the plan the user sees is
/// worse than the structured plan-mode message. Keep salvage only when its
/// extracted artifact passes the same readiness gate used by approval.
fn plan_mode_recovery_fallback(
    salvaged_text: Option<String>,
    structured_message: &str,
    working_history: &[uni::Message],
) -> String {
    if let Some(text) = salvaged_text
        && let Some(clean) = ready_plan_text(&text)
    {
        // Return the validated extracted plan text (not the raw salvage) so
        // surrounding recovery prose or blank-line framing is never injected
        // as the user-visible plan. `ready_plan_text` already enforces the
        // same readiness gate used by approval.
        clean
    } else {
        build_recovery_fallback(working_history, structured_message)
    }
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
            "{lead_in}\n\nFiles already read this turn (their content is in the history above):\n{}",
            files_read.iter().map(|f| format!("  - {f}")).collect::<Vec<_>>().join("\n")
        )
    }
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
            "The provider rejected an unmatched tool result after one bounded history repair; the turn is blocked for resume.",
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
    if (harness_state.wall_clock_exhausted_emitted
        || harness_state.wall_clock_exhausted()
        || harness_state.tool_budget_exhausted_emitted)
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

    fn context_capacity_err() -> anyhow::Error {
        anyhow::Error::new(LLMError::InvalidRequest {
            message: "maximum context length is 114688 tokens".to_string(),
            metadata: None,
        })
    }

    #[test]
    fn follow_up_failure_tui_lines_obey_two_line_contract() {
        let status = follow_up_failure_status_line(" (transient; bounded retry scheduled)");
        assert!(!status.contains('\n'), "status stays one line: {status}");
        assert!(!status.contains("Follow-up error category"), "no category line: {status}");
        assert!(status.contains("model follow-up failed"));

        for action in [
            follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, false, false, false),
            follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, true, false, false),
            follow_up_failure_action_line(PostToolFailureRecovery::RetryToolFree, false, false, false),
            follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, true, true),
            follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, false, false),
        ] {
            assert!(!action.contains('\n'), "action stays one line: {action}");
            assert!(!action.contains("Follow-up error category"), "no category leak: {action}");
            assert!(!action.contains("Elapsed:"), "no forensics: {action}");
        }
    }

    #[test]
    fn follow_up_failure_action_line_matches_outcome() {
        assert!(
            follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, true, false, false)
                .contains("Context capacity")
        );
        assert!(
            follow_up_failure_action_line(PostToolFailureRecovery::RetryToolEnabled, false, false, false)
                .contains("tool-enabled recovery")
        );
        assert!(
            follow_up_failure_action_line(PostToolFailureRecovery::RetryToolFree, false, false, false)
                .contains("tool-free recovery")
        );
        assert!(
            follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, true, true)
                .contains("keep planning"),
            "planning stop names the next verb"
        );
        assert!(
            follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, false, true)
                .contains("narrower prompt"),
            "non-retryable stop names the retry tip"
        );
        assert!(
            follow_up_failure_action_line(PostToolFailureRecovery::StopAfterDirective, false, false, false)
                .contains("continue"),
            "misconfiguration stop offers continue"
        );
    }

    #[test]
    fn misconfiguration_network_error_is_not_transient_recovery() {
        let err = anyhow::Error::new(LLMError::Network {
            message: "network error: invalid endpoint in base_url".to_string(),
            metadata: None,
        });

        assert!(!is_transient_recovery_error(&err));
        assert!(is_transient_recovery_error(&transient_err()));
    }

    #[test]
    fn guided_misconfiguration_is_not_reclassified_as_transient() {
        let err = anyhow::anyhow!(
            "Network error: invalid endpoint in base_url. Check settings/config first (base_url in vtcode.toml): fix it. Correct the configuration before retrying."
        );

        assert!(misconfiguration_guidance(&err).is_none(), "existing guidance must not be rendered twice");
        assert!(!is_transient_recovery_error(&err));
    }

    // ---- Pure policy tests for `planning_finalize_notice` ----
    // All six (exhaustion × draft-ready) combinations are tested in isolation,
    // independent of the async harness. The critical invariant: a ready-draft
    // prompt promising "Review the plan below" must ONLY appear when
    // plan_ready == true. The no-draft variants must never promise a reviewable
    // draft (checkpoint turn_902).

    #[test]
    fn finalize_notice_budget_exhausted_with_draft_mentions_preserved_plan() {
        let notice = planning_finalize_notice(PlanningRecoveryOutcome::BudgetExhausted { plan_ready: true });
        assert!(notice.contains("session plan file"), "budget+draft must point to the persisted plan: {notice}");
        assert!(!notice.contains("Re-state"), "budget+draft must not ask to re-state: {notice}");
    }

    #[test]
    fn finalize_notice_budget_exhausted_no_draft_asks_to_restate() {
        let notice = planning_finalize_notice(PlanningRecoveryOutcome::BudgetExhausted { plan_ready: false });
        assert!(notice.contains("Re-state"), "budget+no-draft must ask to re-state: {notice}");
        assert!(!notice.contains("session plan file"), "budget+no-draft must not reference a plan file: {notice}");
        assert!(!notice.contains("Plan draft ready"), "must not promise a draft: {notice}");
    }

    #[test]
    fn finalize_notice_recovery_exhausted_with_draft_mentions_preserved_plan() {
        let notice = planning_finalize_notice(PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: true });
        assert!(notice.contains("session plan file"), "recovery+draft must point to the persisted plan: {notice}");
    }

    #[test]
    fn finalize_notice_recovery_exhausted_no_draft_asks_to_restate() {
        let notice = planning_finalize_notice(PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: false });
        assert!(notice.contains("Re-state"), "recovery+no-draft must ask to re-state: {notice}");
        assert!(!notice.contains("session plan file"), "recovery+no-draft must not reference a plan file: {notice}");
        assert!(!notice.contains("Plan draft ready"), "must not promise a draft: {notice}");
    }

    #[test]
    fn finalize_notice_interview_denied_with_draft_offers_review_prompt() {
        let notice = planning_finalize_notice(PlanningRecoveryOutcome::InterviewDenied { plan_ready: true });
        assert!(notice.contains("Plan draft ready"), "denied+draft must offer the review prompt: {notice}");
        assert!(notice.contains("Review the plan below"), "denied+draft must present the draft: {notice}");
    }

    #[test]
    fn finalize_notice_interview_denied_no_draft_never_promises_review() {
        let notice = planning_finalize_notice(PlanningRecoveryOutcome::InterviewDenied { plan_ready: false });
        assert!(!notice.contains("Plan draft ready"), "denied+no-draft must NOT promise a draft: {notice}");
        assert!(!notice.contains("Review the plan below"), "denied+no-draft must NOT ask to review: {notice}");
        assert!(
            notice.contains("did not produce an approval-ready plan"),
            "must explain no plan was produced: {notice}"
        );
    }

    #[test]
    fn finalize_notice_from_session_classifies_correctly() {
        let mut session = PlanningWorkflowSessionState::default();
        assert_eq!(
            PlanningRecoveryOutcome::from_session(&session, false),
            PlanningRecoveryOutcome::InterviewDenied { plan_ready: false }
        );
        session.mark_budget_exhausted();
        assert_eq!(
            PlanningRecoveryOutcome::from_session(&session, true),
            PlanningRecoveryOutcome::BudgetExhausted { plan_ready: true }
        );
        let mut session = PlanningWorkflowSessionState::default();
        session.mark_recovery_exhausted();
        assert_eq!(
            PlanningRecoveryOutcome::from_session(&session, false),
            PlanningRecoveryOutcome::RecoveryExhausted { plan_ready: false }
        );
    }

    // ---- Async integration tests ----

    #[tokio::test]
    async fn transient_post_tool_failure_selects_tool_enabled_retry() {
        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![uni::Message::tool_response(
            "call-1".to_string(),
            "{\"ok\":true}".to_string(),
        )];

        let recovery = maybe_recover_after_post_tool_llm_failure(
            &mut renderer,
            &mut working_history,
            &transient_err(),
            1,
            0,
            "test",
            true,
            true,
            false,
        )
        .expect("recovery policy should resolve");

        assert_eq!(recovery, PostToolFailureRecovery::RetryToolEnabled);
        assert!(
            working_history
                .iter()
                .any(|message| { message.content.as_text().contains("required write or verification tools") })
        );
    }

    #[tokio::test]
    async fn execution_error_in_plan_mode_selects_tool_free_plan_synthesis() {
        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![uni::Message::tool_response(
            "call-1".to_string(),
            "{\"ok\":true}".to_string(),
        )];
        let err = anyhow::anyhow!("simulated follow-up execution failure");
        assert_eq!(vtcode_commons::classify_anyhow_error(&err), ErrorCategory::ExecutionError);

        let recovery = maybe_recover_after_post_tool_llm_failure(
            &mut renderer,
            &mut working_history,
            &err,
            1,
            0,
            "test",
            true,
            false,
            true,
        )
        .expect("plan-mode execution failure should resolve");

        assert_eq!(recovery, PostToolFailureRecovery::RetryToolFree);
        assert!(
            working_history
                .iter()
                .any(|message| { message.content.as_text().contains("<proposed_plan>") })
        );
    }

    #[tokio::test]
    async fn out_of_band_copilot_progress_enables_post_tool_recovery_without_tool_history() {
        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![uni::Message::user("finish the plan after the interview".to_string())];

        let recovery = maybe_recover_after_post_tool_llm_failure_with_progress(PostToolLlmRecoveryInputs {
            renderer: &mut renderer,
            working_history: &mut working_history,
            err: &transient_err(),
            step_count: 2,
            turn_history_start_len: 0,
            failure_stage: "execute_llm_request",
            allow_tool_free_retry: true,
            allow_tool_enabled_retry: true,
            planning_active: true,
            out_of_band_tool_progress: true,
        })
        .expect("inline Copilot progress should activate bounded post-tool recovery");

        assert_eq!(recovery, PostToolFailureRecovery::RetryToolEnabled);
        assert!(working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE
        }));
    }

    #[tokio::test]
    async fn context_capacity_failure_selects_compacting_tool_enabled_retry() {
        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![uni::Message::tool_response(
            "call-1".to_string(),
            "{\"ok\":true}".to_string(),
        )];

        let recovery = maybe_recover_after_post_tool_llm_failure(
            &mut renderer,
            &mut working_history,
            &context_capacity_err(),
            1,
            0,
            "execute_llm_request",
            false,
            true,
            false,
        )
        .expect("context recovery policy should resolve");

        assert_eq!(recovery, PostToolFailureRecovery::RetryToolEnabled);
    }

    #[tokio::test]
    async fn context_capacity_marker_from_non_request_stage_does_not_enable_tool_retry() {
        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![uni::Message::tool_response(
            "call-1".to_string(),
            "{\"ok\":true}".to_string(),
        )];

        let recovery = maybe_recover_after_post_tool_llm_failure(
            &mut renderer,
            &mut working_history,
            &context_capacity_err(),
            1,
            0,
            "process_llm_response",
            false,
            true,
            false,
        )
        .expect("non-request recovery policy should resolve");

        assert_eq!(recovery, PostToolFailureRecovery::StopAfterDirective);
        assert!(
            !working_history
                .iter()
                .any(|message| { message.content.as_text().contains("required write or verification tools") })
        );
    }

    #[tokio::test]
    async fn repeated_tool_enabled_retry_failure_returns_blocked_handoff() {
        use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![
            uni::Message::user("apply the requested fix".to_string()),
            uni::Message::tool_response("call-1".to_string(), "{\"ok\":true}".to_string()),
        ];
        let mut harness_state =
            HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
        let err = transient_err();

        let first = dispatch_post_tool_failure(PostToolRecoveryContext {
            renderer: &mut renderer,
            working_history: &mut working_history,
            harness_state: &mut harness_state,
            harness_emitter: None,
            plan_session: None,
            plan_state: None,
            err: &err,
            step_count: 1,
            turn_history_start_len: 1,
            stage: "first",
            tool_free_recovery: false,
        })
        .await
        .expect("first recovery dispatch should succeed");
        assert!(matches!(first, PostToolFailureAction::Continue));
        assert!(harness_state.consume_recovery_pass());

        let second = dispatch_post_tool_failure(PostToolRecoveryContext {
            renderer: &mut renderer,
            working_history: &mut working_history,
            harness_state: &mut harness_state,
            harness_emitter: None,
            plan_session: None,
            plan_state: None,
            err: &err,
            step_count: 2,
            turn_history_start_len: 1,
            stage: "second",
            tool_free_recovery: false,
        })
        .await
        .expect("second recovery dispatch should produce a truthful handoff");

        assert!(matches!(second, PostToolFailureAction::Break(TurnLoopResult::Blocked { reason: Some(_) })));
    }

    #[tokio::test]
    async fn dispatch_recovers_plan_follow_up_after_inline_copilot_interview() {
        use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![uni::Message::user("plan the requested runtime fix".to_string())];
        let mut harness_state =
            HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
        harness_state.record_out_of_band_tool_progress();
        harness_state.switch_to_tool_free_recovery();
        assert!(harness_state.consume_recovery_pass());
        let mut plan_session = PlanningWorkflowSessionState::default();
        let err = transient_err();

        let action = dispatch_post_tool_failure(PostToolRecoveryContext {
            renderer: &mut renderer,
            working_history: &mut working_history,
            harness_state: &mut harness_state,
            harness_emitter: None,
            plan_session: Some(&mut plan_session),
            plan_state: None,
            err: &err,
            step_count: 2,
            turn_history_start_len: 0,
            stage: "execute_llm_request",
            tool_free_recovery: true,
        })
        .await
        .expect("inline interview progress should schedule bounded synthesis recovery");

        assert!(matches!(action, PostToolFailureAction::Continue));
        assert_eq!(harness_state.recovery_retry_count(), 1);
    }

    #[tokio::test]
    async fn planning_synthesis_transient_failure_gets_one_outer_retry() {
        use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

        let mut renderer = AnsiRenderer::stdout();
        let mut working_history = vec![
            uni::Message::user("plan the launch-time optimization".to_string()),
            uni::Message::tool_response("call-1".to_string(), "{\"evidence\":\"startup path\"}".to_string()),
        ];
        let mut harness_state =
            HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
        let mut plan_session = PlanningWorkflowSessionState::default();
        let err = transient_err();

        harness_state.switch_to_tool_free_recovery();
        assert!(harness_state.consume_recovery_pass());

        let first = dispatch_post_tool_failure(PostToolRecoveryContext {
            renderer: &mut renderer,
            working_history: &mut working_history,
            harness_state: &mut harness_state,
            harness_emitter: None,
            plan_session: Some(&mut plan_session),
            plan_state: None,
            err: &err,
            step_count: 1,
            turn_history_start_len: 0,
            stage: "execute_llm_request",
            tool_free_recovery: true,
        })
        .await
        .expect("the first synthesis failure should schedule a retry");

        assert!(matches!(first, PostToolFailureAction::Continue));
        assert_eq!(harness_state.recovery_retry_count(), 1);
        assert!(harness_state.consume_recovery_pass());

        let second = dispatch_post_tool_failure(PostToolRecoveryContext {
            renderer: &mut renderer,
            working_history: &mut working_history,
            harness_state: &mut harness_state,
            harness_emitter: None,
            plan_session: Some(&mut plan_session),
            plan_state: None,
            err: &err,
            step_count: 2,
            turn_history_start_len: 0,
            stage: "execute_llm_request",
            tool_free_recovery: true,
        })
        .await
        .expect("the bounded retry should finalize recovery");

        assert!(matches!(second, PostToolFailureAction::Break(TurnLoopResult::Completed { .. })));
        assert!(
            plan_session.interview_pending(),
            "after the bounded retry, planning must remain resumable instead of approving an incomplete plan"
        );
        assert_eq!(harness_state.recovery_retry_count(), 1);
    }

    #[tokio::test]
    async fn tool_free_recovery_keeps_planning_alive_on_transient_error() {
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            None,
            Some(&mut plan_session),
            None,
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(
            plan_session.interview_pending(),
            "transient error must keep planning alive by re-forcing the interview"
        );
    }

    #[tokio::test]
    async fn tool_free_recovery_keeps_planning_alive_on_non_transient_error() {
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        let err = anyhow::Error::new(LLMError::InvalidRequest { message: "bad request".to_string(), metadata: None });

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&err),
            None,
            Some(&mut plan_session),
            None,
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(
            plan_session.interview_pending(),
            "any tool-free recovery failure must keep planning alive (not dead-end)"
        );
    }

    #[tokio::test]
    async fn dispatch_marks_recovery_exhausted_when_wall_clock_exhausted_in_plan_mode() {
        use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
        use vtcode_core::utils::ansi::AnsiRenderer;

        let mut renderer = AnsiRenderer::stdout();
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut harness_state =
            HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
        harness_state.wall_clock_exhausted_emitted = true;
        let mut plan_session = PlanningWorkflowSessionState::default();
        let err = transient_err();

        let action = dispatch_post_tool_failure(PostToolRecoveryContext {
            renderer: &mut renderer,
            working_history: &mut working_history,
            harness_state: &mut harness_state,
            harness_emitter: None,
            plan_session: Some(&mut plan_session),
            plan_state: None,
            err: &err,
            step_count: 1,
            turn_history_start_len: 0,
            stage: "stage",
            tool_free_recovery: true,
        })
        .await
        .expect("dispatch must not error");

        assert!(
            plan_session.is_recovery_exhausted(),
            "wall-clock exhaustion during planning must mark the session \
             recovery-exhausted so the plan finalizes instead of looping"
        );
        assert!(!plan_session.interview_pending(), "must not re-force the interview after wall-clock exhaustion");
        assert!(matches!(action, PostToolFailureAction::Break(_)));
    }

    #[tokio::test]
    async fn tool_free_recovery_finalizes_when_budget_exhausted() {
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.mark_budget_exhausted();

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            None,
            Some(&mut plan_session),
            None,
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(
            !plan_session.interview_pending(),
            "budget-exhausted must not re-force the interview (would loop forever)"
        );
        assert!(
            working_history.iter().any(|m| m.role == uni::MessageRole::Assistant),
            "budget-exhausted must finalize the plan with a fallback answer"
        );
    }

    #[tokio::test]
    async fn tool_free_recovery_keeps_planning_when_interview_denied_without_valid_plan() {
        // When `request_user_input` is permanently denied (non-interactive
        // runtime), the recovery fallback must not advertise implementation
        // until a validated persisted artifact exists. It must keep the
        // planning session actionable without promising another interview.
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.mark_interview_denied();

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            None,
            Some(&mut plan_session),
            None,
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(
            !plan_session.interview_pending(),
            "interview-denied must not re-force the interview (would recur forever)"
        );
        let text = working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("a final answer must be pushed")
            .content
            .as_text();
        // No draft was persisted, so the ready-draft HITL prompt must NOT be
        // used: it promises "Plan draft ready / Review the plan below" and
        // offers `yes`/`implement`/`no`/`edit` choices that dead-end without a
        // persisted plan (checkpoint turn_902).
        assert!(
            !text.contains("Plan draft ready"),
            "must not promise a reviewable draft when none was persisted: {text}"
        );
        assert!(
            !text.contains("Review the plan below"),
            "must not tell the user to review a draft that does not exist: {text}"
        );
        assert!(
            !text.contains("Yes, clear context and implement"),
            "must not offer an implementation choice without a persisted plan: {text}"
        );
        assert!(
            !text.contains("interview will be presented"),
            "interview-denied fallback must NOT promise a future interview: {text}"
        );
        assert!(text.to_ascii_lowercase().contains("keep planning"), "fallback must keep planning active: {text}");
        assert!(
            text.contains("did not produce an approval-ready plan"),
            "the no-draft notice must explain that synthesis produced no plan: {text}"
        );
    }

    #[tokio::test]
    async fn plan_mode_recovery_rejects_garbled_tool_call_salvage() {
        // When the tool-free synthesis fails and the only "salvage" is a
        // rambling monologue with tool-call markup stripped out (no real
        // plan), plan mode must NOT inject that garbage as the proposed plan.
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        let garbled = "I have enough to plan. <invoke name=\"unified_search\"> \
            read more files</invoke> Here is my half-baked plan.";

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            None,
            Some(garbled.to_string()),
            Some(&mut plan_session),
            None,
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(plan_session.interview_pending(), "non-exhausted plan failure must re-force the interview");
        let text = working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("a final answer must be pushed")
            .content
            .as_text();
        assert!(
            text.contains("final synthesis failed"),
            "plan-mode fallback must be the structured message, not garbled salvage: {text}"
        );
        assert!(!text.contains("unified_search"), "garbled tool-call salvage must not leak into the plan: {text}");
    }

    #[tokio::test]
    async fn plan_mode_recovery_rejects_partial_proposed_plan_salvage() {
        // A partial proposed plan is not approval-ready and must not be
        // presented or persisted as if it were a completed artifact.
        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        let partial_plan =
            "<proposed_plan>\n- Action: add caching -> src/cache.rs\n  verify: cargo test\n</proposed_plan>";

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            None,
            Some(partial_plan.to_string()),
            Some(&mut plan_session),
            None,
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        let text = working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("a final answer must be pushed")
            .content
            .as_text();
        assert!(!text.contains("<proposed_plan"), "partial plans must not be presented: {text}");
        assert!(
            text.to_ascii_lowercase().contains("keep planning"),
            "partial plans must keep planning active: {text}"
        );
    }

    #[tokio::test]
    async fn plan_mode_recovery_persists_salvaged_proposed_plan_to_session_file() {
        // Regression: when the tool-free recovery pass finalizes the plan from
        // an inline `<proposed_plan>` (tools were disabled, so the model could
        // not write the file itself), the plan must be persisted to the
        // session plan file so the "preserved in the session plan file"
        // notice is truthful. Previously the plan lived only in chat history
        // and `.vtcode/plans/` stayed empty/template-only.
        use crate::agent::runloop::unified::planning_workflow::PlanningWorkflowState;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let plan_file = state.plans_dir().join("recovered-plan.md");
        state.set_plan_file(Some(plan_file.clone())).await;

        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.mark_budget_exhausted();
        let salvaged = r#"<proposed_plan>
# Launch-time recovery

## Summary
Persist a concrete recovery plan without implementing it.

## Implementation Steps
1. Add caching -> files: [src/cache.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run the targeted planning tests.

## Assumptions and Defaults
1. The existing cache policy remains unchanged.
</proposed_plan>"#;

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            Some(salvaged.to_string()),
            Some(&mut plan_session),
            Some(&state),
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        let content =
            std::fs::read_to_string(&plan_file).expect("salvaged plan must be persisted to the session plan file");
        assert!(
            content.contains("Add caching"),
            "salvaged plan must be written to the session plan file, got: {content}"
        );
    }

    #[tokio::test]
    async fn interview_denied_with_persisted_draft_offers_approval_prompt() {
        // Counterpart to `tool_free_recovery_keeps_planning_when_interview_denied_without_valid_plan`:
        // when the interview is denied AND a valid draft was already persisted
        // (e.g. from a prior turn) but THIS turn's recovery salvage produced no
        // ready plan, the ready-draft HITL prompt ("Plan draft ready ... Review
        // the plan below ... yes/implement/no/edit") must still be shown so the
        // user can approve the real on-disk draft. Gating the prompt on
        // `persisted_plan_ready` must not suppress it in the legitimate
        // with-draft case (turn_902).
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let plan_file = state.plans_dir().join("denied-interview-plan.md");
        state.set_plan_file(Some(plan_file.clone())).await;
        // Pre-persist a valid plan so `persisted_plan_is_ready` returns true,
        // simulating a draft carried over from a previous turn.
        let valid_plan = r#"<proposed_plan>
# Denied-interview approval

## Summary
Offer the approval prompt when a real draft was persisted despite the denial.

## Implementation Steps
1. Add caching -> files: [src/cache.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run the targeted planning tests.

## Assumptions and Defaults
1. The existing cache policy remains unchanged.
</proposed_plan>"#;
        persist_plan_draft(&state, valid_plan)
            .await
            .expect("pre-persisting a valid plan must succeed");
        assert!(persisted_plan_is_ready(&state).await, "test precondition: a valid plan must be persisted and ready");

        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.mark_interview_denied();
        // No salvage this turn — the on-disk draft is what the prompt offers.

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            None,
            Some(&mut plan_session),
            Some(&state),
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        let text = working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("a final answer must be pushed")
            .content
            .as_text();
        assert!(
            text.contains("Plan draft ready"),
            "a persisted draft must still get the approval HITL prompt: {text}"
        );
        assert!(
            text.contains("Review the plan below"),
            "the reviewable draft must be presented for approval: {text}"
        );
        assert!(
            !text.contains("did not produce an approval-ready plan"),
            "the no-draft notice must NOT appear when a draft exists: {text}"
        );
    }

    #[test]
    fn plan_mode_recovery_fallback_returns_extracted_plan_without_framing() {
        let valid_inner = "# Plan\n\n## Summary\nClean.\n\n## Implementation Steps\n1. Do it -> files: [src/lib.rs] -> verify: [cargo check]\n\n## Test Cases and Validation\n1. Run cargo check.\n\n## Assumptions and Defaults\n1. Keep existing behavior.\n";
        let salvaged = format!("Here is the plan:\n\n<proposed_plan>\n{valid_inner}\n</proposed_plan>\n\nLet me know.");
        let fallback = plan_mode_recovery_fallback(Some(salvaged), "structured", &[]);
        assert!(
            !fallback.contains("Here is the plan"),
            "framing prose must not leak into the user-visible plan: {fallback}"
        );
        assert!(fallback.contains("## Summary"), "extracted plan must be returned: {fallback}");
    }

    #[test]
    fn plan_mode_recovery_fallback_rejects_garbage_salvage() {
        let fallback = plan_mode_recovery_fallback(
            Some("rambling monologue with no plan block".to_string()),
            "structured message",
            &[],
        );
        assert_eq!(fallback, "structured message");
    }

    #[tokio::test]
    async fn recovery_exhausted_with_persisted_draft_offers_preserved_plan_notice() {
        // Coverage gap: `RecoveryExhausted { plan_ready: true }` was the only
        // (exhaustion × draft-ready) combination not exercised by an async
        // integration test. When recovery is exhausted AND a valid draft was
        // persisted, the notice must reference the session plan file (not ask
        // to re-state) so the user can approve the preserved draft.
        use crate::agent::runloop::unified::planning_workflow::PlanningWorkflowState;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let plan_file = state.plans_dir().join("recovery-exhausted-plan.md");
        state.set_plan_file(Some(plan_file.clone())).await;
        let valid_plan = r#"<proposed_plan>
# Recovery-exhausted approval

## Summary
Offer the preserved-plan notice when recovery is exhausted with a persisted draft.

## Implementation Steps
1. Add caching -> files: [src/cache.rs] -> verify: [cargo nextest run -p vtcode]

## Test Cases and Validation
1. Run the targeted planning tests.

## Assumptions and Defaults
1. The existing cache policy remains unchanged.
</proposed_plan>"#;
        persist_plan_draft(&state, valid_plan)
            .await
            .expect("pre-persisting a valid plan must succeed");
        assert!(persisted_plan_is_ready(&state).await, "test precondition: a valid plan must be persisted and ready");

        let mut working_history: Vec<uni::Message> = Vec::new();
        let mut plan_session = PlanningWorkflowSessionState::default();
        plan_session.mark_recovery_exhausted();

        let result = complete_turn_after_failed_tool_free_recovery(
            &mut working_history,
            "stage",
            Some(&transient_err()),
            None,
            Some(&mut plan_session),
            Some(&state),
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(!plan_session.interview_pending(), "recovery-exhausted must not re-force the interview");
        let text = working_history
            .iter()
            .rev()
            .find(|m| m.role == uni::MessageRole::Assistant)
            .expect("a final answer must be pushed")
            .content
            .as_text();
        assert!(
            text.contains("session plan file"),
            "recovery-exhausted+draft must reference the preserved plan file: {text}"
        );
        assert!(!text.contains("Re-state"), "recovery-exhausted+draft must not ask to re-state: {text}");
        assert!(
            !text.contains("did not produce an approval-ready plan"),
            "the no-draft notice must NOT appear when a draft exists: {text}"
        );
    }

    #[tokio::test]
    async fn recovery_fallback_marks_explicit_fallback_flag() {
        use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

        let mut working_history = vec![uni::Message::user("summarize".to_string())];
        let mut harness_state =
            HarnessTurnState::new(TurnRunId("test-run".to_string()), TurnId("test-turn".to_string()), 4, 600, 0);
        assert!(!harness_state.final_response_was_fallback());

        let result = complete_turn_after_failed_tool_free_recovery_with_events(
            &mut working_history,
            "test.stage",
            None,
            None,
            None,
            None,
            None,
            Some(&mut harness_state),
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(
            harness_state.final_response_was_fallback(),
            "recovery fallback must mark the explicit flag so ensure_completed does not rely on !rendered"
        );
        assert!(
            working_history.iter().any(|message| {
                message.role == uni::MessageRole::Assistant
                    && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                    && !message.content.as_text().trim().is_empty()
            }),
            "recovery fallback must push a final answer"
        );
    }

    #[tokio::test]
    async fn recovery_fallback_without_harness_state_still_pushes_final() {
        let mut working_history = vec![uni::Message::user("summarize".to_string())];

        let result = complete_turn_after_failed_tool_free_recovery_with_events(
            &mut working_history,
            "test.stage",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

        assert!(matches!(result, TurnLoopResult::Completed { .. }));
        assert!(
            working_history.iter().any(|message| {
                message.role == uni::MessageRole::Assistant && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            }),
            "None harness_state must remain test-compatible and still push a final"
        );
    }
}
