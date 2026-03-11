use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use vtcode_core::llm::provider as uni;

const TOOL_BUDGET_WARNING_THRESHOLD: f64 = 0.75;

fn build_tool_budget_warning_message(used: usize, max: usize, remaining: usize) -> String {
    format!(
        "Tool-call budget warning: {used}/{max} used; {remaining} remaining for this turn. Use targeted extraction/batching before additional tool calls."
    )
}

pub(crate) fn build_tool_budget_exhausted_reason(used: usize, max: usize) -> String {
    format!(
        "Tool-call budget exhausted for this turn ({used}/{max}). Start a new turn with \"continue\" or provide a new instruction to proceed."
    )
}

pub(crate) fn record_tool_call_budget_usage(ctx: &mut TurnProcessingContext<'_>) {
    ctx.harness_state.record_tool_call();
    if ctx
        .harness_state
        .should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD)
    {
        let used = ctx.harness_state.tool_calls;
        let max = ctx.harness_state.max_tool_calls;
        let remaining = ctx.harness_state.remaining_tool_calls();
        ctx.working_history
            .push(uni::Message::system(build_tool_budget_warning_message(
                used, max, remaining,
            )));
        ctx.harness_state.mark_tool_budget_warning_emitted();
    }
}
