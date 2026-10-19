//! Turn completion and recovery response texts.
//!
//! Pure builders for blocked/completed/fallback final responses plus the shared
//! reason and directive constants. Extracted from the `turn_loop.rs` root; main
//! loop policy stays there. Covered by `turn_loop/tests/`.

use super::TurnLoopContext;
use super::post_tool_recovery::POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON;
use crate::agent::runloop::unified::run_loop_context::HarnessTurnState;
use crate::agent::runloop::unified::run_loop_context::budget_exhaustion_cause;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use anyhow::Result;
use vtcode_core::core::agent::refusal;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

/// defined in crates/codegen/vtcode-core/src/tools/registry?" task produced 4 identical
/// 6500-token outline responses and burned ~90 seconds.  Capping at 2
/// consecutive responses terminate the runaway loop while still allowing one
/// retry for genuine recovery scenarios.
pub(crate) const MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN: u32 = 2;
/// Closure text for streamed tool-call items whose calls never reached the
/// pipeline (rejected pre-flight, dropped from a batch, or interrupted when
/// the turn ended). Teardown emits it so the session log carries a terminal
/// `tool_output` instead of a dangling `item.started`.
pub(crate) const UNDISPATCHED_TOOL_CALL_CLOSURE_TEXT: &str =
    "Tool call ended when the turn finished before it could execute.";
pub(crate) const ASSISTANT_TEXT_RESPONSE_CAP_REASON: &str =
    "Turn blocked after repeated assistant responses reached the safety cap; the latest response was preserved.";
pub(crate) const PENDING_VERIFICATION_BLOCK_REASON: &str =
    "Turn blocked after repeated unverified assistant responses; verification is still pending.";
/// The durable reason names the missing checks and their observed evidence.
fn pending_verification_final_response() -> String {
    "Completion remains unverified. Preserve the changes and passing evidence; resolve the missing checks before claiming success. Type `continue` to resume.".to_string()
}
const CONTEXT_CAPACITY_FINAL_RESPONSE: &str = "The turn is blocked because context capacity or compaction failed. \
    The retained tool outputs and progress are preserved; resume the request or switch \
    models and try again.";
pub(super) const GENERIC_BLOCKED_FINAL_RESPONSE: &str =
    "The turn stopped before the request was completed. Resume the request or give updated instructions to continue.";
/// Maximum number of times the post-tool follow-up failure path may schedule
/// a tool-free recovery pass within a single turn. This is a defense-in-depth
/// backstop: the recovery pass itself is terminal (a text response ends the
/// turn), so under normal operation this cap never trips. It only fires if a
/// future regression re-enables tools after recovery or otherwise re-triggers
/// the post-tool failure path cyclically.
pub(super) const MAX_POST_TOOL_RECOVERY_CYCLES: u8 = 2;
/// Bound retries for stale planning-recovery status text after an approved
/// plan handoff. The response is discarded and the model is reminded that the
/// new build turn has tools; after two attempts normal failure handling wins.
pub(super) const MAX_APPROVED_PLAN_STALE_PAUSE_RETRIES: u8 = 2;
pub(crate) const POST_TOOL_RECOVERY_REASON: &str =
    "Tool follow-up failed. Tools disabled; respond with text using context and recent tool outputs.";
pub(super) const RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER: &str = "Recovery synthesis failed; no tool call applied. The tool outputs gathered above contain the information needed. Re-state your request and the next turn will reuse the gathered context from this conversation history.";
/// Plan-mode variant of [`RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER`]. In plan
/// mode the turn must not dead-end: planning research is preserved and the
/// interview is re-forced on the next turn (the planning session state is left
/// `interview_pending`), so this message tells the user to continue planning
/// rather than re-state a generic request.
pub(super) const PLANNING_RECOVERY_SYNTHESIS_FALLBACK: &str = "Planning research completed, but final synthesis failed after one bounded automatic retry. The gathered context is preserved, no implementation has been approved, and the next planning turn will reuse this research.";
pub(super) const PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_RETRY: &str = "Planning research completed, but final synthesis failed to produce an approval-ready plan. The gathered context is preserved, no implementation has been approved, and the next planning turn will reuse this research.";
/// Variant of [`PLANNING_RECOVERY_SYNTHESIS_FALLBACK`] used once
/// `request_user_input` has been permanently denied by policy this session
/// (`PlanningWorkflowSessionState::is_interview_denied`). The transient-error
/// variant above assumes the interview can be re-forced on the next turn — a
/// denial recurs on every attempt, so no interview will ever be shown again.
/// This presents a clean yes/no/edit HITL prompt to the
/// user instead of a confusing "synthesis failed" message (checkpoint
/// turn_655/turn_660/turn_725). The plan draft from gathered research is
/// preserved in the session plan file; the user picks one of three paths:
///   - `yes`/`implement` → exit plan mode and start implementation
///   - `no` → abandon the plan
///   - `edit`/`keep planning` → refine the plan (user re-states what to revise)
///
/// Use this ONLY when an approval-ready draft was actually persisted
/// (`persisted_plan_ready`). The message promises "Review the plan below" and
/// offers `yes`/`implement`/`no`/`edit` choices that dead-end without a
/// persisted plan. When synthesis produced no draft, use
/// [`PLANNING_INTERVIEW_DENIED_NO_DRAFT_NOTICE`] instead (checkpoint turn_902).
pub(super) const PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_INTERVIEW: &str = "Plan draft ready (interactive questions are unavailable in this runtime, so the plan was finalized from the research already gathered). Review the plan below. For long research sessions, choose `Yes, clear context and implement` to preserve the plan while starting execution with a fresh context and tool budget. Choose `Yes, implement this plan` when the recent planning details are still useful. Type `no` to abandon or `edit` to revise.";
/// Variant of [`PLANNING_RECOVERY_SYNTHESIS_FALLBACK_NO_INTERVIEW`] for the
/// case where the bounded plan synthesis did NOT produce an approval-ready
/// draft (the model emitted research prose or pseudo-tool-call markup instead
/// of a `<proposed_plan>`). The ready-draft variant above promises "Review the
/// plan below" and offers `yes`/`implement`/`no`/`edit` — choices that
/// dead-end without a persisted plan, and which the appended
/// `PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT` immediately contradicts with
/// "no approval-ready plan was produced". This variant makes the primary
/// message consistent with that hint: it tells the user no draft was produced
/// and asks them to re-state so the next turn can synthesize from the research
/// already gathered (checkpoint turn_902).
pub(super) const PLANNING_INTERVIEW_DENIED_NO_DRAFT_NOTICE: &str = "Planning synthesis did not produce an approval-ready plan, and interactive questions are unavailable in this runtime. The research gathered this turn is preserved in this conversation. Re-state what you'd like to plan; the next turn will reuse the evidence already gathered instead of re-exploring.";
/// User-facing final answer for the budget-exhausted plan-mode dead end. The
/// the planning session stays alive so `implement` / `keep planning` still
/// work. Use this variant when an approval-ready draft was persisted.
pub(super) const PLANNING_BUDGET_EXHAUSTED_USER_NOTICE: &str = "Planning research reached its safe budget before synthesis completed. The evidence and current plan draft are preserved in the session plan file (.vtcode/plans/); retry approval or revision from the preserved plan.";
/// No-draft variant of [`PLANNING_BUDGET_EXHAUSTED_USER_NOTICE`]. Used when
/// budget was exhausted but no approval-ready plan was produced. Does NOT
/// mention a plan draft or plan file, consistent with the appended
/// `PLANNING_WORKFLOW_NO_APPROVAL_READY_PLAN_HINT`.
pub(super) const PLANNING_BUDGET_EXHAUSTED_NO_DRAFT_NOTICE: &str = "Planning research reached its safe budget before synthesis completed. The evidence gathered this turn is preserved in this conversation. Re-state what you'd like to plan; the next turn will reuse the evidence already gathered instead of re-exploring.";
/// User-facing final answer for the recovery-exhausted plan-mode dead end.
/// See [`PLANNING_BUDGET_EXHAUSTED_USER_NOTICE`]. Use this variant when an
/// approval-ready draft was persisted.
pub(super) const PLANNING_RECOVERY_EXHAUSTED_USER_NOTICE: &str = "Plan synthesis failed after repeated recovery attempts (provider errors or saturated context). The research gathered and the current plan draft are preserved in the session plan file (.vtcode/plans/).";
/// No-draft variant of [`PLANNING_RECOVERY_EXHAUSTED_USER_NOTICE`]. Used when
/// recovery is exhausted but no approval-ready plan was produced.
pub(super) const PLANNING_RECOVERY_EXHAUSTED_NO_DRAFT_NOTICE: &str = "Plan synthesis failed after repeated recovery attempts (provider errors or saturated context). The research gathered this turn is preserved in this conversation. Re-state what you'd like to plan; the next turn will reuse the evidence already gathered instead of re-exploring.";
/// Reason set on `TurnLoopResult::Blocked` when an approved-plan execution
/// turn keeps echoing the stale planning-turn pause after the bounded
/// clearing retries are exhausted. The stale text is discarded, so the
/// generic text-response budget never sees it.
pub(super) const STALE_APPROVED_PLAN_PAUSE_BLOCK_REASON: &str = "Approved-plan execution kept returning stale recovery state after bounded retries; the stale pause response was discarded.";
pub(crate) const COMPLETED_TURN_FALLBACK_RESPONSE: &str = "The turn stopped before a final assistant response was produced. No final outcome was confirmed; please retry the request.";
/// Planning-specific variant of [`COMPLETED_TURN_FALLBACK_RESPONSE`]. A
/// plan-mode turn that ends without any final text must stay resumable: the
/// research is preserved and the next `keep planning` turn reuses it instead
/// of re-exploring. The generic fallback ("please retry the request") hides
/// that contract, so planning publishes this response (paired with
/// [`completed_fallback_reason`] planning reason) instead. Mirrors the
/// `<proposed_plan>` contract in `break_planning_recovery_with_handoff`
/// without duplicating its detail (that path knows the synthesis failed;
/// this path only knows no final was produced).
pub(crate) const PLANNING_COMPLETED_FALLBACK_RESPONSE: &str = "Planning remains active, but this turn ended without a final plan synthesis. The research gathered above is preserved, so the next turn can reuse it without re-reading files. Type `keep planning` (or re-state the request) to continue; the next turn should emit one complete `<proposed_plan>` with `Action -> files: [path] -> verify: [command]` steps. Each `verify:` must be a concrete command or observable check; valid examples are `verify: [cargo nextest run -p vtcode]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, and `verify: [grep -n 'symbol' src/file.rs]`, while `verify: [run checks]` and `verify: [git diff --check]` are invalid. No changes were applied.";
const COMPLETED_TURN_FALLBACK_REASON: &str = "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained.";
/// Planning-specific variant of [`COMPLETED_TURN_FALLBACK_REASON`]. When a
/// plan-mode turn ends via the generic fallback path, the generic reason hides
/// that planning remains active. Preserve the planning reason code (not full
/// fallback prose) so `Blocked` telemetry and resume handoffs stay actionable.
/// Worded as "without confirming" rather than "did not produce" because this
/// call site only knows planning was active, not whether a ready draft was
/// already persisted.
const PLANNING_COMPLETED_TURN_FALLBACK_REASON: &str = "Planning turn ended via recovery fallback without confirming an approval-ready plan; planning remains active. The current plan and task state were retained.";
/// Pure selection of the Completed-to-Blocked fallback reason so the planning
/// specificity is unit-testable without harness state. Returns the planning
/// reason code (not full fallback prose) when planning is active.
pub(crate) fn completed_fallback_reason(planning_active: bool) -> &'static str {
    if planning_active {
        PLANNING_COMPLETED_TURN_FALLBACK_REASON
    } else {
        COMPLETED_TURN_FALLBACK_REASON
    }
}

pub(super) fn completed_recovery_fallback_reason(planning_active: bool, state: &HarnessTurnState) -> String {
    match state.budget_recovery_reason() {
        Some(cause) => format!("{cause}. {}", completed_fallback_reason(planning_active)),
        None => completed_fallback_reason(planning_active).to_string(),
    }
}

pub(crate) fn budget_recovery_final_response(planning_active: bool, state: &HarnessTurnState) -> Option<String> {
    state
        .budget_recovery_reason()
        .map(|_| format_blocked_turn_final_response(&completed_recovery_fallback_reason(planning_active, state)))
}
pub(super) const COMPLETED_TURN_NO_RESPONSE_REASON: &str =
    "Turn ended without a harness-visible final assistant response, so successful completion could not be confirmed.";
pub(super) const PLAN_RECOVERY_EXHAUSTED_REASON: &str = "Approved-plan execution stopped after recovery was exhausted. The approved plan and task checklist were retained; retry from the pending step.";
/// System message injected before retrying a tool-free recovery pass when the model
/// produced tool calls or textual tool-call markup (which are discarded) instead of
/// plain text. It names the failure mode explicitly so the model can self-correct on
/// the retry rather than repeating the same violation (observed in checkpoint turn_621,
/// where a single `<tool_call>` block terminated the turn instead of being retried).
pub(super) const RECOVERY_TOOL_CALL_RETRY_DIRECTIVE: &str = "Recovery: tools are disabled, so respond with plain text only. Your previous \
     attempt included tool-call or function-call markup; summarize the findings from \
     the tool outputs already in history as a final answer, without any <tool_call>, \
     <function=...>, or other tool-call syntax.";
/// Plan-mode variant of [`POST_TOOL_RECOVERY_REASON`]. In plan mode the agent
/// must emit the `<proposed_plan>` from the research already gathered, not start
/// more research — so the recovery reason names the plan format explicitly.
/// Without this, the model treats the tool-free recovery pass as another
/// research step and emits `<invoke>`/`<tool_call>` markup instead of a plan
/// (observed in checkpoints turn_648 and turn_650).
pub(crate) const POST_TOOL_RECOVERY_REASON_PLAN_MODE: &str = "Planning research completed, but final plan synthesis needs recovery. Tools are disabled for this pass. Produce the `<proposed_plan>` from the context and tool outputs already in this conversation: include Summary, numbered Implementation Steps (one line each: `Action -> files: [src/parser.rs] -> verify: [cargo check --locked]`), Test Cases and Validation, and Assumptions and Defaults. Every step must name a concrete file, symbol, or behavior target and one concrete `verify:` command or observable check. Valid examples: `verify: [cargo nextest run -p vtcode]`, `verify: [cargo check --locked]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, `verify: [grep -n 'symbol' src/file.rs]`, or `verify: [after launch confirm startup timing is reported]`. Invalid examples: `verify: [run checks]`, `verify: [check later]`, and `verify: [git diff --check]`; vague prose and generic VCS-only checks are rejected. Each comma-separated verify item must independently be concrete. Prefer file:symbol references. Tool calls and tool-call markup are discarded in this pass, so the response should contain none. Keep any text outside the block to one line or omit it; it is ignored.";
/// Plan-mode variant of [`RECOVERY_TOOL_CALL_RETRY_DIRECTIVE`]. The generic
/// directive only says \"respond with plain text\"; in plan mode the agent must
/// instead finalize the `<proposed_plan>` from gathered research, otherwise it
/// loops emitting `<invoke>` research calls during the tool-free recovery pass.
pub(crate) const RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE: &str = "Recovery: in plan mode, tools are disabled for this pass, so the next step is to finalize the plan. Emit the `<proposed_plan>` from the research already gathered in this conversation — Summary, numbered steps on single lines (`Action -> files: [src/parser.rs] -> verify: [cargo check --locked]`), Validation, Assumptions — every step with a concrete file, symbol, or behavior target and one concrete `verify:` command or observable check. Valid examples are `verify: [cargo nextest run -p vtcode]`, `verify: [rg -n 'symbol' src/file.rs]`, `verify: [sed -n '1,40p' docs/file.md]`, and `verify: [grep -n 'symbol' src/file.rs]`; invalid examples are `verify: [run checks]`, `verify: [check later]`, and `verify: [git diff --check]`. Each comma-separated verify item must independently be concrete. At most one intro line may appear outside the block; tool calls and `<tool_call>`/`<invoke>`/`<function=...>` markup are discarded, so the response should contain none.";
pub(super) const APPROVED_PLAN_STALE_PAUSE_RECOVERY_DIRECTIVE: &str = "Approved-plan execution recovery: the previous response stated that tools were disabled or implementation was paused, but the planning approval is complete and the write-capable build agent is active. The next step is the next concrete implementation action: use task_tracker and run an edit or verification command. No further confirmation is needed, and a pause or status-only message does not advance the approved plan.";

fn latest_final_assistant_response(history: &[uni::Message], turn_history_start_len: usize) -> Option<String> {
    history
        .get(turn_history_start_len..)
        .unwrap_or_default()
        .iter()
        .rev()
        .find(|message| {
            message.role == uni::MessageRole::Assistant
                && message.tool_calls.is_none()
                && message.phase != Some(uni::AssistantPhase::Commentary)
                && !message.content.as_text().trim().is_empty()
        })
        .map(|message| message.content.as_text().trim().to_string())
}

/// Promote the latest substantive commentary response when the anti-runaway
/// cap ends a turn without a final answer. The response was already rendered
/// as commentary; changing its phase preserves that content for history and
/// lets the normal blocked-turn publisher emit the canonical final item.
pub(super) fn promote_latest_commentary_to_final(history: &mut [uni::Message], turn_history_start_len: usize) -> bool {
    if latest_final_assistant_response(history, turn_history_start_len).is_some() {
        return false;
    }

    let Some(turn_history) = history.get_mut(turn_history_start_len..) else {
        return false;
    };
    let Some(message) = turn_history.iter_mut().rev().find(|message| {
        message.role == uni::MessageRole::Assistant
            && message.tool_calls.is_none()
            && message.phase == Some(uni::AssistantPhase::Commentary)
            && !message.content.as_text().trim().is_empty()
    }) else {
        return false;
    };

    message.phase = Some(uni::AssistantPhase::FinalAnswer);
    true
}

fn publish_final_assistant_response(ctx: &mut TurnLoopContext<'_>, text: &str) -> Result<bool> {
    if text.trim().is_empty() {
        return Ok(false);
    }

    if !ctx.harness_state.final_response_rendered() {
        ctx.renderer.line(MessageStyle::Response, text)?;
        ctx.harness_state.mark_final_response_rendered();
    }

    if ctx.harness_emitter.is_none() {
        ctx.harness_state.mark_final_response_event_emitted();
        return Ok(true);
    }

    if !ctx.harness_state.final_response_event_emitted()
        && let Some(emitter) = ctx.harness_emitter
    {
        match emitter.emit_assistant_message(&ctx.harness_state.turn_id.0, text) {
            Ok(()) => ctx.harness_state.mark_final_response_event_emitted(),
            Err(err) => tracing::warn!(error = %err, "final assistant message harness emission failed"),
        }
    }

    Ok(ctx.harness_state.final_response_event_emitted())
}

pub(crate) fn format_blocked_turn_final_response(reason: &str) -> String {
    if refusal::is_refusal_notice(reason) {
        // The refusal notice is already a complete, user-facing explanation
        // with its own next step; wrapping it would repeat that guidance.
        return reason.trim().to_string();
    }
    if budget_exhaustion_cause(reason).is_some() {
        format!(
            "The turn stopped: {}. Continue from the retained conversation and pending task state; reuse gathered evidence and report remaining work and verification explicitly.",
            reason_clause(reason)
        )
    } else if super::super::tool_outcomes::helpers::is_pending_verification_block(reason) {
        let evidence = vtcode_commons::formatting::truncate_byte_budget(reason, 2000, " [truncated]");
        format!("{}\n\n{evidence}", pending_verification_final_response())
    } else if reason.contains(POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON) {
        CONTEXT_CAPACITY_FINAL_RESPONSE.to_string()
    } else if reason.contains("tool-call limit")
        || reason.contains("Recovery tool-call limit")
        || reason.contains("Blocked tool-call limit")
    {
        format!(
            "The turn stopped because repeated tool calls were rejected: {}. Resume the request with specific guidance, or adjust permissions or tools to continue.",
            reason_clause(reason)
        )
    } else if reason.contains("Repeated shell command") {
        "The turn stopped because repeated identical shell commands were detected. Give alternative instructions or adjust the command to continue.".to_string()
    } else if !reason.trim().is_empty() && reason.trim() != "blocked" {
        format!(
            "The turn stopped: {}. Resume the request or give updated instructions to continue.",
            reason_clause(reason)
        )
    } else {
        GENERIC_BLOCKED_FINAL_RESPONSE.to_string()
    }
}

/// Render a block reason as a clause that continues a sentence after a colon:
/// trailing sentence punctuation is dropped (the caller supplies the period)
/// and a leading capitalized plain word is lowercased. Acronyms and
/// identifiers keep their case.
fn reason_clause(reason: &str) -> String {
    vtcode_commons::formatting::lowercase_leading_word(reason.trim().trim_end_matches(['.', '!', '?']).trim_end())
}

#[cfg(test)]
pub(crate) fn blocked_turn_final_response(reason: &str) -> String {
    format_blocked_turn_final_response(reason)
}

pub(super) fn ensure_blocked_turn_response(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    turn_history_start_len: usize,
    reason: &str,
) -> Result<()> {
    if ctx.harness_state.turn_refused() {
        return publish_refusal_notice(ctx, reason);
    }
    let existing_final = latest_final_assistant_response(working_history, turn_history_start_len);
    let generated_fallback = existing_final.is_none();
    let final_text = existing_final.unwrap_or_else(|| format_blocked_turn_final_response(reason));
    if generated_fallback {
        ctx.harness_state.mark_final_response_fallback();
        working_history
            .push(uni::Message::assistant(final_text.clone()).with_phase(Some(uni::AssistantPhase::FinalAnswer)));
    }
    if generated_fallback && ctx.harness_state.final_response_event_emitted() {
        // The harness already has the one allowed final assistant item. The
        // retained history may have been compacted after that emission, so
        // restore the deterministic handoff locally without duplicating the
        // canonical AgentMessage event.
        if !ctx.harness_state.final_response_rendered() {
            ctx.renderer.line(MessageStyle::Response, &final_text)?;
            ctx.harness_state.mark_final_response_rendered();
        }
    } else {
        let _ = publish_final_assistant_response(ctx, &final_text)?;
    }
    Ok(())
}

/// Publish the refusal notice for a refused turn.
///
/// The notice is always shown, even when an earlier final answer exists in
/// this turn: that answer predates the refused request and would misreport
/// the outcome. The notice goes to the transcript and the harness event
/// stream only. It is not appended to `working_history`, because the session
/// loop rolls the refused turn out of model-visible history.
fn publish_refusal_notice(ctx: &mut TurnLoopContext<'_>, reason: &str) -> Result<()> {
    let notice = format_blocked_turn_final_response(reason);
    ctx.harness_state.mark_final_response_fallback();
    ctx.renderer.line(MessageStyle::Response, &notice)?;
    ctx.harness_state.mark_final_response_rendered();
    if ctx.harness_emitter.is_none() {
        ctx.harness_state.mark_final_response_event_emitted();
    } else if !ctx.harness_state.final_response_event_emitted()
        && let Some(emitter) = ctx.harness_emitter
    {
        // When a stale final was already emitted, the canonical final item is
        // taken; the `turn.blocked` event still carries the refusal reason.
        match emitter.emit_assistant_message(&ctx.harness_state.turn_id.0, &notice) {
            Ok(()) => ctx.harness_state.mark_final_response_event_emitted(),
            Err(err) => tracing::warn!(error = %err, "refusal notice harness emission failed"),
        }
    }
    Ok(())
}

/// Ensure a completed turn has crossed both user-visible response surfaces.
/// Recovery helpers may already have appended a fallback to history; this
/// publishes that existing text instead of appending a second answer.
///
/// A found final answer is treated as a genuine model synthesis, even when the
/// render flag was missed (e.g. streamed output). Recovery fallbacks mark
/// `final_response_was_fallback` explicitly at creation, so this helper must
/// not infer fallback from `!rendered` — that heuristic converts valid
/// single-step text turns into generic `Blocked` (observed as simple requests
/// ending with `COMPLETED_TURN_FALLBACK_REASON`). Unpublished valid answers
/// are still caught by the outer `!rendered || !event_emitted` guard with the
/// distinct `NO_RESPONSE` reason.
pub(super) fn ensure_completed_turn_response(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    turn_history_start_len: usize,
) -> Result<bool> {
    let mut response_was_fallback = ctx.harness_state.final_response_was_fallback();
    let final_text = latest_final_assistant_response(working_history, turn_history_start_len);

    let mut final_text = if let Some(final_text) = final_text {
        final_text
    } else {
        response_was_fallback = true;
        let fallback = if ctx.is_planning_active() {
            PLANNING_COMPLETED_FALLBACK_RESPONSE.to_string()
        } else if ctx.harness_state.budget_recovery_reason().is_some() {
            format_blocked_turn_final_response(&completed_recovery_fallback_reason(false, ctx.harness_state))
        } else {
            COMPLETED_TURN_FALLBACK_RESPONSE.to_string()
        };
        working_history
            .push(uni::Message::assistant(fallback.clone()).with_phase(Some(uni::AssistantPhase::FinalAnswer)));
        fallback
    };

    // Preserve best-effort prose and planning drafts, but publish the runtime
    // cause with their fallback. Genuine model synthesis remains untouched.
    if response_was_fallback
        && let Some(cause) = ctx.harness_state.budget_recovery_reason()
        && !final_text.contains(cause)
        && !final_text.contains(&reason_clause(cause))
    {
        let notice = format_blocked_turn_final_response(&completed_recovery_fallback_reason(
            ctx.is_planning_active(),
            ctx.harness_state,
        ));
        let previous_text = final_text;
        let retained_text = previous_text
            .strip_prefix(RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER)
            .or_else(|| previous_text.strip_prefix(COMPLETED_TURN_FALLBACK_RESPONSE))
            .unwrap_or(&previous_text)
            .trim();
        final_text = if retained_text.is_empty() {
            notice
        } else {
            format!("{notice}\n\n{retained_text}")
        };
        if let Some(message) = working_history
            .get_mut(turn_history_start_len..)
            .unwrap_or_default()
            .iter_mut()
            .rev()
            .find(|message| {
                message.role == uni::MessageRole::Assistant
                    && message.tool_calls.is_none()
                    && message.phase != Some(uni::AssistantPhase::Commentary)
                    && message.content.as_text().trim() == previous_text
            })
        {
            message.content = uni::MessageContent::Text(final_text.clone());
        }
    }

    let _ = publish_final_assistant_response(ctx, &final_text)?;

    if response_was_fallback {
        ctx.harness_state.mark_final_response_fallback();
    }
    Ok(response_was_fallback)
}

pub(super) fn provider_stop_outcome(
    state: &crate::agent::runloop::unified::state::CtrlCState,
    error: &anyhow::Error,
) -> Option<TurnLoopResult> {
    let outcome = crate::agent::runloop::unified::stop_requests::stop_outcome(state)
        .or_else(|| state.is_cancel_handled().then_some(TurnLoopResult::Cancelled));
    if outcome.is_none()
        && matches!(vtcode_commons::classify_anyhow_error(error), vtcode_commons::ErrorCategory::Cancelled)
    {
        // Provider text is failure evidence, not authority to cancel a local turn.
        tracing::warn!(error = %error, "provider reported cancellation without a local stop request");
    }
    outcome
}

/// True when a `Completed` turn must publish a final assistant response.
///
/// Primary-agent handoffs (plan entry via `SwitchPrimaryAgent`, deferred
/// mid-turn `start_planning`, approved-plan execution via the policy target)
/// are control-flow turns: the session loop constructs the follow-up request,
/// and the handoff turn legitimately ends on a tool result with no assistant
/// final. Requiring one would append a misleading recovery fallback and
/// convert the handoff into `Blocked`.
pub(super) fn completed_turn_requires_final_response(result: &TurnLoopResult, primary_agent_handoff: bool) -> bool {
    !primary_agent_handoff && matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: false })
}

/// Whether the turn ends on a primary-agent control-flow handoff that the
/// outer session loop will follow. Includes the deferred plan-entry switch
/// so mid-turn `start_planning` is exempt from the final-response guard
/// before that switch is applied to `pending_primary_agent`.
pub(super) fn is_primary_agent_handoff(
    pending_primary_agent: &Option<String>,
    pending_plan_execution_target: &Option<crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget>,
    deferred_plan_entry_switch: bool,
) -> bool {
    pending_primary_agent.is_some() || pending_plan_execution_target.is_some() || deferred_plan_entry_switch
}

/// True when a `SwitchPrimaryAgent` handoff is *entering* plan mode rather than
/// leaving it for an approved-plan execution agent. Plan entry must not claim
/// `plan_approved_execution_pending` (that flag starts implementation).
pub(crate) fn is_plan_entry_handoff(agent: &str) -> bool {
    agent.eq_ignore_ascii_case("plan")
}

pub(crate) const POST_TOOL_RESUME_DIRECTIVE: &str = "Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration. If those tool outputs include `critical_note`, `hint`, `next_action`, `fallback_tool`, `fallback_tool_args`, or `rerun_hint`, follow that guidance first. Files read in the previous turn are already in the conversation history above, so re-reading them adds no information. Synthesize a plan or answer from what is already gathered.";
pub(crate) const POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE: &str = "The previous model follow-up failed after tool execution. The older context will be compacted before this retry. Reuse the completed tool outputs above, do not repeat read-only exploration, and continue the user's request with any required write or verification tools. Only finish after the requested work is confirmed; do not claim success from an unverified plan.";
