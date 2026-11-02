//! Anti-runaway text-response cap guard for the turn loop.

use super::context::TurnLoopContext;
use super::responses::{
    ASSISTANT_TEXT_RESPONSE_CAP_REASON, MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN, promote_latest_commentary_to_final,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

/// Enforce the consecutive-assistant-text cap.
///
/// Returns `Some(result)` when the cap trips (caller breaks with it);
/// returns `None` to continue the turn loop.
pub(super) fn enforce_text_response_cap(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut [uni::Message],
    turn_history_start_len: usize,
) -> Option<TurnLoopResult> {
    // Anti-runaway guard: if the model has already emitted
    // MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN consecutive text-only
    // responses without admitted tool progress, the recovery / continuation
    // loop is regenerating the same answer. Conclude the turn with the
    // last response stored in working_history and emit a warning so users
    // can see what happened. See MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN for
    // the observed failure mode (checkpoint turn_594: 4 identical
    // 6500-token outline responses, ~90s of wasted context).
    // Harness state is authoritative. History cannot represent Copilot's
    // inline VTCode tool calls, while compaction does not discard this
    // turn-scoped counter.
    let text_response_streak = ctx.harness_state.consecutive_assistant_text_responses;
    // A pending bounded planning retry (validation repair, denied-interview
    // synthesis, pseudo-tool-call reprompt) is a deliberate extra candidate,
    // so allow the next request through the generic text-response cap. This
    // allowance is queued independently of the response count because a
    // retry can be scheduled after an ordinary planning response has
    // already used that budget.
    let bounded_planning_follow_up = ctx.is_planning_active()
        && (ctx.plan_session.bounded_planning_follow_up_allowed() || ctx.plan_session.recovery.research_closed);
    if text_response_streak >= MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN && !bounded_planning_follow_up {
        tracing::warn!(
            text_response_streak,
            cap = MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN,
            "Assistant text-response cap reached; ending turn to prevent runaway regeneration loop"
        );
        let _ = ctx.renderer.line(
            MessageStyle::Warning,
            "Recovery loop detected: capping repeated assistant responses to avoid wasted context.",
        );
        let promoted_final = promote_latest_commentary_to_final(working_history, turn_history_start_len);
        if promoted_final {
            // Commentary has already crossed the renderer surface. Avoid
            // rendering it a second time while still allowing the normal
            // blocked-turn finalizer to publish the canonical event.
            ctx.harness_state.mark_final_response_rendered();
            if ctx.harness_state.streamed_response_event_emitted() {
                // The streaming lifecycle already emitted the assistant
                // item; publishing another AgentMessage would duplicate it.
                ctx.harness_state.mark_final_response_event_emitted();
            }
        }
        // A preserved final answer with a clear verification gate is a
        // completed turn: the model concluded and the loop merely
        // re-prompted past its answer. Blocked/handoff semantics stay
        // reserved for unverified work (checkpoint session-vtcode-20260912
        // T083718Z: a verified recap was continued into the cap and the
        // finished turn was published as Blocked).
        let cap_ends_completed =
            promoted_final && !ctx.tool_registry.completion_verification_pending() && !ctx.is_planning_active();
        let capped = if cap_ends_completed {
            TurnLoopResult::Completed { plan_approved_execution_pending: false }
        } else {
            TurnLoopResult::Blocked {
                reason: Some(ASSISTANT_TEXT_RESPONSE_CAP_REASON.to_string()),
            }
        };
        return Some(capped);
    }
    if bounded_planning_follow_up {
        ctx.plan_session.consume_bounded_planning_follow_up();
    }
    None
}
