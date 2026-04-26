use anyhow::Result;
use std::time::Duration;

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use super::super::helpers::push_tool_response;
use super::ValidationResult;

const MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS: usize = 4;
const MAX_RATE_LIMIT_WAIT: Duration =
    Duration::from_secs(vtcode_config::constants::execution::MAX_RATE_LIMIT_WAIT_SECS);

fn build_rate_limit_error_content(tool_name: &str, retry_after_ms: u64) -> String {
    serde_json::json!({
        "error": format!(
            "Tool '{}' is temporarily rate limited. Try again after a short delay.",
            tool_name
        ),
        "failure_kind": "rate_limit",
        "rate_limited": true,
        "retry_after_ms": retry_after_ms,
    })
    .to_string()
}

pub(crate) async fn acquire_adaptive_rate_limit_slot<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
) -> Result<Option<ValidationResult>> {
    for attempt in 0..MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS {
        let acquire_result = ctx.rate_limiter.try_acquire(tool_name);

        match acquire_result {
            Ok(_) => return Ok(None),
            Err(wait_time) => {
                if ctx.ctrl_c_state.is_cancel_requested() {
                    return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                        TurnLoopResult::Cancelled,
                    ))));
                }
                if ctx.ctrl_c_state.is_exit_requested() {
                    return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                        TurnLoopResult::Exit,
                    ))));
                }

                let bounded_wait = wait_time.min(MAX_RATE_LIMIT_WAIT);
                if attempt + 1 >= MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS {
                    let retry_after_ms = bounded_wait.as_millis() as u64;
                    tracing::warn!(
                        tool = %tool_name,
                        attempts = MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS,
                        retry_after_ms,
                        "Adaptive rate limiter blocked tool execution after repeated attempts"
                    );
                    push_tool_response(
                        ctx.working_history,
                        tool_call_id,
                        build_rate_limit_error_content(tool_name, retry_after_ms),
                    );
                    return Ok(Some(ValidationResult::Blocked));
                }

                if bounded_wait.is_zero() {
                    tokio::task::yield_now().await;
                    continue;
                }

                tokio::select! {
                    _ = tokio::time::sleep(bounded_wait) => {},
                    _ = ctx.ctrl_c_notify.notified() => {
                        if ctx.ctrl_c_state.is_exit_requested() {
                            return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                                TurnLoopResult::Exit,
                            ))));
                        }
                        if ctx.ctrl_c_state.is_cancel_requested() {
                            return Ok(Some(ValidationResult::Outcome(TurnHandlerOutcome::Break(
                                TurnLoopResult::Cancelled,
                            ))));
                        }
                    }
                }
            }
        }
    }

    Ok(Some(ValidationResult::Blocked))
}
