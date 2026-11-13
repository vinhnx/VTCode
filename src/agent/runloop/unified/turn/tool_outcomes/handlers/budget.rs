use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

pub(crate) fn record_tool_call_usage(ctx: &mut TurnProcessingContext<'_>) {
    ctx.harness_state.record_admitted_tool_call();
    ctx.harness_state.record_tool_call();
}
