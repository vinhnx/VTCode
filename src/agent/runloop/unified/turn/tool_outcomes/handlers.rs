//! Tool outcome handling helpers for turn execution.

use anyhow::Result;
use std::sync::Arc;
use vtcode_core::llm::provider as uni;

use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_call_safety::SafetyError;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolPipelineOutcome, execute_tool_with_timeout_ref,
};
use crate::agent::runloop::unified::tool_routing::{
    ensure_tool_permission, prompt_session_limit_increase,
};
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use super::execution_result::handle_tool_execution_result;
use super::helpers::{push_tool_response, resolve_max_tool_retries, update_repetition_tracker};
use crate::agent::runloop::unified::tool_routing::ToolPermissionFlow;

/// Result of a tool call validation phase.
pub(crate) enum ValidationResult {
    /// Proceed with execution
    Proceed,
    /// Tool was blocked or handled internally (e.g. error pushed to history), skip execution but continue turn
    Blocked,
    /// Stop turn/loop with a specific outcome (e.g. Exit or Cancel)
    Outcome(TurnHandlerOutcome),
}

/// Consolidated state for tool outcomes to reduce signature bloat and ensure DRY across handlers.
pub struct ToolOutcomeContext<'a, 'b> {
    pub ctx: &'b mut TurnProcessingContext<'a>,
    pub repeated_tool_attempts: &'b mut std::collections::HashMap<String, usize>,
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
    match validate_tool_call(t_ctx.ctx, &tool_call_id, tool_name, &args_val).await? {
        ValidationResult::Outcome(outcome) => return Ok(Some(outcome)),
        ValidationResult::Blocked => return Ok(None),
        ValidationResult::Proceed => {}
    }

    // 3. Execute and Handle Result
    execute_and_handle_tool_call(
        t_ctx.ctx,
        t_ctx.repeated_tool_attempts,
        t_ctx.turn_modified_files,
        tool_call_id,
        tool_name,
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
    // Phase 4 Check: Per-tool Circuit Breaker
    if !ctx.circuit_breaker.allow_request_for_tool(tool_name) {
        let error_msg = format!(
            "Tool '{}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).",
            tool_name
        );
        tracing::warn!(tool = %tool_name, "Circuit breaker open, tool disabled");
        push_tool_response(
            ctx.working_history,
            tool_call_id.to_string(),
            serde_json::json!({"error": error_msg}).to_string(),
        );
        return Ok(ValidationResult::Blocked);
    }

    // Phase 4 Check: Adaptive Rate Limiter
    match ctx.rate_limiter.try_acquire(tool_name) {
        Ok(_) => {}
        Err(wait_time) => {
            if wait_time.as_secs_f64() > 0.0 {
                tokio::time::sleep(wait_time).await;
            }
        }
    }

    // Phase 4 Check: Adaptive Loop Detection
    if let Some(warning) = ctx
        .autonomous_executor
        .record_tool_call(tool_name, args_val)
    {
        let should_block = {
            if let Ok(detector) = ctx.autonomous_executor.loop_detector().read() {
                detector.is_hard_limit_exceeded(tool_name)
            } else {
                false
            }
        };

        if should_block {
            let error_msg = format!(
                "Tool '{}' is blocked due to excessive repetition (Loop Detected).",
                tool_name
            );
            tracing::warn!(tool = %tool_name, "Loop detector blocked tool");
            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                serde_json::json!({"error": error_msg}).to_string(),
            );
            return Ok(ValidationResult::Blocked);
        } else {
            tracing::warn!(tool = %tool_name, warning = %warning, "Loop detector warning");
        }
    }

    // Safety Validation Loop
    loop {
        let validation_result = {
            let mut validator = ctx.safety_validator.write().await;
            validator.validate_call(tool_name)
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
                            serde_json::json!({"error": "Session tool limit reached and not increased by user"})
                                .to_string(),
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
                    serde_json::json!({"error": format!("Safety validation failed: {}", err)})
                        .to_string(),
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
        },
        tool_name,
        Some(args_val),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => Ok(ValidationResult::Proceed),
        Ok(ToolPermissionFlow::Denied) => {
            let denial = ToolExecutionError::new(
                tool_name.to_string(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{}' execution denied by policy", tool_name),
            )
            .to_json_value();

            push_tool_response(
                ctx.working_history,
                tool_call_id.to_string(),
                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
            );
            Ok(ValidationResult::Blocked)
        }
        Ok(ToolPermissionFlow::Exit) => Ok(ValidationResult::Outcome(
            TurnHandlerOutcome::Break(TurnLoopResult::Exit),
        )),
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
        let args_val: serde_json::Value =
            serde_json::from_str(&func.arguments).unwrap_or(serde_json::json!({}));

        match validate_tool_call(t_ctx.ctx, &tool_call.id, tool_name, &args_val).await? {
            ValidationResult::Outcome(outcome) => return Ok(Some(outcome)),
            ValidationResult::Blocked => continue,
            ValidationResult::Proceed => {
                validated_calls.push((tool_call, tool_name.to_string(), args_val));
            }
        }
    }

    if validated_calls.is_empty() {
        return Ok(None);
    }

    // 2. Parallel Execution
    let progress_reporter = ProgressReporter::new();
    let _spinner = crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
        t_ctx.ctx.handle,
        t_ctx.ctx.input_status_state.left.clone(),
        t_ctx.ctx.input_status_state.right.clone(),
        format!("Executing {} tools...", validated_calls.len()),
        Some(&progress_reporter),
    );

    let registry = t_ctx.ctx.tool_registry.clone();
    let ctrl_c_state = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_state);
    let ctrl_c_notify = std::sync::Arc::clone(t_ctx.ctx.ctrl_c_notify);
    let vt_cfg = t_ctx.ctx.vt_cfg;

    let mut execution_futures = Vec::new();
    for (tool_call, tool_name, args_val) in &validated_calls {
        let registry = registry.clone();
        let ctrl_c_state = std::sync::Arc::clone(&ctrl_c_state);
        let ctrl_c_notify = std::sync::Arc::clone(&ctrl_c_notify);
        let reporter = progress_reporter.clone();
        let name = tool_name.clone();
        let args = args_val.clone();
        let call_id = tool_call.id.clone();
        
        let fut = async move {
            let start_time = std::time::Instant::now();
            let max_retries = resolve_max_tool_retries(&name, vt_cfg);
            let status = crate::agent::runloop::unified::tool_pipeline::execute_tool_with_timeout_ref(
                &registry,
                &name,
                &args,
                &ctrl_c_state,
                &ctrl_c_notify,
                Some(&reporter),
                max_retries,
            ).await;
            (call_id, name, args, status, start_time)
        };
        execution_futures.push(fut);
    }

    let results = futures::future::join_all(execution_futures).await;

    // 3. Sequential Result Handling
    for (call_id, name, args, status, start_time) in results {
        let outcome = crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(status);
        
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
    repeated_tool_attempts: &'b mut std::collections::HashMap<String, usize>,
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
        ).await
    })
}

async fn execute_and_handle_tool_call_inner<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    tool_call_id: String,
    tool_name: &str,
    args_val: serde_json::Value,
    batch_progress_reporter: Option<&ProgressReporter>,
) -> Result<()> {
    // Show pre-execution indicator for file modification operations
    if crate::agent::runloop::unified::tool_summary::is_file_modification_tool(
        tool_name, &args_val,
    ) {
        crate::agent::runloop::unified::tool_summary::render_file_operation_indicator(
            ctx.renderer,
            tool_name,
            &args_val,
        )?;
    }

    // Caching Logic (Read-Only Tools)
    let is_read_only = tool_name == "read_file"
        || tool_name == "list_dir"
        || tool_name == "search_files"
        || tool_name == "codebase_search"
        || tool_name == "grep_search";

    use vtcode_core::tools::result_cache::ToolCacheKey;
    
    let cache_key = if is_read_only {
        Some(ToolCacheKey::from_json(tool_name, &args_val, ""))
    } else {
        None
    };

    // Check cache
    if let Some(ref key) = cache_key {
        let mut tool_cache_guard = ctx.tool_result_cache.write().await;
        if let Some(cached_output) = tool_cache_guard.get(key) {
            #[cfg(debug_assertions)]
            tracing::debug!("Cache hit for tool: {}", tool_name);

            let cached_json: serde_json::Value =
                serde_json::from_str(&cached_output).unwrap_or(serde_json::json!({}));
            
            let outcome = ToolPipelineOutcome::from_status(
                crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success {
                    output: cached_json,
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                }
            );

            let mut t_ctx_local = ToolOutcomeContext {
                ctx: &mut *ctx,
                repeated_tool_attempts: &mut *repeated_tool_attempts,
                turn_modified_files: &mut *turn_modified_files,
            };

            handle_tool_execution_result(
                &mut t_ctx_local,
                tool_call_id,
                tool_name,
                &args_val,
                &outcome,
                std::time::Instant::now(),
            )
            .await?;

            return Ok(());
        }
    }

    let progress_reporter = if let Some(r) = batch_progress_reporter {
        r.clone()
    } else {
        ProgressReporter::new()
    };

    let _spinner = if batch_progress_reporter.is_none() {
        Some(crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
            ctx.handle,
            ctx.input_status_state.left.clone(),
            ctx.input_status_state.right.clone(),
            format!("Executing {}...", tool_name),
            Some(&progress_reporter),
        ))
    } else {
        None
    };

    let progress_reporter_clone = progress_reporter.clone();
    let handle_clone = ctx.handle.clone();
    let is_pty_command = tool_name == vtcode_core::config::constants::tools::RUN_PTY_CMD
        || tool_name == vtcode_core::config::constants::tools::UNIFIED_EXEC
        || tool_name == vtcode_core::config::constants::tools::SEND_PTY_INPUT;

    ctx.tool_registry
        .set_progress_callback(Arc::new(move |name: &str, output: &str| {
            let reporter = progress_reporter_clone.clone();
            let output_owned = output.to_string();
            let handle = handle_clone.clone();
            let is_pty = is_pty_command || name == "run_pty_cmd" || name == "unified_exec";

            tokio::spawn(async move {
                if is_pty && !output_owned.is_empty() {
                    for line in output_owned.lines() {
                        let clean_line = vtcode_core::utils::ansi_parser::strip_ansi(line);
                        let trimmed = clean_line.trim();
                        if !trimmed.is_empty() {
                            handle.append_line(
                                vtcode_core::ui::tui::InlineMessageKind::Pty,
                                vec![vtcode_core::ui::tui::InlineSegment {
                                    text: trimmed.to_string(),
                                    style: std::sync::Arc::new(
                                        vtcode_core::ui::tui::InlineTextStyle::default(),
                                    ),
                                }],
                            );
                        }
                    }
                    if let Some(last_line) = output_owned.lines().last() {
                        let clean_line =
                            vtcode_core::utils::ansi_parser::strip_ansi(last_line);
                        let trimmed = clean_line.trim();
                        if !trimmed.is_empty() {
                            reporter.set_message(trimmed.to_string()).await;
                        }
                    }
                } else if let Some(last_line) = output_owned.lines().last() {
                    let clean_line =
                        vtcode_core::utils::ansi_parser::strip_ansi(last_line);
                    let trimmed = clean_line.trim();
                    if !trimmed.is_empty() {
                        reporter.set_message(trimmed.to_string()).await;
                    }
                }
            });
        }));

     let tool_execution_start = std::time::Instant::now();
    let max_tool_retries = resolve_max_tool_retries(tool_name, ctx.vt_cfg);
    let tool_result = execute_tool_with_timeout_ref(
        ctx.tool_registry,
        tool_name,
        &args_val,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        Some(&progress_reporter),
        max_tool_retries,
    )
    .await;

    ctx.tool_registry.clear_progress_callback();

    let pipeline_outcome = ToolPipelineOutcome::from_status(tool_result);

    if let Some(ref key) = cache_key
        && let crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success { ref output, .. } = pipeline_outcome.status {
             let output_json = serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string());
             let mut cache: tokio::sync::RwLockWriteGuard<'_, vtcode_core::tools::ToolResultCache> = ctx.tool_result_cache.write().await;
             cache.insert_arc(key.clone(), Arc::new(output_json));
    }

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

    handle_tool_execution_result(
        &mut t_ctx,
        tool_call_id,
        tool_name,
        &args_val,
        &pipeline_outcome,
        tool_execution_start,
    )
    .await?;

    Ok(())
}
