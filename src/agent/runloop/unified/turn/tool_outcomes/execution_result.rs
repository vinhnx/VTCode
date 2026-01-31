//! Tool execution result handling for turn flow.

use anyhow::Result;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, ToolPipelineOutcome, run_tool_call,
};
use crate::agent::runloop::unified::turn::turn_helpers::display_status;

use super::helpers::push_tool_response;

#[allow(clippy::too_many_arguments)]
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Main handler for tool execution results.
///
/// This function coordinates:
/// - Recording metrics (circuit breaker, health tracker, telemetry)
/// - Pushing tool responses to conversation history
/// - Handling pipeline output (printing to UI)
/// - Running post-tool-use hooks
/// - Handling specific logic like "Plan Mode" auto-exit
/// - Dispatching MCP events
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_tool_execution_result<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    tool_start_time: std::time::Instant,
) -> Result<()> {
    // 1. Record metrics and handle outcome specific logic
    match &pipeline_outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            record_tool_success(ctx, tool_name, tool_start_time);

            let content_for_model = serialize_output(output);
            push_tool_response(ctx.working_history, tool_call_id, content_for_model);

            // Handle UI output and file modifications
            let vt_cfg = ctx.vt_cfg;
            let (_any_write, mod_files, _last_stdout) = handle_pipeline_output_from_turn_ctx(
                &mut ctx.as_turn_loop_context(),
                tool_name,
                args_val,
                pipeline_outcome,
                vt_cfg,
                traj,
            )
            .await?;

            for f in mod_files {
                turn_modified_files.insert(f);
            }

            // Run post-tool hooks
            if let Some(hooks) = ctx.lifecycle_hooks {
                match hooks
                    .run_post_tool_use(tool_name, Some(args_val), output)
                    .await
                {
                    Ok(outcome) => {
                        crate::agent::runloop::unified::turn::utils::render_hook_messages(
                            ctx.renderer,
                            &outcome.messages,
                        )?;
                        for context in outcome.additional_context {
                            if !context.trim().is_empty() {
                                ctx.working_history.push(uni::Message::system(context));
                            }
                        }
                    }
                    Err(err) => {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to run post-tool hooks: {}", err),
                        )?;
                    }
                }
            }
        }
        ToolExecutionStatus::Failure { error } => {
            let error_str = error.to_string();
            let is_plan_mode_denial = error_str.contains("tool denied by plan mode");

            if !is_plan_mode_denial {
                let is_argument_error = check_is_argument_error(&error_str);
                record_tool_failure(ctx, tool_name, tool_start_time, is_argument_error);
            } else {
                tracing::debug!(
                    tool = %tool_name,
                    "Plan mode denial - not recording as circuit breaker failure"
                );
            }

            let error_msg = format!("Tool '{}' execution failed: {}", tool_name, error);
            tracing::debug!(tool = %tool_name, error = %error, "Tool execution failed");

            // Push error to history
            let error_content = serde_json::json!({"error": error_msg});
            push_tool_response(
                ctx.working_history,
                tool_call_id,
                error_content.to_string(),
            );

            // Handle auto-exit from Plan Mode if applicable
            if is_plan_mode_denial && ctx.session_stats.is_plan_mode() {
                handle_plan_mode_auto_exit(ctx, turn_modified_files, traj, tool_start_time).await?;
            }
        }
        ToolExecutionStatus::Timeout { error } => {
            record_tool_failure(ctx, tool_name, tool_start_time, false);

            let error_msg = format!("Tool '{}' timed out: {}", tool_name, error.message);
            tracing::debug!(tool = %tool_name, error = %error.message, "Tool timed out");

            let error_content = serde_json::json!({"error": error_msg});
            push_tool_response(
                ctx.working_history,
                tool_call_id,
                error_content.to_string(),
            );
        }
        ToolExecutionStatus::Cancelled => {
            let error_msg = format!("Tool '{}' execution cancelled", tool_name);
            ctx.renderer.line(MessageStyle::Info, &error_msg)?;

            let error_content = serde_json::json!({"error": error_msg});
            push_tool_response(
                ctx.working_history,
                tool_call_id,
                error_content.to_string(),
            );
        }
        ToolExecutionStatus::Progress(_) => {}
    }

    // 2. Record MCP specific events
    if tool_name.starts_with("mcp_") {
        record_mcp_tool_event(ctx, tool_name, &pipeline_outcome.status);
    }

    Ok(())
}

// --- Helpers ---

fn record_tool_success(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    start_time: std::time::Instant,
) {
    let duration = start_time.elapsed();
    ctx.circuit_breaker.record_success_for_tool(tool_name);
    ctx.tool_health_tracker
        .record_execution(tool_name, true, duration);
    ctx.telemetry.record_tool_usage(tool_name, true);
}

fn record_tool_failure(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    start_time: std::time::Instant,
    is_argument_error: bool,
) {
    let duration = start_time.elapsed();
    ctx.circuit_breaker
        .record_failure_for_tool(tool_name, is_argument_error);
    ctx.tool_health_tracker
        .record_execution(tool_name, false, duration);
    ctx.telemetry.record_tool_usage(tool_name, false);
}

fn check_is_argument_error(error_str: &str) -> bool {
    error_str.contains("Missing required")
        || error_str.contains("Invalid arguments")
        || error_str.contains("required path parameter")
        || error_str.contains("expected ")
        || error_str.contains("Expected:")
}

fn serialize_output(output: &serde_json::Value) -> String {
    if let Some(s) = output.as_str() {
        s.to_string()
    } else {
        serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())
    }
}

async fn handle_plan_mode_auto_exit<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    trigger_start_time: std::time::Instant,
) -> Result<()> {
    if *ctx.auto_exit_plan_mode_attempted {
        display_status(
            ctx.renderer,
            "Plan Mode still active. Call `exit_plan_mode` to review the plan or refine the plan before retrying.",
        )?;
        return Ok(());
    }
    *ctx.auto_exit_plan_mode_attempted = true;

    let exit_args = serde_json::json!({
        "reason": "auto_trigger_on_plan_denial"
    });
    let exit_args_json = serde_json::to_string(&exit_args).unwrap_or_else(|_| "{}".to_string());
    
    // Generate a unique ID for the injected call
    let exit_call_id = format!(
        "call_auto_exit_plan_mode_{}",
        trigger_start_time.elapsed().as_millis()
    );
    let exit_call = uni::ToolCall::function(
        exit_call_id.clone(),
        tool_names::EXIT_PLAN_MODE.to_string(),
        exit_args_json.clone(),
    );

    let exit_start = std::time::Instant::now();
    
    // Construct temporary run loop context for the recursive call
    let mut run_loop_ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: ctx.renderer,
        handle: ctx.handle,
        tool_registry: ctx.tool_registry,
        tools: ctx.tools,
        tool_result_cache: ctx.tool_result_cache,
        tool_permission_cache: ctx.tool_permission_cache,
        decision_ledger: ctx.decision_ledger,
        session_stats: ctx.session_stats,
        mcp_panel_state: ctx.mcp_panel_state,
        approval_recorder: ctx.approval_recorder,
        session: ctx.session,
        traj,
        harness_state: ctx.harness_state,
        harness_emitter: ctx.harness_emitter,
    };

    let exit_outcome = run_tool_call(
        &mut run_loop_ctx,
        &exit_call,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        ctx.default_placeholder.clone(),
        ctx.lifecycle_hooks,
        true,
        ctx.vt_cfg,
        0,
    )
    .await;

    // Handle the outcome of the exit call
    if let Ok(exit_pipeline_outcome) = exit_outcome {
        match &exit_pipeline_outcome.status {
            ToolExecutionStatus::Success { output, .. } => {
                record_tool_success(ctx, tool_names::EXIT_PLAN_MODE, exit_start);

                let content_for_model = serialize_output(output);
                push_tool_response(
                    ctx.working_history,
                    exit_call_id,
                    content_for_model,
                );

                let vt_cfg = ctx.vt_cfg;
                let (_any_write, mod_files, _last_stdout) = handle_pipeline_output_from_turn_ctx(
                    &mut ctx.as_turn_loop_context(),
                    tool_names::EXIT_PLAN_MODE,
                    &exit_args,
                    &exit_pipeline_outcome,
                    vt_cfg,
                    traj,
                )
                .await?;
                for f in mod_files {
                    turn_modified_files.insert(f);
                }
            }
            ToolExecutionStatus::Failure { error } => {
                record_tool_failure(ctx, tool_names::EXIT_PLAN_MODE, exit_start, false);

                let err_msg = format!(
                    "Tool '{}' execution failed: {}",
                    tool_names::EXIT_PLAN_MODE,
                    error
                );
                tracing::debug!(
                    tool = tool_names::EXIT_PLAN_MODE,
                    error = %error,
                    "exit_plan_mode failed"
                );
                let error_content = serde_json::json!({"error": err_msg});
                push_tool_response(
                    ctx.working_history,
                    exit_call_id,
                    error_content.to_string(),
                );
            }
            ToolExecutionStatus::Timeout { error } => {
                record_tool_failure(ctx, tool_names::EXIT_PLAN_MODE, exit_start, false);
                let err_msg = format!(
                    "Tool '{}' timed out: {}",
                    tool_names::EXIT_PLAN_MODE,
                    error.message
                );
                tracing::debug!(
                    tool = tool_names::EXIT_PLAN_MODE,
                    error = %error.message,
                    "exit_plan_mode timed out"
                );
                let error_content = serde_json::json!({"error": err_msg});
                push_tool_response(
                    ctx.working_history,
                    exit_call_id,
                    error_content.to_string(),
                );
            }
            ToolExecutionStatus::Cancelled | ToolExecutionStatus::Progress(_) => {}
        }
    }
    
    Ok(())
}

fn record_mcp_tool_event(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    status: &ToolExecutionStatus,
) {
    if matches!(status, ToolExecutionStatus::Progress(_)) {
        return;
    }

    let data_preview = match status {
        ToolExecutionStatus::Success { output, .. } => Some(serialize_output(output)),
        ToolExecutionStatus::Failure { error } => {
            Some(serde_json::json!({"error": error.to_string()}).to_string())
        }
        ToolExecutionStatus::Timeout { error } => {
            Some(serde_json::json!({"error": error.message}).to_string())
        }
        ToolExecutionStatus::Cancelled => {
            Some(serde_json::json!({"error": "Cancelled"}).to_string())
        }
        ToolExecutionStatus::Progress(_) => None,
    };

    let mut mcp_event = mcp_events::McpEvent::new(
        "mcp".to_string(),
        tool_name.to_string(),
        data_preview,
    );

    match status {
        ToolExecutionStatus::Success { .. } => {
            mcp_event.success(None);
        }
        ToolExecutionStatus::Failure { error } => {
            mcp_event.failure(Some(error.to_string()));
        }
        ToolExecutionStatus::Timeout { error } => {
            mcp_event.failure(Some(error.message.clone()));
        }
        ToolExecutionStatus::Cancelled => {
            mcp_event.failure(Some("Cancelled".to_string()));
        }
        ToolExecutionStatus::Progress(_) => {}
    }

    ctx.mcp_panel_state.add_event(mcp_event);
}
