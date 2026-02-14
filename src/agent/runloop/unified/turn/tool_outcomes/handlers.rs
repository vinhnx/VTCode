//! Tool outcome handling helpers for turn execution.

use anyhow::Result;
use std::time::Duration;
use vtcode_core::llm::provider as uni;

use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_call_safety::SafetyError;
use crate::agent::runloop::unified::tool_pipeline::run_tool_call_with_args;
use crate::agent::runloop::unified::tool_routing::{
    ensure_tool_permission, prompt_session_limit_increase,
};
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};
use vtcode_core::config::constants::tools;

use super::execution_result::handle_tool_execution_result;
use super::helpers::{push_tool_response, resolve_max_tool_retries, update_repetition_tracker};
use crate::agent::runloop::unified::tool_routing::ToolPermissionFlow;

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

fn can_parallelize_batch_tool_call(prepared: &PreparedToolCall) -> bool {
    if matches!(
        prepared.canonical_name.as_str(),
        tools::ENTER_PLAN_MODE
            | tools::EXIT_PLAN_MODE
            | tools::ASK_USER_QUESTION
            | tools::REQUEST_USER_INPUT
            | tools::ASK_QUESTIONS
            | tools::RUN_PTY_CMD
            | tools::UNIFIED_EXEC
            | tools::SEND_PTY_INPUT
            | tools::SHELL
    ) {
        return false;
    }
    prepared.readonly_classification
}

pub(crate) async fn handle_tool_call_batch<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_calls: &[&uni::ToolCall],
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.harness_state.set_phase(TurnPhase::ExecutingTools);

    let mut validated_calls = Vec::new();

    // 1. Validate all calls sequentially (safety first)
    for tool_call in tool_calls {
        let func = match tool_call.function.as_ref() {
            Some(f) => f,
            None => continue, // Skip non-function calls
        };
        let tool_name = func.name.as_str();
        let args_val: serde_json::Value = match serde_json::from_str(&func.arguments) {
            Ok(args) => args,
            Err(err) => {
                push_tool_response(
                    t_ctx.ctx.working_history,
                    tool_call.id.clone(),
                    serde_json::json!({
                        "error": format!(
                            "Invalid tool arguments for '{}': {}",
                            tool_name,
                            err
                        )
                    })
                    .to_string(),
                );
                continue;
            }
        };

        match validate_tool_call(t_ctx.ctx, &tool_call.id, tool_name, &args_val).await? {
            ValidationResult::Outcome(outcome) => return Ok(Some(outcome)),
            ValidationResult::Blocked => continue,
            ValidationResult::Proceed(prepared) => {
                validated_calls.push((tool_call, prepared, args_val));
            }
        }
    }

    if validated_calls.is_empty() {
        return Ok(None);
    }

    let can_parallelize = validated_calls
        .iter()
        .all(|(_, prepared, _)| can_parallelize_batch_tool_call(prepared));
    if !can_parallelize {
        for (tool_call, prepared, args_val) in validated_calls {
            execute_and_handle_tool_call(
                t_ctx.ctx,
                t_ctx.repeated_tool_attempts,
                t_ctx.turn_modified_files,
                tool_call.id.clone(),
                &prepared.canonical_name,
                args_val,
                None,
            )
            .await?;
        }
        return Ok(None);
    }

    // 2. Parallel Execution
    let progress_reporter = ProgressReporter::new();
    let validated_call_count = validated_calls.len();
    let _spinner =
        crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
            t_ctx.ctx.handle,
            t_ctx.ctx.input_status_state.left.clone(),
            t_ctx.ctx.input_status_state.right.clone(),
            format!("Executing {} tools...", validated_call_count),
            Some(&progress_reporter),
        );

    let registry = t_ctx.ctx.tool_registry.clone();
    let ctrl_c_state = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_state);
    let ctrl_c_notify = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_notify);
    let vt_cfg = t_ctx.ctx.vt_cfg;

    let mut execution_futures = Vec::with_capacity(validated_call_count);
    for (tool_call, prepared, args) in validated_calls {
        let registry = registry.clone();
        let ctrl_c_state = std::sync::Arc::clone(&ctrl_c_state);
        let ctrl_c_notify = std::sync::Arc::clone(&ctrl_c_notify);
        let reporter = progress_reporter.clone();
        let name = prepared.canonical_name;
        let call_id = tool_call.id.clone();

        let fut = async move {
            let start_time = std::time::Instant::now();
            let max_retries = resolve_max_tool_retries(&name, vt_cfg);
            let status =
                crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout_ref_prevalidated(
                    &registry,
                    &name,
                    &args,
                    &ctrl_c_state,
                    &ctrl_c_notify,
                    Some(&reporter),
                    max_retries,
                )
                .await;
            (call_id, name, args, status, start_time)
        };
        execution_futures.push(fut);
    }

    let results = futures::future::join_all(execution_futures).await;

    // 3. Sequential Result Handling
    for (call_id, name, args, status, start_time) in results {
        let outcome =
            crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(status);

        // Track repetition
        update_repetition_tracker(t_ctx.repeated_tool_attempts, &outcome, &name, &args);

        // Handle result
        crate::agent::runloop::unified::turn::tool_outcomes::execution_result::handle_tool_execution_result(
            t_ctx,
            call_id,
            &name,
            &args,
            &outcome,
            start_time,
        ).await?;
    }

    Ok(None)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_and_handle_tool_call<'a, 'b>(
    ctx: &'b mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &'b mut super::helpers::LoopTracker,
    turn_modified_files: &'b mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &str,
    args_val: serde_json::Value,
    batch_progress_reporter: Option<&'b ProgressReporter>,
) -> futures::future::BoxFuture<'b, Result<()>> {
    let tool_name_owned = tool_name.to_string();

    Box::pin(async move {
        execute_and_handle_tool_call_inner(
            ctx,
            repeated_tool_attempts,
            turn_modified_files,
            tool_call_id,
            &tool_name_owned,
            args_val,
            batch_progress_reporter,
        )
        .await
    })
}

async fn execute_and_handle_tool_call_inner<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &mut super::helpers::LoopTracker,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &str,
    args_val: serde_json::Value,
    _batch_progress_reporter: Option<&ProgressReporter>,
) -> Result<()> {
    // Show pre-execution indicator for file modification operations
    if crate::agent::runloop::unified::tool_summary::is_file_modification_tool(tool_name, &args_val)
    {
        crate::agent::runloop::unified::tool_summary::render_file_operation_indicator(
            ctx.renderer,
            tool_name,
            &args_val,
        )?;
    }
    let tool_execution_start = std::time::Instant::now();
    let pipeline_outcome = {
        let ctrl_c_state = ctx.ctrl_c_state;
        let ctrl_c_notify = ctx.ctrl_c_notify;
        let default_placeholder = ctx.default_placeholder.clone();
        let lifecycle_hooks = ctx.lifecycle_hooks;
        let vt_cfg = ctx.vt_cfg;
        let turn_index = ctx.working_history.len();
        let mut run_loop_ctx = ctx.as_run_loop_context();
        run_tool_call_with_args(
            &mut run_loop_ctx,
            tool_call_id.clone(),
            tool_name,
            &args_val,
            ctrl_c_state,
            ctrl_c_notify,
            default_placeholder,
            lifecycle_hooks,
            true,
            vt_cfg,
            turn_index,
            true,
        )
        .await?
    };

    update_repetition_tracker(
        repeated_tool_attempts,
        &pipeline_outcome,
        tool_name,
        &args_val,
    );

    let mut t_ctx = ToolOutcomeContext {
        ctx,
        repeated_tool_attempts,
        turn_modified_files,
    };

    let handle_result = handle_tool_execution_result(
        &mut t_ctx,
        tool_call_id,
        tool_name,
        &args_val,
        &pipeline_outcome,
        tool_execution_start,
    )
    .await;

    handle_result?;

    Ok(())
}
