use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use vtcode_core::llm::provider as uni;

const TOOL_BUDGET_WARNING_THRESHOLD: f64 = 0.75;

fn build_tool_budget_warning_message(used: usize, max: usize, remaining: usize) -> String {
    format!(
        "Tool-call budget warning: {used}/{max} used; {remaining} remaining for this turn. Use targeted extraction/batching before additional tool calls."
    )
}

pub(crate) fn build_tool_budget_exhausted_reason(used: usize, max: usize) -> String {
    debug_assert!(max > 0, "disabled tool-call caps must not emit exhaustion");
    format!(
        "Tool-call budget exhausted for this turn ({used}/{max}). Start a new turn with \"continue\" or provide a new instruction to proceed."
    )
}

pub(crate) fn record_tool_call_budget_usage(ctx: &mut TurnProcessingContext<'_>) {
    if let Some(warning) = ctx
        .harness_state
        .record_tool_call_with_warning(TOOL_BUDGET_WARNING_THRESHOLD)
    {
        ctx.working_history
            .push(uni::Message::system(build_tool_budget_warning_message(
                warning.used,
                warning.max,
                warning.remaining,
            )));
    }
}
