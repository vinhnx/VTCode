//! Plan-mode synthesis truncation recovery.
//!
//! When a planning-mode synthesis is cut off at the model's output token limit
//! (an unclosed `<proposed_plan>`), the emitted plan is incomplete ("cut off
//! mid-flight"). The previous verbose plan format exceeded the limit and was
//! truncated, which re-triggered the recovery loop forever.
//!
//! This module owns that policy so `turn_loop.rs` no longer carries plan-mode
//! specifics: [`plan_synthesis_was_truncated`] detects the truncated
//! synthesis, and [`maybe_condense_truncated_plan`] injects the compact-spec
//! directive and re-runs the turn loop exactly once (bounded, so a genuinely
//! oversized plan cannot loop).

use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Injected when a planning synthesis is truncated, asking the model to emit a
/// single compact spec that fits the limit.
pub(crate) const PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE: &str = "Your previous `<proposed_plan>` was cut off at the token limit. Re-emit ONE compact `<proposed_plan>` spec that fits within the limit: keep each step to a single line (`Action -> files/symbols -> verify:`), drop prose, and prefer file:symbol references. Do not repeat the truncated draft.";

/// Maximum number of condense-and-retry passes when a planning synthesis is
/// truncated. Bounded so a genuinely oversized plan cannot loop forever.
const MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS: u8 = 1;

/// Detect a planning synthesis that was cut off at the model's output token
/// limit: the response ended with `finish_reason == Length`, carried no tool
/// calls, and left a `<proposed_plan>` block unclosed. Such a partial plan is
/// what previously re-triggered the recovery loop forever.
pub(crate) fn plan_synthesis_was_truncated(response: &uni::LLMResponse) -> bool {
    matches!(response.finish_reason, uni::FinishReason::Length)
        && response
            .tool_calls
            .as_ref()
            .is_none_or(|calls| calls.is_empty())
        && response.content.as_deref().is_some_and(|text| {
            text.contains("<proposed_plan") && !text.contains("</proposed_plan>")
        })
}

/// If the planning synthesis was truncated, inject the condense directive into
/// `working_history`, render a notice, and return `true` so the caller re-runs
/// the turn loop. The retry is bounded by [`MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS`]
/// via `attempts`, and is suppressed during the tool-free recovery pass (where a
/// truncated synthesis falls into the post-tool recovery cycle cap instead).
///
/// Returns `false` (no continue) when the conditions are not met.
pub(crate) fn maybe_condense_truncated_plan(
    working_history: &mut Vec<uni::Message>,
    renderer: &mut AnsiRenderer,
    planning_active: bool,
    tool_free_recovery: bool,
    attempts: &mut u8,
    response: &uni::LLMResponse,
) -> bool {
    if tool_free_recovery
        || !planning_active
        || !plan_synthesis_was_truncated(response)
        || *attempts >= MAX_PLAN_SYNTHESIS_CONDENSE_ATTEMPTS
    {
        return false;
    }

    *attempts += 1;
    working_history.push(uni::Message::system(
        PLANNING_SYNTHESIS_TRUNCATED_CONDENSE_DIRECTIVE.to_string(),
    ));
    let _ = renderer.line(
        MessageStyle::Info,
        "Plan was truncated at the token limit; requesting a more compact spec.",
    );
    true
}
