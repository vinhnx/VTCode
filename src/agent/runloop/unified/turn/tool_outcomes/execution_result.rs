//! Tool execution result handling for turn flow.

use anyhow::Result;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, ToolPipelineOutcome,
};
use crate::agent::runloop::unified::turn::turn_helpers::display_status;

use super::helpers::{check_is_argument_error, push_tool_response, serialize_output};

use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

fn record_tool_execution(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    start_time: std::time::Instant,
    success: bool,
    is_argument_error: bool,
) {
    let duration = start_time.elapsed();
    if success {
        ctx.circuit_breaker.record_success_for_tool(tool_name);
    } else {
        ctx.circuit_breaker
            .record_failure_for_tool(tool_name, is_argument_error);
    }
    ctx.tool_health_tracker
        .record_execution(tool_name, success, duration);
    ctx.telemetry.record_tool_usage(tool_name, success);
}

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
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    tool_start_time: std::time::Instant,
) -> Result<()> {
    // 1. Record metrics and outcome
    let is_success = matches!(pipeline_outcome.status, ToolExecutionStatus::Success { .. });
    let is_argument_error = if let ToolExecutionStatus::Failure { error } = &pipeline_outcome.status {
        check_is_argument_error(&error.to_string())
    } else {
        false
    };
    
    self::record_tool_execution(
        t_ctx.ctx,
        tool_name,
        tool_start_time,
        is_success,
        is_argument_error,
    );

    match &pipeline_outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            handle_success(
                t_ctx,
                tool_call_id,
                tool_name,
                args_val,
                pipeline_outcome,
                output,
            )
            .await?;
        }
        ToolExecutionStatus::Failure { error } => {
            handle_failure(
                t_ctx,
                tool_call_id,
                tool_name,
                error,
                tool_start_time,
            )
            .await?;
        }
        ToolExecutionStatus::Timeout { error } => {
            handle_timeout(t_ctx, tool_call_id, tool_name, error).await?;
        }
        ToolExecutionStatus::Cancelled => {
            handle_cancelled(t_ctx, tool_call_id, tool_name).await?;
        }
        ToolExecutionStatus::Progress(_) => {}
    }

    // 2. Record MCP specific events
    if tool_name.starts_with("mcp_") {
        record_mcp_tool_event(t_ctx, tool_name, &pipeline_outcome.status);
    }

    Ok(())
}

async fn handle_success<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    output: &serde_json::Value,
) -> Result<()> {
    let content_for_model = serialize_output(output);
    push_tool_response(t_ctx.ctx.working_history, tool_call_id, content_for_model);

    // Handle UI output and file modifications
    let vt_cfg = t_ctx.ctx.vt_cfg;
    let (_any_write, mod_files, _last_stdout) = handle_pipeline_output_from_turn_ctx(
        &mut t_ctx.ctx.as_turn_loop_context(),
        tool_name,
        args_val,
        pipeline_outcome,
        vt_cfg,
    )
    .await?;

    for f in mod_files {
        t_ctx.turn_modified_files.insert(f);
    }

    // Run post-tool hooks
    run_post_tool_hooks(t_ctx.ctx, tool_name, args_val, output).await?;

    Ok(())
}

async fn handle_failure<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    error: &anyhow::Error,
    tool_start_time: std::time::Instant,
) -> Result<()> {
    let error_str = error.to_string();
    let is_plan_mode_denial = error_str.contains("tool denied by plan mode");

    let error_msg = format!("Tool '{}' execution failed: {}", tool_name, error);
    tracing::debug!(tool = %tool_name, error = %error, "Tool execution failed");

    // Push error to history
    let error_content = serde_json::json!({"error": error_msg});
    push_tool_response(
        t_ctx.ctx.working_history,
        tool_call_id,
        error_content.to_string(),
    );

    // Handle auto-exit from Plan Mode if applicable
    if is_plan_mode_denial && t_ctx.ctx.session_stats.is_plan_mode() {
        handle_plan_mode_auto_exit(t_ctx, tool_start_time).await?;
    }

    Ok(())
}

async fn handle_timeout(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    error: &vtcode_core::tools::registry::ToolExecutionError,
) -> Result<()> {
    let error_msg = format!("Tool '{}' timed out: {}", tool_name, error.message);
    tracing::debug!(tool = %tool_name, error = %error.message, "Tool timed out");

    let error_content = serde_json::json!({"error": error_msg});
    push_tool_response(
        t_ctx.ctx.working_history,
        tool_call_id,
        error_content.to_string(),
    );
    
    Ok(())
}

async fn handle_cancelled(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
) -> Result<()> {
    let error_msg = format!("Tool '{}' execution cancelled", tool_name);
    t_ctx.ctx.renderer.line(MessageStyle::Info, &error_msg)?;

    let error_content = serde_json::json!({"error": error_msg});
    push_tool_response(
        t_ctx.ctx.working_history,
        tool_call_id,
        error_content.to_string(),
    );
    
    Ok(())
}

async fn run_post_tool_hooks<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> Result<()> {
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
    Ok(())
}




async fn handle_plan_mode_auto_exit<'a, 'b>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, 'b>,
    trigger_start_time: std::time::Instant,
) -> Result<()> {
    if *t_ctx.ctx.auto_exit_plan_mode_attempted {
        display_status(
            t_ctx.ctx.renderer,
            "Plan Mode still active. Call `exit_plan_mode` to review the plan or refine the plan before retrying.",
        )?;
        return Ok(());
    }
    *t_ctx.ctx.auto_exit_plan_mode_attempted = true;

    let exit_args = serde_json::json!({
        "reason": "auto_trigger_on_plan_denial"
    });
    
    // Generate a unique ID for the injected call
    let exit_call_id = format!(
        "call_auto_exit_plan_mode_{}",
        trigger_start_time.elapsed().as_millis()
    );

    // HP-6: Use the unified execute_and_handle_tool_call so that recording and side-effects happen correctly
    super::handlers::execute_and_handle_tool_call(
        t_ctx.ctx,
        t_ctx.repeated_tool_attempts,
        t_ctx.turn_modified_files,
        exit_call_id,
        tool_names::EXIT_PLAN_MODE,
        exit_args,
        None,
    )
    .await?;
    
    Ok(())
}

fn record_mcp_tool_event(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
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

    t_ctx.ctx.mcp_panel_state.add_event(mcp_event);
}
