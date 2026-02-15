//! Tool outcome handling helpers for turn execution.

use anyhow::Result;
use std::time::Duration;

use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::tool_call_safety::SafetyError;
use crate::agent::runloop::unified::tool_routing::{
    ensure_tool_permission, prompt_session_limit_increase,
};
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use super::helpers::push_tool_response;
use crate::agent::runloop::unified::tool_routing::ToolPermissionFlow;
#[path = "handlers_batch.rs"]
mod handlers_batch;
pub(crate) use handlers_batch::{execute_and_handle_tool_call, handle_tool_call_batch};

/// Result of a tool call validation phase.
pub(crate) enum ValidationResult {
    /// Proceed with execution
    Proceed(PreparedToolCall),
    /// Tool was blocked or handled internally (e.g. error pushed to history), skip execution but continue turn
    Blocked,
    /// Stop turn/loop with a specific outcome (e.g. Exit or Cancel)
    Outcome(TurnHandlerOutcome),
}

/// Canonicalized validation data reused across the execution path.
pub(crate) struct PreparedToolCall {
    pub canonical_name: String,
    pub readonly_classification: bool,
}

const MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS: usize = 4;
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(5);

fn build_failure_error_content(error: String, failure_kind: &'static str) -> String {
    super::execution_result::build_error_content(error, None, failure_kind).to_string()
}

fn build_validation_error_content(error: String, validation_stage: &'static str) -> String {
    // Validation errors have additional fields, so we construct them directly
    serde_json::json!({
        "error": error,
        "failure_kind": "validation",
        "validation_stage": validation_stage,
        "retryable": false,
    })
    .to_string()
}

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

/// Consolidated state for tool outcomes to reduce signature bloat and ensure DRY across handlers.
pub struct ToolOutcomeContext<'a, 'b> {
    pub ctx: &'b mut TurnProcessingContext<'a>,
    pub repeated_tool_attempts: &'b mut super::helpers::LoopTracker,
    pub turn_modified_files: &'b mut std::collections::BTreeSet<std::path::PathBuf>,
}

/// Unified handler for a single tool call (whether native or textual).
///
/// This handler applies the full pipeline of checks:
/// 1. Circuit Breaker
/// 2. Rate Limiting
/// 3. Loop Detection
/// 4. Safety Validation (with potential user interaction for limits)
/// 5. Permission Checking (Allow/Deny/Ask)
/// 6. Execution (with progress spinners and PTY streaming)
/// 7. Result Handling (recording metrics, history, UI output)
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_single_tool_call<'a, 'b, 'tool>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call_id: String,
    tool_name: &'tool str,
    args_val: serde_json::Value,
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.harness_state.set_phase(TurnPhase::ExecutingTools);

    // 1. Validate (Circuit Breaker, Rate Limit, Loop Detection, Safety, Permission)
    let prepared = match validate_tool_call(t_ctx.ctx, &tool_call_id, tool_name, &args_val).await? {
        ValidationResult::Outcome(outcome) => return Ok(Some(outcome)),
        ValidationResult::Blocked => return Ok(None),
        ValidationResult::Proceed(prepared) => prepared,
    };

    // 3. Execute and Handle Result
    execute_and_handle_tool_call(
        t_ctx.ctx,
        t_ctx.repeated_tool_attempts,
        t_ctx.turn_modified_files,
        tool_call_id,
        &prepared.canonical_name,
        args_val,
        None,
    )
    .await?;

    Ok(None)
}

/// Validates a tool call against all safety and permission checks.
/// Returns Some(TurnHandlerOutcome) if the turn loop should break/exit/cancel.
/// Returns None if execution should proceed (or if a local error was already handled/pushed).
pub(crate) async fn validate_tool_call<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
) -> Result<ValidationResult> {
    if ctx.harness_state.tool_budget_exhausted() {
        let error_msg = format!(
            "Policy violation: exceeded max tool calls per turn ({})",
            ctx.harness_state.max_tool_calls
        );
        push_tool_response(
            ctx.working_history,
            tool_call_id.to_string(),
            build_failure_error_content(error_msg, "policy"),
        );
        return Ok(ValidationResult::Blocked);
    }

    if ctx.harness_state.wall_clock_exhausted() {
        let error_msg = format!(
            "Policy violation: exceeded tool wall clock budget ({}s)",
            ctx.harness_state.max_tool_wall_clock.as_secs()
        );
        push_tool_response(
            ctx.working_history,
            tool_call_id.to_string(),
            build_failure_error_content(error_msg, "policy"),
        );
        return Ok(ValidationResult::Blocked);
    }

    ctx.harness_state.record_tool_call();

    let preflight = match ctx
        .tool_registry
        .preflight_validate_call(tool_name, args_val)
    {
        Ok(preflight) => preflight,
        Err(err) => {
            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                build_validation_error_content(
                    format!("Tool preflight validation failed: {}", err),
                    "preflight",
                ),
            );
            return Ok(ValidationResult::Blocked);
        }
    };
    let canonical_tool_name = preflight.normalized_tool_name.clone();

    // Phase 4 Check: Per-tool Circuit Breaker
    if !ctx
        .circuit_breaker
        .allow_request_for_tool(&canonical_tool_name)
    {
        let error_msg = format!(
            "Tool '{}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).",
            canonical_tool_name
        );
        tracing::warn!(tool = %canonical_tool_name, "Circuit breaker open, tool disabled");
        push_tool_response(
            ctx.working_history,
            tool_call_id.to_string(),
            build_failure_error_content(error_msg, "circuit_breaker"),
        );
        return Ok(ValidationResult::Blocked);
    }

    // Phase 4 Check: Adaptive Rate Limiter
    if let Some(outcome) =
        acquire_adaptive_rate_limit_slot(ctx, tool_call_id, &canonical_tool_name).await?
    {
        return Ok(outcome);
    }

    // Phase 4 Check: Adaptive Loop Detection
    if let Some(warning) = ctx
        .autonomous_executor
        .record_tool_call(&canonical_tool_name, args_val)
    {
        let should_block = {
            if let Ok(detector) = ctx.autonomous_executor.loop_detector().read() {
                detector.is_hard_limit_exceeded(&canonical_tool_name)
            } else {
                false
            }
        };

        if should_block {
            tracing::warn!(tool = %canonical_tool_name, "Loop detector blocked tool");
            if let Some(mut spooled) = ctx.tool_registry.find_recent_spooled_output(
                &canonical_tool_name,
                args_val,
                Duration::from_secs(120),
            ) {
                if let Some(obj) = spooled.as_object_mut() {
                    obj.insert(
                        "reused_spooled_output".to_string(),
                        serde_json::Value::Bool(true),
                    );
                    obj.insert("loop_detected".to_string(), serde_json::Value::Bool(true));
                    obj.insert(
                        "loop_detected_note".to_string(),
                        serde_json::Value::String(
                            "Loop detected; reusing spooled output from a recent identical call. Read the spooled file instead of re-running the tool.".to_string(),
                        ),
                    );
                }
                push_tool_response(
                    ctx.working_history,
                    tool_call_id.to_string(),
                    spooled.to_string(),
                );
                return Ok(ValidationResult::Blocked);
            }

            let error_msg = format!(
                "Tool '{}' is blocked due to excessive repetition (Loop Detected).",
                canonical_tool_name
            );
            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                build_failure_error_content(error_msg, "loop_detection"),
            );
            return Ok(ValidationResult::Blocked);
        } else {
            tracing::warn!(tool = %canonical_tool_name, warning = %warning, "Loop detector warning");
        }
    }

    // Safety Validation Loop
    loop {
        let validation_result = {
            let mut validator = ctx.safety_validator.write().await;
            validator
                .validate_call(&canonical_tool_name, args_val)
                .await
        };

        match validation_result {
            Ok(_) => break,
            Err(SafetyError::SessionLimitReached { max }) => {
                match prompt_session_limit_increase(
                    ctx.handle,
                    ctx.session,
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    max,
                )
                .await
                {
                    Ok(Some(increment)) => {
                        let mut validator = ctx.safety_validator.write().await;
                        validator.increase_session_limit(increment);
                        continue;
                    }
                    _ => {
                        push_tool_response(
                            ctx.working_history,
                            tool_call_id.to_string(),
                            build_failure_error_content(
                                "Session tool limit reached and not increased by user".to_string(),
                                "safety_limit",
                            ),
                        );
                        return Ok(ValidationResult::Blocked);
                    }
                }
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Safety validation failed: {}", err),
                )?;
                push_tool_response(
                    ctx.working_history,
                    tool_call_id.to_string(),
                    build_failure_error_content(
                        format!("Safety validation failed: {}", err),
                        "safety_validation",
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
        }
    }

    // Ensure tool permission
    match ensure_tool_permission(
        crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
            tool_registry: ctx.tool_registry,
            renderer: ctx.renderer,
            handle: ctx.handle,
            session: ctx.session,
            default_placeholder: ctx.default_placeholder.clone(),
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            hooks: ctx.lifecycle_hooks,
            justification: None,
            approval_recorder: Some(ctx.approval_recorder.as_ref()),
            decision_ledger: Some(ctx.decision_ledger),
            tool_permission_cache: Some(ctx.tool_permission_cache),
            hitl_notification_bell: ctx
                .vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
            autonomous_mode: ctx.session_stats.is_autonomous_mode(),
            human_in_the_loop: ctx
                .vt_cfg
                .map(|cfg| cfg.security.human_in_the_loop)
                .unwrap_or(true),
            delegate_mode: ctx.session_stats.is_delegate_mode(),
            skip_confirmations: false, // Normal tool calls should prompt if configured
        },
        &canonical_tool_name,
        Some(args_val),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => Ok(ValidationResult::Proceed(PreparedToolCall {
            canonical_name: canonical_tool_name,
            readonly_classification: preflight.readonly_classification,
        })),
        Ok(ToolPermissionFlow::Denied) => {
            let denial = ToolExecutionError::new(
                canonical_tool_name,
                ToolErrorType::PolicyViolation,
                format!(
                    "Tool '{}' execution denied by policy",
                    preflight.normalized_tool_name
                ),
            )
            .to_json_value();

            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
            );
            Ok(ValidationResult::Blocked)
        }
        Ok(ToolPermissionFlow::Exit) => Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(
            TurnLoopResult::Exit,
        ))),
        Ok(ToolPermissionFlow::Interrupted) => Ok(ValidationResult::Outcome(
            TurnHandlerOutcome::Break(TurnLoopResult::Cancelled),
        )),
        Err(err) => {
            let err_json = serde_json::json!({
                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
            });
            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                err_json.to_string(),
            );
            Ok(ValidationResult::Blocked)
        }
    }
}

async fn acquire_adaptive_rate_limit_slot<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
) -> Result<Option<ValidationResult>> {
    for attempt in 0..MAX_RATE_LIMIT_ACQUIRE_ATTEMPTS {
        match ctx.rate_limiter.try_acquire(tool_name) {
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
                        tool_call_id.to_string(),
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
