use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome,
};
use anyhow::Result;
use call::{handle_prepared_tool_call_dispatch, push_invalid_tool_args_response};
use std::time::Instant;

mod call;

pub(crate) async fn handle_tool_calls<'a, 'b>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, 'b>,
    tool_calls: &[PreparedAssistantToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    if tool_calls.is_empty() {
        return Ok(None);
    }

    let planning_started = Instant::now();
    let mut valid_calls = 0usize;
    let mut parallel_safe_calls = 0usize;
    for tool_call in tool_calls {
        if tool_call.args().is_some() {
            valid_calls += 1;
        }
        if tool_call.is_parallel_safe() {
            parallel_safe_calls += 1;
        }
    }

    if valid_calls == 0 {
        for tool_call in tool_calls {
            if let Some(err) = tool_call.args_error() {
                push_invalid_tool_args_response(
                    t_ctx.ctx.working_history,
                    tool_call.call_id(),
                    tool_call.tool_name(),
                    err,
                );
            }
        }
        return Ok(None);
    }

    let batch_candidate = t_ctx.ctx.full_auto && valid_calls > 1;
    tracing::debug!(
        target: "vtcode.turn.metrics",
        metric = "tool_dispatch_plan",
        total_calls = valid_calls,
        batch_candidate,
        parallel_safe_calls,
        planning_ms = planning_started.elapsed().as_millis(),
        "turn metric"
    );

    if batch_candidate {
        let outcome = crate::agent::runloop::unified::turn::tool_outcomes::handlers::handle_tool_call_batch_prepared(t_ctx, tool_calls).await?;
        if let Some(o) = outcome {
            return Ok(Some(o));
        }
    } else {
        for tool_call in tool_calls {
            if let Some(err) = tool_call.args_error() {
                push_invalid_tool_args_response(
                    t_ctx.ctx.working_history,
                    tool_call.call_id(),
                    tool_call.tool_name(),
                    err,
                );
                continue;
            }

            let outcome = handle_prepared_tool_call_dispatch(t_ctx, tool_call).await?;
            if let Some(o) = outcome {
                return Ok(Some(o));
            }
        }
    }

    Ok(None)
}
