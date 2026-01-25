//! Tool call dispatching for the turn loop.

use anyhow::Result;
use futures::future::join_all;
use std::sync::Arc;

use vtcode_core::llm::provider as uni;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::parallel_executor::ParallelExecutionPlanner;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::tool_call_safety::SafetyError;
use crate::agent::runloop::unified::tool_pipeline::{
    execute_tool_with_timeout_ref, ToolPipelineOutcome,
};
use crate::agent::runloop::unified::tool_routing::{
    ensure_tool_permission, prompt_session_limit_increase, ToolPermissionFlow,
};
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use super::execution_result::handle_tool_execution_result;
use super::helpers::{push_tool_response, resolve_max_tool_retries, signature_key_for};

pub(crate) async fn handle_tool_calls(
    ctx: &mut TurnProcessingContext<'_>,
    tool_calls: &[uni::ToolCall],
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<Option<TurnHandlerOutcome>> {
    if tool_calls.is_empty() {
        return Ok(None);
    }

    // HP-4: Use ParallelExecutionPlanner to group independent tool calls
    let planner = ParallelExecutionPlanner::new();
    let mut planner_calls = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        let name = tc.function.as_ref().map(|f| f.name.as_str()).unwrap_or("");
        let args = Arc::new(tc.parsed_arguments().unwrap_or(serde_json::json!({})));
        planner_calls.push((name.to_string(), args, tc.id.clone()));
    }

    let groups = planner.plan(&planner_calls);

    for group in groups {
        if group.len() > 1 && ctx.full_auto {
            // HP-5: Implement true parallel execution for non-conflicting groups in full-auto mode
            let mut group_tool_calls = Vec::with_capacity(group.len());
            for (_, _, call_id) in &group.tool_calls {
                if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id) {
                    group_tool_calls.push(tc);
                }
            }

            // Check if all tools in group are safe and approved
            let mut can_run_parallel = true;
            let mut execution_items = Vec::with_capacity(group_tool_calls.len());

            for tc in &group_tool_calls {
                let func = match tc.function.as_ref() {
                    Some(f) => f,
                    None => {
                        can_run_parallel = false;
                        break;
                    }
                };
                let tool_name = &func.name;
                let args_val = tc
                    .parsed_arguments()
                    .unwrap_or_else(|_| serde_json::json!({}));

                // Quick safety check
                {
                    let mut validator = ctx.safety_validator.write().await;
                    if validator.validate_call(tool_name).is_err() {
                        can_run_parallel = false;
                        break;
                    }
                }

                let is_allowed = matches!(
                    ctx.tool_registry.get_tool_policy(tool_name).await,
                    ToolPolicy::Allow
                );

                if !is_allowed {
                    can_run_parallel = false;
                    break;
                }

                execution_items.push((tc.id.clone(), tool_name.clone(), args_val));
            }

            if can_run_parallel && !execution_items.is_empty() {
                for (_, name, args) in &execution_items {
                    if crate::agent::runloop::unified::tool_summary::is_file_modification_tool(
                        name, args,
                    ) {
                        crate::agent::runloop::unified::tool_summary::render_file_operation_indicator(
                            ctx.renderer,
                            name,
                            args,
                        )?;
                    }
                }

                let tool_names: Vec<_> = execution_items
                    .iter()
                    .map(|(_, name, _)| name.as_str())
                    .collect();
                let batch_msg = format!("Executing batch: [{}]", tool_names.join(", "));

                let progress_reporter = ProgressReporter::new();
                let _spinner = crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
                    ctx.handle,
                    ctx.input_status_state.left.clone(),
                    ctx.input_status_state.right.clone(),
                    batch_msg,
                    Some(&progress_reporter),
                );

                // Execute all in parallel
                let registry = ctx.tool_registry.clone();
                let ctrl_c_state = Arc::clone(ctx.ctrl_c_state);
                let ctrl_c_notify = Arc::clone(ctx.ctrl_c_notify);

                let max_tool_retries = resolve_max_tool_retries(ctx.vt_cfg);
                let futures: Vec<_> = execution_items
                    .iter()
                    .map(|(call_id, name, args)| {
                        let registry = registry.clone();
                        let ctrl_c_state = Arc::clone(&ctrl_c_state);
                        let ctrl_c_notify = Arc::clone(&ctrl_c_notify);
                        let name = name.clone();
                        let args = args.clone();
                        let reporter = progress_reporter.clone();
                        let max_tool_retries = max_tool_retries;

                        async move {
                            let start_time = std::time::Instant::now();
                            let result = execute_tool_with_timeout_ref(
                                &registry,
                                &name,
                                &args,
                                &ctrl_c_state,
                                &ctrl_c_notify,
                                Some(&reporter),
                                max_tool_retries,
                            )
                            .await;
                            (call_id.clone(), name, args, result, start_time)
                        }
                    })
                    .collect();

                let results = join_all(futures).await;

                for (call_id, name, args, status, start_time) in results {
                    let outcome = ToolPipelineOutcome::from_status(status);

                    let signature_key = signature_key_for(&name, &args);
                    let current_count = repeated_tool_attempts.entry(signature_key).or_insert(0);
                    *current_count += 1;

                    handle_tool_execution_result(
                        &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext {
                            renderer: ctx.renderer,
                            handle: ctx.handle,
                            session: ctx.session,
                            session_stats: ctx.session_stats,
                            auto_exit_plan_mode_attempted: ctx.auto_exit_plan_mode_attempted,
                            mcp_panel_state: ctx.mcp_panel_state,
                            tool_result_cache: ctx.tool_result_cache,
                            approval_recorder: ctx.approval_recorder,
                            decision_ledger: ctx.decision_ledger,
                            tool_registry: ctx.tool_registry,
                            tools: ctx.tools,
                            cached_tools: ctx.cached_tools,
                            ctrl_c_state: ctx.ctrl_c_state,
                            ctrl_c_notify: ctx.ctrl_c_notify,
                            context_manager: ctx.context_manager,
                            last_forced_redraw: ctx.last_forced_redraw,
                            input_status_state: ctx.input_status_state,
                            lifecycle_hooks: ctx.lifecycle_hooks,
                            default_placeholder: ctx.default_placeholder,
                            tool_permission_cache: ctx.tool_permission_cache,
                            safety_validator: ctx.safety_validator,
                            circuit_breaker: ctx.circuit_breaker,
                            tool_health_tracker: ctx.tool_health_tracker,
                            rate_limiter: ctx.rate_limiter,
                            telemetry: ctx.telemetry,
                            autonomous_executor: ctx.autonomous_executor,
                            error_recovery: ctx.error_recovery,
                            harness_state: ctx.harness_state,
                            harness_emitter: ctx.harness_emitter,
                        },
                        call_id,
                        &name,
                        &args,
                        &outcome,
                        ctx.working_history,
                        turn_modified_files,
                        ctx.vt_cfg,
                        traj,
                        start_time,
                    )
                    .await?;
                }
                continue; // Move to next group
            }

            for tc in group_tool_calls {
                if let Some(outcome) =
                    handle_tool_call(ctx, tc, repeated_tool_attempts, turn_modified_files, traj)
                        .await?
                {
                    return Ok(Some(outcome));
                }
            }
        } else {
            let call_id = &group.tool_calls[0].2;
            if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id)
                && let Some(outcome) =
                    handle_tool_call(ctx, tc, repeated_tool_attempts, turn_modified_files, traj)
                        .await?
            {
                return Ok(Some(outcome));
            }
        }
    }

    Ok(None)
}

pub(crate) async fn handle_tool_call(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call: &uni::ToolCall,
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<Option<TurnHandlerOutcome>> {
    let function = tool_call
        .function
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Tool call has no function definition"))?;
    let tool_name = &function.name;
    let args_val = tool_call
        .parsed_arguments()
        .unwrap_or_else(|_| serde_json::json!({}));

    // HP-4: Validate tool call safety before execution

    // Phase 4 Check: Per-tool Circuit Breaker
    if !ctx.circuit_breaker.allow_request_for_tool(tool_name) {
        let error_msg = format!(
            "Tool '{}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).",
            tool_name
        );
        // Log to tracing but don't show error in TUI - just inform the LLM via history
        tracing::warn!(tool = %tool_name, "Circuit breaker open, tool disabled");
        push_tool_response(
            ctx.working_history,
            tool_call.id.clone(),
            serde_json::json!({"error": error_msg}).to_string(),
            tool_name,
        );
        return Ok(None);
    }

    // Phase 4 Check: Adaptive Rate Limiter
    // We prioritize keeping the UI responsive, so we'll wait if needed but with a timeout
    // Using a simple blocking check for now for simplicity in the async context
    match ctx.rate_limiter.try_acquire(tool_name) {
        Ok(_) => {} // Acquired
        Err(wait_time) => {
            // Rate limit exceeded, wait and proceed (backpressure)
            if wait_time.as_secs_f64() > 0.0 {
                tokio::time::sleep(wait_time).await;
            }
        }
    }

    // Phase 4 Check: Adaptive Loop Detection
    if let Some(warning) = ctx
        .autonomous_executor
        .record_tool_call(tool_name, &args_val)
    {
        // Check if we should block due to hard limit
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
                tool_call.id.clone(),
                serde_json::json!({"error": error_msg}).to_string(),
                tool_name,
            );
            return Ok(None);
        } else {
            // Log warning but proceed
            tracing::warn!(tool = %tool_name, warning = %warning, "Loop detector warning");
            // Optionally inject warning into history? AgentRunner didn't seems to do it explicitly here,
            // but providing feedback to the model is good.
            // However, avoid spamming content.
        }
    }

    loop {
        let validation_result = {
            let mut validator = ctx.safety_validator.write().await;
            validator.validate_call(tool_name)
        };

        match validation_result {
            Ok(_) => break, // Validation passed
            Err(SafetyError::SessionLimitReached { max }) => {
                // Prompt user to increase limit
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
                        continue; // Retry validation
                    }
                    _ => {
                        // Denied or cancelled
                        push_tool_response(
                            ctx.working_history,
                            tool_call.id.clone(),
                            serde_json::json!({"error": "Session tool limit reached and not increased by user"})
                                .to_string(),
                            tool_name,
                        );
                        return Ok(None);
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
                    tool_call.id.clone(),
                    serde_json::json!({"error": format!("Safety validation failed: {}", err)})
                        .to_string(),
                    tool_name,
                );
                return Ok(None);
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
        },
        tool_name,
        Some(&args_val),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => {
            let signature_key = signature_key_for(tool_name, &args_val);
            let current_count = repeated_tool_attempts.entry(signature_key).or_insert(0);
            *current_count += 1;

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

            let progress_reporter = ProgressReporter::new();
            let _spinner =
                crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
                    ctx.handle,
                    ctx.input_status_state.left.clone(),
                    ctx.input_status_state.right.clone(),
                    format!("Executing {}...", tool_name),
                    Some(&progress_reporter),
                );

            let progress_reporter_clone = progress_reporter.clone();
            ctx.tool_registry
                .set_progress_callback(Arc::new(move |_name, output| {
                    let reporter = progress_reporter_clone.clone();
                    let output_owned = output.to_string();
                    tokio::spawn(async move {
                        if let Some(last_line) = output_owned.lines().last() {
                            let clean_line = vtcode_core::utils::ansi_parser::strip_ansi(last_line);
                            let trimmed = clean_line.trim();
                            if !trimmed.is_empty() {
                                reporter.set_message(trimmed.to_string()).await;
                            }
                        }
                    });
                }));

            let tool_execution_start = std::time::Instant::now();
            let max_tool_retries = resolve_max_tool_retries(ctx.vt_cfg);
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

            handle_tool_execution_result(
                &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext {
                    renderer: ctx.renderer,
                    handle: ctx.handle,
                    session: ctx.session,
                    session_stats: ctx.session_stats,
                    auto_exit_plan_mode_attempted: ctx.auto_exit_plan_mode_attempted,
                    mcp_panel_state: ctx.mcp_panel_state,
                    tool_result_cache: ctx.tool_result_cache,
                    approval_recorder: ctx.approval_recorder,
                    decision_ledger: ctx.decision_ledger,
                    tool_registry: ctx.tool_registry,
                    tools: ctx.tools,
                    cached_tools: ctx.cached_tools,
                    ctrl_c_state: ctx.ctrl_c_state,
                    ctrl_c_notify: ctx.ctrl_c_notify,
                    context_manager: ctx.context_manager,
                    last_forced_redraw: ctx.last_forced_redraw,
                    input_status_state: ctx.input_status_state,
                    lifecycle_hooks: ctx.lifecycle_hooks,
                    default_placeholder: ctx.default_placeholder,
                    tool_permission_cache: ctx.tool_permission_cache,
                    safety_validator: ctx.safety_validator,
                    circuit_breaker: ctx.circuit_breaker,
                    tool_health_tracker: ctx.tool_health_tracker,
                    rate_limiter: ctx.rate_limiter,
                    telemetry: ctx.telemetry,
                    autonomous_executor: ctx.autonomous_executor,
                    error_recovery: ctx.error_recovery,
                    harness_state: ctx.harness_state,
                    harness_emitter: ctx.harness_emitter,
                },
                tool_call.id.clone(),
                tool_name,
                &args_val,
                &pipeline_outcome,
                ctx.working_history,
                turn_modified_files,
                ctx.vt_cfg,
                traj,
                tool_execution_start,
            )
            .await?;
        }
        Ok(ToolPermissionFlow::Denied) => {
            let denial = ToolExecutionError::new(
                tool_name.clone(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{}' execution denied by policy", tool_name),
            )
            .to_json_value();

            push_tool_response(
                ctx.working_history,
                tool_call.id.clone(),
                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
                tool_name,
            );
        }
        Ok(ToolPermissionFlow::Exit) => {
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
        Ok(ToolPermissionFlow::Interrupted) => {
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
        Err(err) => {
            let err_json = serde_json::json!({
                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
            });
            push_tool_response(
                ctx.working_history,
                tool_call.id.clone(),
                err_json.to_string(),
                tool_name,
            );
        }
    }

    Ok(None)
}
