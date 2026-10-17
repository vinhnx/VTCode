use crate::agent::runloop::unified::turn::context::{PreparedAssistantToolCall, TurnHandlerOutcome};
use anyhow::Result;
use call::handle_prepared_tool_call_dispatch;
use std::time::Instant;

mod call;

pub(crate) async fn handle_tool_calls<'a, 'b>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, 'b>,
    tool_calls: &[PreparedAssistantToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    if tool_calls.is_empty() {
        return Ok(None);
    }
    t_ctx.ctx.harness_state.record_requested_tool_calls(tool_calls.len());
    if let Some(stop) = crate::agent::runloop::unified::stop_requests::stop_outcome(t_ctx.ctx.ctrl_c_state) {
        super::handlers::drain_preflight_circuit_responses(t_ctx.ctx, tool_calls);
        return Ok(Some(TurnHandlerOutcome::Break(stop)));
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
        for (index, tool_call) in tool_calls.iter().enumerate() {
            if let Some(err) = tool_call.args_error() {
                if let Some(outcome) = super::handlers::handle_preflight_failure(
                    t_ctx.ctx,
                    tool_call.call_id(),
                    tool_call.tool_name(),
                    err,
                    None,
                    None,
                ) {
                    super::handlers::drain_preflight_circuit_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                    super::handlers::flush_preflight_circuit_recovery(t_ctx.ctx);
                    return Ok(Some(outcome));
                }
            }
        }
        return Ok(None);
    }

    let batch_candidate = valid_calls > 1;
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
        let outcome = crate::agent::runloop::unified::turn::tool_outcomes::handlers::handle_tool_call_batch_prepared(
            t_ctx, tool_calls,
        )
        .await?;
        if let Some(o) = outcome {
            flush_dispatch_recovery(t_ctx.ctx);
            return Ok(Some(o));
        }
    } else {
        for (index, tool_call) in tool_calls.iter().enumerate() {
            if let Some(err) = tool_call.args_error() {
                if let Some(outcome) = super::handlers::handle_preflight_failure(
                    t_ctx.ctx,
                    tool_call.call_id(),
                    tool_call.tool_name(),
                    err,
                    None,
                    None,
                ) {
                    super::handlers::drain_preflight_circuit_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                    flush_dispatch_recovery(t_ctx.ctx);
                    return Ok(Some(outcome));
                }
                continue;
            }

            let outcome = handle_prepared_tool_call_dispatch(t_ctx, tool_call).await?;
            if let Some(o) = outcome {
                if t_ctx.ctx.harness_state.blocked_tool_recovery_pending() {
                    super::handlers::drain_blocked_tool_recovery_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                }
                if matches!(
                    &o,
                    TurnHandlerOutcome::Break(
                        crate::agent::runloop::unified::turn::context::TurnLoopResult::Cancelled
                            | crate::agent::runloop::unified::turn::context::TurnLoopResult::Exit
                    )
                ) || t_ctx.ctx.session_stats.preflight_recovery.exhausted()
                    || t_ctx.ctx.harness_state.consecutive_preflight_failures
                        >= super::handlers::max_consecutive_blocked_tool_calls_per_turn(t_ctx.ctx)
                {
                    super::handlers::drain_preflight_circuit_responses(t_ctx.ctx, &tool_calls[index + 1..]);
                }
                flush_dispatch_recovery(t_ctx.ctx);
                return Ok(Some(o));
            }
        }
    }

    flush_dispatch_recovery(t_ctx.ctx);

    Ok(None)
}

/// Flush both post-dispatch recovery paths. Every dispatch exit runs these in
/// this order so interview-denial recovery and preflight-circuit recovery are
/// armed consistently regardless of which path produced the outcome.
fn flush_dispatch_recovery(ctx: &mut crate::agent::runloop::unified::turn::context::TurnProcessingContext<'_>) {
    super::execution_result::flush_auto_permission_probe_warning(ctx);
    super::handlers::flush_session_limit_grant_directive(ctx);
    super::handlers::flush_interview_denial_recovery(ctx);
    super::handlers::flush_preflight_circuit_recovery(ctx);
    super::handlers::flush_blocked_tool_recovery(ctx);
}
