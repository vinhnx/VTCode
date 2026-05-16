//! Tool execution result handling for turn flow.
//! Agent Legibility:
//! - Entrypoint: this root coordinates tool success and failure shaping through `record_tool_execution`, `emit_turn_metric_log`, and the tool-outcome helpers it calls.
//! - Common changes:
//!   - Structured error presentation and fallback content route through sibling modules in `tool_outcomes/`.
//!   - Auto-mode probe handling and failure-path response shaping now live in `execution_result/` support modules.
//!   - Success-path notification and turn-result shaping still flow through this root and remain queued for further decomposition.
//! - Constraints: TD-005 is active here; preserve the root as a coordinator and prefer new responsibility-named support modules over adding more inline branches.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode inline_events::tests`

mod auto_mode_probe;
mod failure_path;

use anyhow::Result;
use vtcode_commons::ErrorCategory;
use vtcode_core::core::agent::error_recovery::ErrorType as RecoveryErrorType;
use vtcode_core::notifications::notify_tool_success;
use vtcode_core::tools::error_messages::agent_execution;
use vtcode_core::tools::registry::ToolExecutionError;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};

use self::auto_mode_probe::push_tool_response_with_auto_mode_probe;
use self::failure_path::{
    finalize_failed_tool_response, log_structured_failure, notify_structured_failure,
    record_recovery_tool_error,
};
pub(crate) use super::error_handling::build_error_content;
use super::error_handling::{format_structured_tool_error_for_user, is_blocked_or_denied_failure};
use super::helpers::{check_is_argument_error, serialize_output, signature_key_for};
pub(crate) use super::response_content::compact_model_tool_payload;
use super::response_content::prepare_tool_response_content;
use super::subagent_memory::{
    merge_subagent_completion_into_memory, record_request_user_input_interview_result,
};

#[cfg(test)]
use super::error_handling::{build_structured_error_content, fallback_from_error};

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

#[cfg(test)]
use super::error_handling::serialize_json_for_model;
#[cfg(test)]
use super::response_content::{
    maybe_inline_spooled, tool_output_summary_input_or_serialized, truncate_stderr_preview,
};
#[cfg(test)]
use super::subagent_memory::{build_subagent_memory_update, parse_subagent_summary_markdown};
#[cfg(test)]
use vtcode_core::config::constants::tools as tool_names;
#[cfg(test)]
use vtcode_core::persistent_memory::GroundedFactRecord;

fn record_tool_execution(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    start_time: std::time::Instant,
    success: bool,
    is_argument_error: bool,
) {
    let duration = start_time.elapsed();
    ctx.tool_health_tracker
        .record_execution(tool_name, success, duration);
    if !is_argument_error {
        ctx.autonomous_executor.record_execution(tool_name, success);
    }
    ctx.telemetry.record_tool_usage(tool_name, success);
}

fn emit_turn_metric_log(
    ctx: &TurnProcessingContext<'_>,
    metric: &'static str,
    tool_name: &str,
    blocked_streak: usize,
    blocked_cap: usize,
) {
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric,
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        plan_mode = ctx.session_stats.is_plan_mode(),
        tool = %tool_name,
        blocked_streak,
        blocked_cap,
        blocked_total = ctx.harness_state.blocked_tool_calls,
        tool_calls = ctx.harness_state.tool_calls,
        "turn metric"
    );
}

/// Main handler for tool execution results.
///
/// This function coordinates:
/// - Recording metrics (circuit breaker, health tracker, telemetry)
/// - Pushing tool responses to conversation history
/// - Handling pipeline output (printing to UI)
/// - Running post-tool-use hooks
/// - Dispatching MCP events
#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_tool_execution_result<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    tool_start_time: std::time::Instant,
) -> Result<Option<TurnHandlerOutcome>> {
    // 1. Record metrics and outcome
    let is_success = matches!(pipeline_outcome.status, ToolExecutionStatus::Success { .. });
    let is_argument_error = if let ToolExecutionStatus::Failure { error } = &pipeline_outcome.status
    {
        check_is_argument_error(&error.message)
    } else {
        false
    };

    record_tool_execution(
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
            if let Some(outcome) =
                handle_failure(t_ctx, tool_call_id, tool_name, args_val, error).await?
            {
                return Ok(Some(outcome));
            }
        }
        ToolExecutionStatus::Timeout { error } => {
            handle_timeout(t_ctx, tool_call_id, tool_name, args_val, error).await?;
        }
        ToolExecutionStatus::Cancelled => {
            handle_cancelled(t_ctx, tool_call_id, tool_name, args_val).await?;
            if t_ctx.ctx.ctrl_c_state.is_exit_requested() {
                return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Exit)));
            }
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
    }

    // 2. Record MCP specific events
    if tool_name.starts_with("mcp_") {
        record_mcp_tool_event(t_ctx, tool_name, &pipeline_outcome.status);
    }

    Ok(None)
}

async fn handle_success<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    output: &serde_json::Value,
) -> Result<()> {
    if let Err(err) = notify_tool_success(tool_name, None).await {
        tracing::debug!(
            tool = %tool_name,
            error = %err,
            "Failed to emit tool success notification"
        );
    }

    // Update blocked-streak and record tool response in grouped context form.
    t_ctx.ctx.reset_blocked_tool_call_streak();
    let content_for_model =
        prepare_tool_response_content(t_ctx.ctx, tool_name, args_val, output).await;
    push_tool_response_with_auto_mode_probe(
        t_ctx,
        tool_call_id.clone(),
        tool_name,
        content_for_model,
    )
    .await?;
    if !vtcode_core::tools::tool_intent::classify_tool_intent(tool_name, args_val).mutating {
        let signature = signature_key_for(tool_name, args_val);
        t_ctx
            .ctx
            .harness_state
            .record_successful_readonly_signature(signature);
    }
    let mut turn_loop_ctx = t_ctx.ctx.as_turn_loop_context();
    let vt_cfg = turn_loop_ctx.vt_cfg;

    // Handle UI output and file modifications
    let (mod_files, _last_stdout) = handle_pipeline_output_from_turn_ctx(
        &mut turn_loop_ctx,
        tool_name,
        args_val,
        pipeline_outcome,
        vt_cfg,
    )
    .await?;

    for f in mod_files {
        t_ctx.turn_modified_files.insert(f);
    }
    t_ctx.ctx.session_stats.record_touched_files(
        t_ctx
            .turn_modified_files
            .iter()
            .map(|path| path.display().to_string()),
    );
    merge_subagent_completion_into_memory(t_ctx.ctx, tool_name, output)?;

    // Run post-tool hooks
    run_post_tool_hooks(t_ctx.ctx, &tool_call_id, tool_name, args_val, output).await?;

    record_request_user_input_interview_result(t_ctx.ctx, tool_name, Some(output));

    Ok(())
}

async fn handle_failure<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    error: &ToolExecutionError,
) -> Result<Option<TurnHandlerOutcome>> {
    let error_str = error.message.as_str();
    let (user_msg, hint) = format_structured_tool_error_for_user(tool_name, error);
    notify_structured_failure(tool_name, &user_msg, None).await;

    let is_plan_mode_denial = matches!(error.category, ErrorCategory::PlanModeViolation)
        || agent_execution::is_plan_mode_denial(error_str);
    let blocked_or_denied_failure = matches!(
        error.category,
        ErrorCategory::InvalidParameters
            | ErrorCategory::PermissionDenied
            | ErrorCategory::PolicyViolation
            | ErrorCategory::PlanModeViolation
    ) || is_blocked_or_denied_failure(error_str);
    log_structured_failure(tool_name, error, hint.as_deref(), "Tool execution failed");

    if is_plan_mode_denial {
        let consecutive_blocked_tool_calls = t_ctx.ctx.harness_state.consecutive_blocked_tool_calls;
        emit_turn_metric_log(
            t_ctx.ctx,
            "plan_mode_denial",
            tool_name,
            consecutive_blocked_tool_calls,
            super::handlers::max_consecutive_blocked_tool_calls_per_turn(t_ctx.ctx),
        );
    }

    // Record genuine tool errors for recovery diagnostics (skip policy denials)
    if !is_plan_mode_denial && !blocked_or_denied_failure {
        record_recovery_tool_error(
            t_ctx.ctx,
            tool_name,
            error,
            RecoveryErrorType::ToolExecution,
        )
        .await;
    }

    finalize_failed_tool_response(t_ctx, tool_call_id, tool_name, args_val, error, "execution")
        .await;

    if blocked_or_denied_failure {
        let streak = t_ctx.ctx.record_blocked_tool_call();
        let max_streak = super::handlers::max_consecutive_blocked_tool_calls_per_turn(t_ctx.ctx);
        if streak > max_streak {
            emit_turn_metric_log(
                t_ctx.ctx,
                "blocked_streak_break",
                tool_name,
                streak,
                max_streak,
            );
            let display_tool = tool_action_label(tool_name, args_val);
            let block_reason = format!(
                "Consecutive blocked/denied tool calls reached per-turn cap ({max_streak}). Last blocked call: '{display_tool}'. Stopping turn to prevent retry churn."
            );
            t_ctx.ctx.push_system_message(block_reason.clone());
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                reason: Some(block_reason),
            })));
        }
    } else {
        t_ctx.ctx.reset_blocked_tool_call_streak();
    }

    Ok(None)
}

async fn handle_timeout(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    error: &ToolExecutionError,
) -> Result<()> {
    let (user_msg, _) = format_structured_tool_error_for_user(tool_name, error);
    notify_structured_failure(tool_name, &user_msg, Some("timeout")).await;
    log_structured_failure(tool_name, error, None, "Tool timed out");

    record_recovery_tool_error(t_ctx.ctx, tool_name, error, RecoveryErrorType::Timeout).await;

    finalize_failed_tool_response(t_ctx, tool_call_id, tool_name, args_val, error, "timeout").await;

    Ok(())
}

async fn handle_cancelled(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
) -> Result<()> {
    let display_tool = tool_action_label(tool_name, args_val);
    let error_msg = format!("Tool '{}' execution cancelled", display_tool);
    t_ctx.ctx.renderer.line(MessageStyle::Info, &error_msg)?;

    let error_content = serde_json::json!({"error": error_msg});
    push_tool_response_with_auto_mode_probe(
        t_ctx,
        tool_call_id,
        tool_name,
        error_content.to_string(),
    )
    .await?;

    record_request_user_input_interview_result(t_ctx.ctx, tool_name, None);

    Ok(())
}

async fn run_post_tool_hooks<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> Result<()> {
    let hooks = ctx.lifecycle_hooks;

    if let Some(hooks) = hooks {
        match hooks
            .run_post_tool_use(tool_name, Some(args_val), output, Some(tool_call_id))
            .await
        {
            Ok(outcome) => {
                crate::agent::runloop::unified::turn::utils::render_hook_messages(
                    ctx.renderer,
                    &outcome.messages,
                )?;
                for context in outcome.additional_context {
                    if !context.trim().is_empty() {
                        ctx.push_system_message(context);
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

/// Record MCP tool execution event for the UI panel.
///
/// This is the canonical MCP event recorder used across all tool execution paths.
pub(crate) fn record_mcp_tool_event(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_name: &str,
    status: &ToolExecutionStatus,
) {
    record_mcp_event_to_panel(t_ctx.ctx.mcp_panel_state, tool_name, status);
}

/// Record MCP tool execution event directly to the MCP panel state.
///
/// This is the low-level MCP event recorder that can be called from any context.
pub(super) fn record_mcp_event_to_panel(
    mcp_panel_state: &mut mcp_events::McpPanelState,
    tool_name: &str,
    status: &ToolExecutionStatus,
) {
    let data_preview = match status {
        ToolExecutionStatus::Success { output, .. } => Some(serialize_output(output)),
        ToolExecutionStatus::Failure { error } | ToolExecutionStatus::Timeout { error } => {
            Some(error.to_json_value().to_string())
        }
        ToolExecutionStatus::Cancelled => {
            Some(serde_json::json!({"error": "Cancelled"}).to_string())
        }
    };

    let mut mcp_event =
        mcp_events::McpEvent::new("mcp".to_string(), tool_name.to_string(), data_preview);

    match status {
        ToolExecutionStatus::Success { .. } => {
            mcp_event.success(None);
        }
        ToolExecutionStatus::Failure { error } => {
            mcp_event.failure(Some(error.user_message()));
        }
        ToolExecutionStatus::Timeout { error } => {
            mcp_event.failure(Some(error.user_message()));
        }
        ToolExecutionStatus::Cancelled => {
            mcp_event.failure(Some("Cancelled".to_string()));
        }
    }

    mcp_panel_state.add_event(mcp_event);
}

#[cfg(test)]
mod tests;
