//! Auto-continue directives: tracker, recoverable-block, and plan-mode follow-ups.

use super::*;

/// Build the model-facing auto-continue follow-up for incomplete tracker work.
pub(crate) const TRACKER_CONTINUE_FOLLOW_UP_PREFIX: &str = "The task tracker still has incomplete steps:";

pub(crate) fn tracker_continue_follow_up(incomplete: &[String]) -> String {
    let joined = incomplete.join(", ");
    format!(
        "{TRACKER_CONTINUE_FOLLOW_UP_PREFIX} {joined}. This follow-up is the harness resuming \
         the work, so no user reply is needed. The next step is to continue with the next incomplete step \
         using tools and update task_tracker as steps complete. A status-only recap does not advance the \
         tracker. The turn can end when the tracker is complete, or when a user decision or a \
         permission/policy block stops progress."
    )
}

/// Stable opening for recoverable blocked-end auto-continue when no tracker
/// items remain (or none were created). Session evidence: build/auto turns
/// ending with recovery fallback / blocked-tool fuse parked at `Continue…`.
pub(crate) const RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX: &str =
    "The previous turn ended on a recoverable block:";

pub(crate) fn recoverable_blocked_continue_follow_up(reason: &str) -> String {
    format!(
        "{RECOVERABLE_BLOCKED_CONTINUE_FOLLOW_UP_PREFIX} {reason} This follow-up is the harness resuming \
         the work, so no user reply is needed. Retry the requested work with tools; if a policy block \
         repeated, switch to a read-only approach or ask the user. A status-only recap does not advance \
         the work. The turn can end when the request is complete, or when a user decision or a \
         permission/policy block stops progress."
    )
}

pub(crate) fn recoverable_blocked_auto_continue_directive(reason: &str) -> String {
    format!(
        "Blocked-end auto-continue: the harness queued this turn after a recoverable block ({reason}). \
         No user reply is needed. Resume the original request; do not restate a status recap as the answer."
    )
}

/// Label of the system directive paired with a session-resume tracker continuation.
pub(crate) const TRACKER_RESUME_DIRECTIVE_LABEL: &str = "Resume continuation";
/// Label of the system directive paired with an in-session tracker auto-continue.
pub(crate) const TRACKER_AUTO_CONTINUE_DIRECTIVE_LABEL: &str = "Tracker auto-continue";

/// System directive paired with a queued tracker continuation. Both the
/// session-resume and in-session paths share this wording so the stated
/// consequence (the harness resumed; a recap does not advance the tracker)
/// cannot drift between them.
pub(crate) fn tracker_continue_directive(label: &str, incomplete: &[String]) -> String {
    format!(
        "{label}: task_tracker still has incomplete steps: {}. The harness queued this continuation, so no \
         user reply is needed. The next step is the next concrete tracker step; a status-only recap does not \
         advance the tracker while work remains.",
        incomplete.join(", ")
    )
}

/// Shared recoverable blocked-reason shapes for every mode's auto-continue.
/// Keep this the single allow-list so plan-mode and tracker classifiers cannot
/// drift (fuse / no-final / budget / recovery-fallback wording).
/// Deny lists stay per-mode: evaluation order differs (plan allows first so
/// `PLANNING_COMPLETED_TURN_FALLBACK_REASON` is not shadowed by
/// "approval-ready plan").
pub(crate) const RECOVERABLE_BLOCK_ALLOW_TOKENS: &[&str] = &[
    "recovery fallback",
    "recovery could not confirm",
    "recovery exhausted",
    "recovery was exhausted",
    "reached the safety cap",
    "safety cap",
    "preview budget",
    "tool preview budget",
    "turn budget",
    "tool budget",
    "tool loop budget",
    "tool loop",
    "tool-call budget",
    "tool follow-up",
    "tool-free recovery",
    "wall clock",
    "blocked due to repeated",
    "blocked after repeated",
    "without a harness-visible final assistant response",
    "blocked tool-call limit",
    "recovery tool-call limit",
    "consecutive blocked calls",
    "tool-call safety limit",
    "max tool",
    "per-turn tool",
    "read cap",
    "work budget",
    "budget exhausted",
    "budget ran out",
];

fn matches_recoverable_block_shape(lower: &str) -> bool {
    RECOVERABLE_BLOCK_ALLOW_TOKENS.iter().any(|token| lower.contains(token))
}

fn contains_any_token(lower: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| lower.contains(token))
}

/// Deny tokens shared by the tracker and plan-mode auto-continue classifiers.
/// They mirror production blocked-reason constants and true-handoff vocabulary:
/// `RECOVERY_CONTRACT_VIOLATION_REASON` ("final tool-free synthesis pass" /
/// "attempted more tool calls"), `PENDING_VERIFICATION_BLOCK_REASON`
/// ("verification is still pending"), `POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON`
/// ("compaction could not reduce"), and UNMATCHED_TOOL_RESULT. When one of those
/// constants' wording changes, update the token here in the same commit — the
/// auto-continue gates classify by substring.
const RECOVERABLE_BLOCK_BASE_DENY_TOKENS: &[&str] = &[
    "permission",
    "user input",
    "request_user_input",
    "interview",
    "verification is still pending",
    "compaction could not reduce",
    "unmatched tool result",
    "attempted more tool calls",
    "final tool-free synthesis pass",
];

/// Tracker auto-continue deny tokens beyond [`RECOVERABLE_BLOCK_BASE_DENY_TOKENS`].
const TRACKER_AUTO_CONTINUE_EXTRA_DENY_TOKENS: &[&str] = &[
    "safety fuse",
    "manual intervention",
    "unverified assistant responses",
    "anti-blind",
    "verification gate",
    "context exceeded",
    "stale recovery state",
    "awaiting approval",
    "approval-ready plan remains",
];

/// Budget-only verification block: `verification is still pending` caused by
/// tool-call budget exhaustion (fresh turn gets fresh execution budget, so a
/// verifier can run). Session `session-vtcode-20261008T094713Z` blocked on
/// exactly this shape and required manual `continue`; other verification
/// blocks (failing verifier, safety, permission) stay terminal.
pub(crate) fn is_budget_exhausted_verification_block(lower: &str) -> bool {
    lower.contains("verification is still pending")
        && (lower.contains("tool-call budget")
            || lower.contains("tool budget")
            || lower.contains("turn budget")
            || lower.contains("budget exhausted")
            || lower.contains("budget ran out"))
}

/// Whether a blocked/completed turn reason is recoverable for tracker auto-queue
/// (budget/preview/tool-free recovery) rather than a user-input handoff.
///
/// Matches **production blocked-reason constants** (turn_loop / post_tool
/// recovery), not paraphrases. Unknown / missing reasons are not auto-queued
/// when the turn did not complete.
pub(crate) fn tracker_auto_continue_is_recoverable_block(reason: Option<&str>) -> bool {
    // A provider refusal is terminal for the refused request: resending it is
    // refused again. Checked before the substring classifiers because the
    // notice may quote provider or response text containing any token.
    if reason.is_some_and(refusal::is_refusal_notice) {
        return false;
    }
    let Some(reason) = reason.map(str::to_ascii_lowercase) else {
        // Only used when the outer gate already marked the turn Completed.
        // Blocked { reason: None } must not auto-queue.
        return false;
    };
    // Budget-exhausted verification blocks can resume on a fresh turn with
    // fresh execution budget. Still deny when harder handoff signals are
    // present (permission / safety / compaction / contract violation).
    if is_budget_exhausted_verification_block(&reason) {
        let hard_deny = RECOVERABLE_BLOCK_BASE_DENY_TOKENS
            .iter()
            .filter(|token| **token != "verification is still pending")
            .chain(TRACKER_AUTO_CONTINUE_EXTRA_DENY_TOKENS.iter())
            .any(|token| reason.contains(token));
        return !hard_deny;
    }
    // Deny production constants that must never auto-queue (true handoffs).
    // RECOVERY_CONTRACT_VIOLATION_REASON: "...final tool-free synthesis pass...attempted more tool calls."
    // PENDING_VERIFICATION_BLOCK_REASON: "...verification is still pending."
    // POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON: "context exceeded...compaction could not reduce"
    // STALE_APPROVED_PLAN_PAUSE_BLOCK_REASON: "stale recovery state"
    // UNMATCHED_TOOL_RESULT / planning interview-approval handoffs / permission / safety fuse.
    if contains_any_token(&reason, RECOVERABLE_BLOCK_BASE_DENY_TOKENS)
        || contains_any_token(&reason, TRACKER_AUTO_CONTINUE_EXTRA_DENY_TOKENS)
    {
        return false;
    }
    matches_recoverable_block_shape(&reason)
}

/// Pure gate for outer-loop tracker auto-continue after a turn end.
///
/// `auto_continue_enabled` is the raw kill-switch (do not pre-AND
/// `planning_active` — this gate owns that check).
/// `final_text_is_safety_handoff` is true when the turn's final assistant
/// text is a permission/policy/safety handoff — those never auto-queue, even
/// on `Completed` ends with incomplete tracker work.
/// `final_text_requires_user_input` is true when the final text asks the user
/// a genuine question/decision — Completed ends must not auto-queue past the ask.
///
/// Completed turns require incomplete tracker work. Recoverable **blocked**
/// ends continue even without tracker items: session evidence shows
/// recovery-fallback and blocked-tool fuse turns parking at the user
/// `Continue…` prompt in build/auto when no tracker is active.
pub(crate) fn should_queue_tracker_auto_continue(
    auto_continue_enabled: bool,
    planning_active: bool,
    turn_completed: bool,
    blocked_reason: Option<&str>,
    is_verification_block: bool,
    incomplete_items: Option<&[String]>,
    cross_turn_turns: u8,
    final_text_is_safety_handoff: bool,
    final_text_requires_user_input: bool,
) -> bool {
    if !auto_continue_enabled || planning_active || cross_turn_turns == 0 {
        return false;
    }
    if final_text_is_safety_handoff || final_text_requires_user_input {
        return false;
    }
    if turn_completed {
        return incomplete_items.is_some_and(|items| !items.is_empty());
    }
    if blocked_reason.is_none() {
        return false;
    }
    if is_verification_block {
        // Budget-only verification blocks get a bounded fresh-turn retry
        // (fresh execution budget can run the verifier). Other verification
        // blocks keep their existing recovery path first.
        let lower = blocked_reason.map(str::to_ascii_lowercase).unwrap_or_default();
        if !is_budget_exhausted_verification_block(&lower) {
            return false;
        }
    }
    tracker_auto_continue_is_recoverable_block(blocked_reason)
}

/// Pure gate for resume auto-queue of incomplete tracker work.
pub(crate) fn should_queue_tracker_resume_continuation(
    auto_continue_enabled: bool,
    cross_turn_turns: u8,
    incomplete_items: Option<&[String]>,
) -> bool {
    auto_continue_enabled && cross_turn_turns > 0 && incomplete_items.is_some_and(|items| !items.is_empty())
}

/// Pure gate for plan-mode outer auto-continue.
///
/// Continues incomplete planning only on recoverable **blocked** ends when the
/// plan is not yet ready for approval. Ordinary completed planning turns are
/// never auto-continued (they may be interview or approval handoffs). Never
/// auto-approves.
///
/// `consecutive_empty_fallbacks` breaks the empty-turn self-loop: turns that
/// end with the deterministic `PLANNING_COMPLETED_FALLBACK_RESPONSE` (no LLM
/// synthesis, no tool activity) must not re-queue forever. After
/// [`MAX_PLAN_EMPTY_FALLBACK_AUTO_CONTINUE`] consecutive empties the gate
/// closes and the user must `continue` manually.
pub(crate) const MAX_PLAN_EMPTY_FALLBACK_AUTO_CONTINUE: u8 = 2;

/// Stable marker of the deterministic empty-turn fallback text in
/// `turn_loop::PLANNING_COMPLETED_FALLBACK_RESPONSE`. Matched by substring so
/// the gate stays pure (no cross-module constant import) and robust to
/// surrounding file-list appends.
pub(crate) fn is_plan_empty_fallback_text(text: &str) -> bool {
    text.contains("without a final plan synthesis")
        && text.contains("the next turn can reuse it without re-reading files")
}

pub(crate) fn should_queue_plan_mode_auto_continue(
    auto_continue_enabled: bool,
    planning_active: bool,
    plan_ready_for_approval: bool,
    turn_completed: bool,
    blocked_reason: Option<&str>,
    is_verification_block: bool,
    cross_turn_turns: u8,
    consecutive_empty_fallbacks: u8,
) -> bool {
    if !auto_continue_enabled || !planning_active || cross_turn_turns == 0 {
        return false;
    }
    if plan_ready_for_approval || is_verification_block || turn_completed {
        return false;
    }
    if consecutive_empty_fallbacks >= MAX_PLAN_EMPTY_FALLBACK_AUTO_CONTINUE {
        return false;
    }
    blocked_reason.is_some_and(plan_mode_recoverable_block)
}

/// Recoverable planning blocked-reason classifier.
///
/// Allow-list is evaluated **first**. Production
/// `PLANNING_COMPLETED_TURN_FALLBACK_REASON` ("Planning turn ended via
/// recovery fallback … approval-ready plan …") must auto-queue; a deny-first
/// match on "planning turn ended" / "approval-ready plan" incorrectly blocked
/// that path and forced a user `continue` nudge. Interview/approval/permission
/// handoffs stay denied after the allow-list misses.
pub(crate) fn plan_mode_recoverable_block(reason: &str) -> bool {
    // Refusals never auto-continue; see `tracker_auto_continue_is_recoverable_block`.
    if refusal::is_refusal_notice(reason) {
        return false;
    }
    let lower = reason.to_ascii_lowercase();
    // Budget-only verification blocks can retry on a fresh turn, same as the
    // tracker gate. Other verification blocks stay terminal.
    if is_budget_exhausted_verification_block(&lower) {
        return !lower.contains("awaiting");
    }
    // True handoffs deny even when recovery/budget tokens are also present
    // (compound reasons must not auto-queue past a permission/interview wait).
    // "awaiting" is deliberately broader than the tracker's "awaiting approval".
    if contains_any_token(&lower, RECOVERABLE_BLOCK_BASE_DENY_TOKENS) || lower.contains("awaiting") {
        return false;
    }
    // Production recovery constants (including PLANNING_COMPLETED_TURN_FALLBACK_REASON)
    // are recoverable. Deny tokens like "planning turn ended" / "approval-ready
    // plan" must not shadow "recovery fallback".
    //
    // Entry-turn Blocked shapes common after mid-turn `start_planning`: the
    // model hits the read-only gate (blocked-tool fuse) or ends tools without a
    // published final. Both must auto-continue planning research instead of
    // parking at the user `Continue…` prompt.
    if matches_recoverable_block_shape(&lower) {
        return true;
    }
    // Remaining planning handoffs (interview/approval without recovery tokens).
    if lower.contains("planning turn ended") || lower.contains("approval-ready plan") {
        return false;
    }
    false
}

/// User-facing plan progress line (title + phase/status only).
pub(crate) fn plan_progress_line(
    title: &str,
    ready_for_approval: bool,
    open_decisions: usize,
    step_count: usize,
) -> String {
    let label = title.trim();
    let name = if label.is_empty() { None } else { Some(label) };
    if ready_for_approval {
        return match name {
            Some(name) if step_count > 0 => format!("• Plan {name} — ready for approval ({step_count} steps)"),
            Some(name) => format!("• Plan {name} — ready for approval"),
            None if step_count > 0 => format!("• Plan — ready for approval ({step_count} steps)"),
            None => "• Plan — ready for approval".to_string(),
        };
    }
    if open_decisions > 0 {
        return match name {
            Some(name) => format!("• Plan {name} — open decisions: {open_decisions}"),
            None => format!("• Plan — open decisions: {open_decisions}"),
        };
    }
    match name {
        Some(name) => format!("• Plan {name} — research/synthesis"),
        None => "• Plan — research/synthesis".to_string(),
    }
}

/// Stable opening marker for the harness-generated plan-mode auto-continue
/// directive. The planning exit trigger treats any user message carrying this
/// marker as machine-generated (not a genuine user turn), so the two sites
/// must share one literal instead of drifting.
pub(crate) const PLAN_MODE_AUTO_CONTINUE_MARKER: &str = "Plan-mode auto-continue:";

/// Shared tail of every plan-mode continuation message. States the
/// consequence (planning stays read-only, the harness resumed the turn, code
/// changes wait for approval) and the next step. It deliberately contains the
/// stay phrase `continue planning` and no implementation cue, so even without
/// the [`PLAN_MODE_AUTO_CONTINUE_MARKER`] guard it could never read as an
/// exit-and-implement intent.
const PLAN_MODE_CONTINUE_DIRECTIVE_TAIL: &str = "Planning stays active and read-only, so the next step is to continue \
planning: read-only research and synthesis toward one compact `<proposed_plan>`. The harness queued this \
continuation, so no user reply is needed, and code changes wait for plan approval.";

/// Follow-up prompt for plan-mode auto-continue turns.
pub(crate) fn plan_mode_continue_follow_up() -> String {
    format!(
        "{PLAN_MODE_AUTO_CONTINUE_MARKER} no validated persisted plan is ready for approval yet. \
{PLAN_MODE_CONTINUE_DIRECTIVE_TAIL}"
    )
}

/// System directive paired with an in-session plan-mode auto-continue.
pub(crate) fn plan_mode_auto_continue_directive() -> String {
    format!(
        "{PLAN_MODE_AUTO_CONTINUE_MARKER} planning remains active and no validated plan is ready for approval. \
{PLAN_MODE_CONTINUE_DIRECTIVE_TAIL}"
    )
}

/// System directive paired with a plan-mode continuation queued on session
/// resume after a recoverable blocked handoff.
pub(crate) fn plan_mode_resume_directive() -> String {
    format!(
        "Resume continuation: planning remains active after a recoverable blocked handoff. \
{PLAN_MODE_CONTINUE_DIRECTIVE_TAIL}"
    )
}
