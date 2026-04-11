//! Tool execution result handling for turn flow.

use anyhow::Result;
use vtcode_commons::ErrorCategory;
use vtcode_core::core::agent::error_recovery::ErrorType as RecoveryErrorType;
use vtcode_core::notifications::{notify_tool_failure, notify_tool_success};
use vtcode_core::tools::error_messages::agent_execution;
use vtcode_core::tools::registry::ToolExecutionError;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::auto_mode::probe_tool_output;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};

pub(crate) use super::error_handling::build_error_content;
use super::error_handling::{
    build_structured_error_content, fallback_from_error, format_structured_tool_error_for_user,
    is_blocked_or_denied_failure,
};
use super::helpers::{check_is_argument_error, serialize_output, signature_key_for};
pub(crate) use super::response_content::compact_model_tool_payload;
use super::response_content::prepare_tool_response_content;
use super::subagent_memory::{
    merge_subagent_completion_into_memory, record_request_user_input_interview_result,
};

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
            if let Some(outcome) =
                handle_failure(t_ctx, tool_call_id, tool_name, args_val, error).await?
            {
                return Ok(Some(outcome));
            }
        }
        ToolExecutionStatus::Timeout { error } => {
            handle_timeout(t_ctx, tool_call_id, tool_name, error).await?;
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

async fn auto_mode_probe_warning(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    content_for_model: &str,
) -> Option<crate::agent::runloop::unified::auto_mode::ProbeWarning> {
    if !ctx.vt_cfg.is_some_and(|cfg| {
        cfg.permissions.default_mode == vtcode_core::config::PermissionMode::Auto
    }) || !ctx.session_stats.is_autonomous_mode()
    {
        return None;
    }

    let permissions = ctx.vt_cfg.map(|cfg| cfg.permissions.clone())?;
    let working_history = ctx.working_history.clone();
    match probe_tool_output(
        ctx.provider_client.as_mut(),
        ctx.config,
        ctx.vt_cfg,
        &permissions,
        &working_history,
        content_for_model,
    )
    .await
    {
        Ok(warning) => warning,
        Err(err) => {
            tracing::warn!(tool = %tool_name, error = %err, "auto mode prompt probe failed");
            None
        }
    }
}

fn append_probe_warning(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    probe_warning: crate::agent::runloop::unified::auto_mode::ProbeWarning,
) -> Result<()> {
    tracing::trace!(tool = %tool_name, probe_hit = true, "auto mode prompt probe flagged tool output");
    ctx.working_history
        .push(vtcode_core::llm::provider::Message::system(
            probe_warning.warning.clone(),
        ));
    ctx.renderer.line(
        MessageStyle::Warning,
        "Auto mode flagged the latest tool output as suspicious prompt injection.",
    )?;
    Ok(())
}

async fn push_tool_response_with_auto_mode_probe(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    content_for_model: String,
) -> Result<()> {
    let probe_warning = auto_mode_probe_warning(t_ctx.ctx, tool_name, &content_for_model).await;
    t_ctx
        .ctx
        .push_tool_response(tool_call_id, content_for_model);
    if let Some(probe_warning) = probe_warning {
        append_probe_warning(t_ctx.ctx, tool_name, probe_warning)?;
    }
    Ok(())
}

async fn notify_structured_failure(
    tool_name: &str,
    user_msg: &str,
    notification_kind: Option<&'static str>,
) {
    if let Err(err) = notify_tool_failure(tool_name, user_msg, notification_kind).await {
        let notification_label = notification_kind.unwrap_or("failure");
        tracing::debug!(
            tool = %tool_name,
            error = %err,
            notification = notification_label,
            "Failed to emit tool failure notification"
        );
    }
}

fn log_structured_failure(
    tool_name: &str,
    error: &ToolExecutionError,
    hint: Option<&str>,
    log_message: &'static str,
) {
    if let Some(hint) = hint {
        tracing::debug!(
            tool = %tool_name,
            category = ?error.category,
            retryable = error.retryable,
            partial_state_possible = error.partial_state_possible,
            hint = %hint,
            error = %error.message,
            "{log_message}"
        );
    } else {
        tracing::debug!(
            tool = %tool_name,
            category = ?error.category,
            retryable = error.retryable,
            partial_state_possible = error.partial_state_possible,
            error = %error.message,
            "{log_message}"
        );
    }
}

async fn record_recovery_tool_error(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    error: &ToolExecutionError,
    error_type: RecoveryErrorType,
) {
    ctx.record_recovery_tool_error(tool_name, error, error_type)
        .await;
}

async fn finalize_failed_tool_response(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    error: &ToolExecutionError,
    failure_kind: &'static str,
) {
    push_tool_error_response(
        t_ctx,
        tool_call_id,
        tool_name,
        error.message.as_str(),
        failure_kind,
        Some(error),
    )
    .await;

    record_request_user_input_interview_result(t_ctx.ctx, tool_name, None);
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
    push_tool_response_with_auto_mode_probe(t_ctx, tool_call_id, tool_name, content_for_model)
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
    run_post_tool_hooks(t_ctx.ctx, tool_name, args_val, output).await?;

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

    finalize_failed_tool_response(t_ctx, tool_call_id, tool_name, error, "execution").await;

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
    error: &vtcode_core::tools::registry::ToolExecutionError,
) -> Result<()> {
    let (user_msg, _) = format_structured_tool_error_for_user(tool_name, error);
    notify_structured_failure(tool_name, &user_msg, Some("timeout")).await;
    log_structured_failure(tool_name, error, None, "Tool timed out");

    record_recovery_tool_error(t_ctx.ctx, tool_name, error, RecoveryErrorType::Timeout).await;

    finalize_failed_tool_response(t_ctx, tool_call_id, tool_name, error, "timeout").await;

    Ok(())
}

async fn push_tool_error_response(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    error_msg: &str,
    failure_kind: &'static str,
    structured_error: Option<&ToolExecutionError>,
) {
    let (fallback_tool, fallback_tool_args) =
        if let Some((tool, args)) = fallback_from_error(tool_name, error_msg) {
            (Some(tool), Some(args))
        } else {
            let fallback = t_ctx
                .ctx
                .tool_registry
                .suggest_fallback_tool(tool_name)
                .await;
            (fallback, None)
        };
    let error_content = match structured_error {
        Some(error) => {
            build_structured_error_content(error, fallback_tool, fallback_tool_args, failure_kind)
        }
        None => build_error_content(
            error_msg.to_string(),
            fallback_tool,
            fallback_tool_args,
            failure_kind,
        ),
    };
    let serialized = error_content.to_string();
    if let Err(err) =
        push_tool_response_with_auto_mode_probe(t_ctx, tool_call_id, tool_name, serialized).await
    {
        tracing::warn!(tool = %tool_name, error = %err, "failed to push probed tool error response");
    }
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
    tool_name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> Result<()> {
    let hooks = ctx.lifecycle_hooks;

    if let Some(hooks) = hooks {
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
mod tests {
    use super::*;
    use std::borrow::Cow;
    use tempfile::tempdir;
    use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};

    #[test]
    fn fallback_from_error_extracts_unified_exec_poll() {
        let error = "Tool failed. Use unified_exec with action=\"poll\" and session_id=\"run-ab12\" instead of read_file.";
        let fallback = fallback_from_error(tool_names::UNIFIED_FILE, error);
        assert_eq!(
            fallback,
            Some((
                tool_names::UNIFIED_EXEC.to_string(),
                serde_json::json!({"action":"poll","session_id":"run-ab12"}),
            ))
        );
    }

    #[test]
    fn fallback_from_error_recovers_unified_search_invalid_read_action() {
        let error = "Tool execution failed: Invalid action: read";
        let fallback = fallback_from_error(tool_names::UNIFIED_SEARCH, error);
        assert_eq!(
            fallback,
            Some((
                tool_names::UNIFIED_SEARCH.to_string(),
                serde_json::json!({
                    "action": "list",
                    "path": "."
                }),
            ))
        );
    }

    #[test]
    fn fallback_from_error_extracts_read_file_for_patch_context_mismatch() {
        let error = "Tool 'apply_patch' execution failed: failed to locate expected lines in 'vtcode-exec-events/src/trace.rs': context mismatch";
        let fallback = fallback_from_error(tool_names::APPLY_PATCH, error);
        assert_eq!(
            fallback,
            Some((
                tool_names::READ_FILE.to_string(),
                serde_json::json!({
                    "path": "vtcode-exec-events/src/trace.rs",
                    "offset": 1,
                    "limit": 120
                }),
            ))
        );
    }

    #[test]
    fn fallback_from_error_uses_task_tracker_list_for_update_argument_errors() {
        let error = "Tool 'task_tracker' execution failed: Tool execution failed: 'index' is required for 'update' (1-indexed)";
        let fallback = fallback_from_error(tool_names::TASK_TRACKER, error);
        assert_eq!(
            fallback,
            Some((
                tool_names::TASK_TRACKER.to_string(),
                serde_json::json!({"action": "list"}),
            ))
        );
    }

    #[test]
    fn build_error_content_includes_fallback_args() {
        let payload = build_error_content(
            "boom".to_string(),
            Some(tool_names::READ_PTY_SESSION.to_string()),
            Some(serde_json::json!({"session_id":"run-1"})),
            "execution",
        );

        assert_eq!(
            payload.get("fallback_tool").and_then(|v| v.as_str()),
            Some(tool_names::READ_PTY_SESSION)
        );
        assert_eq!(
            payload.get("fallback_tool_args"),
            Some(&serde_json::json!({"session_id":"run-1"}))
        );
        assert_eq!(
            payload.get("error_class").and_then(|v| v.as_str()),
            Some("execution_failure")
        );
        assert_eq!(
            payload.get("is_recoverable").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn build_error_content_truncates_large_errors() {
        let large_error = format!("Tool failed: {}", "x".repeat(700));
        let payload = build_error_content(large_error, None, None, "execution");

        assert_eq!(
            payload.get("error_truncated").and_then(|v| v.as_bool()),
            Some(true)
        );
        let rendered = payload
            .get("error")
            .and_then(|v| v.as_str())
            .expect("error field");
        assert!(rendered.contains("[truncated]"));
    }

    #[test]
    fn build_error_content_marks_policy_denials_non_recoverable() {
        let payload = build_error_content(
            "tool permission denied by policy".to_string(),
            None,
            None,
            "execution",
        );

        assert_eq!(
            payload.get("error_class").and_then(|v| v.as_str()),
            Some("policy_blocked")
        );
        assert_eq!(
            payload.get("is_recoverable").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(payload.get("next_action").is_none());
    }

    #[test]
    fn build_error_content_compacts_large_fallback_args() {
        let payload = build_error_content(
            "boom".to_string(),
            Some(tool_names::READ_FILE.to_string()),
            Some(serde_json::json!({"content": "x".repeat(600)})),
            "execution",
        );

        assert!(payload.get("fallback_tool_args").is_none());
        assert_eq!(
            payload
                .get("fallback_tool_args_truncated")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(
            payload
                .get("fallback_tool_args_preview")
                .and_then(|v| v.as_str())
                .is_some()
        );
        assert_eq!(
            payload.get("next_action").and_then(|v| v.as_str()),
            Some("Try an alternative tool or narrower scope.")
        );
    }

    #[test]
    fn build_error_content_keeps_structured_fallback_fields_only() {
        let payload = build_error_content(
            "boom".to_string(),
            Some(tool_names::UNIFIED_SEARCH.to_string()),
            Some(serde_json::json!({"action":"list","path":"."})),
            "execution",
        );

        assert_eq!(
            payload.get("fallback_tool"),
            Some(&serde_json::json!(tool_names::UNIFIED_SEARCH))
        );
        assert_eq!(
            payload.get("fallback_tool_args"),
            Some(&serde_json::json!({"action":"list","path":"."}))
        );
        assert_eq!(
            payload.get("is_recoverable").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            payload.get("next_action").and_then(|v| v.as_str()),
            Some("Try an alternative tool or narrower scope.")
        );
    }

    #[test]
    fn build_structured_error_content_preserves_retry_and_partial_state_fields() {
        let mut error = ToolExecutionError::new(
            tool_names::WRITE_FILE.to_string(),
            ToolErrorType::ExecutionError,
            "write failed".to_string(),
        )
        .with_partial_state(true, false)
        .with_surface("unified_runloop")
        .with_attempt(2);
        error.is_recoverable = true;
        error.retryable = true;
        error.retry_delay_ms = Some(750);
        error.recovery_suggestions = vec![Cow::Borrowed("Retry with smaller scope.")];

        let payload = build_structured_error_content(&error, None, None, "execution");

        assert_eq!(
            payload
                .get("partial_state_possible")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            payload
                .get("retry_delay_ms")
                .and_then(|value| value.as_u64()),
            Some(750)
        );
        assert_eq!(
            payload.get("next_action").and_then(|value| value.as_str()),
            Some("Retry with smaller scope.")
        );
        assert_eq!(
            payload
                .get("error")
                .and_then(|value| value.get("partial_state_possible"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn structured_retry_summary_ignores_initial_attempt() {
        let error = ToolExecutionError::new(
            tool_names::READ_FILE.to_string(),
            ToolErrorType::ExecutionError,
            "read failed".to_string(),
        )
        .with_attempt(1);

        assert_eq!(error.retry_summary(), None);
    }

    #[test]
    fn build_structured_error_content_round_trips_tool_error() {
        let mut error = ToolExecutionError::new(
            tool_names::WRITE_FILE.to_string(),
            ToolErrorType::ExecutionError,
            "write failed".to_string(),
        )
        .with_partial_state(true, false)
        .with_surface("unified_runloop")
        .with_attempt(2);
        error.retry_delay_ms = Some(750);

        let payload = build_structured_error_content(&error, None, None, "execution");
        let parsed = ToolExecutionError::from_tool_output(&payload).expect("structured error");

        assert_eq!(parsed.tool_name, tool_names::WRITE_FILE);
        assert!(parsed.partial_state_possible);
        assert_eq!(parsed.retry_delay_ms, Some(750));
    }

    #[test]
    fn maybe_inline_spooled_removes_redundant_fields() {
        let serialized = maybe_inline_spooled(
            tool_names::UNIFIED_EXEC,
            &serde_json::json!({
                "output": "tail",
                "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
                "spool_hint": "verbose hint",
                "spooled_bytes": 12345,
                "success": true,
                "status": "success",
                "message": "ok",
                "metadata": {"size_bytes": 100},
                "no_spool": false,
                "id": "run-1",
                "session_id": "run-1",
                "process_id": "run-1",
                "command": "cargo check -p vtcode",
                "is_exited": false,
                "working_directory": null,
                "rows": 24,
                "cols": 80,
                "wall_time": 1.23,
                "stderr": "warn",
                "stderr_preview": "warn",
                "follow_up_prompt": "More output available.",
                "has_more": false,
                "truncated": false,
                "auto_recovered": false,
                "query_truncated": false,
                "stdout": "tail",
                "next_continue_args": {
                    "action": "continue",
                    "session_id": "run-1"
                }
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert!(parsed.get("spool_hint").is_none());
        assert!(parsed.get("spooled_bytes").is_none());
        assert!(parsed.get("spooled_to_file").is_none());
        assert!(parsed.get("success").is_none());
        assert!(parsed.get("status").is_none());
        assert!(parsed.get("message").is_none());
        assert!(parsed.get("metadata").is_none());
        assert!(parsed.get("no_spool").is_none());
        assert!(parsed.get("id").is_none());
        assert!(parsed.get("process_id").is_none());
        assert!(parsed.get("command").is_none());
        assert!(parsed.get("is_exited").is_none());
        assert!(parsed.get("working_directory").is_none());
        assert!(parsed.get("rows").is_none());
        assert!(parsed.get("cols").is_none());
        assert!(parsed.get("wall_time").is_none());
        assert!(parsed.get("follow_up_prompt").is_none());
        assert!(parsed.get("has_more").is_none());
        assert!(parsed.get("truncated").is_none());
        assert!(parsed.get("auto_recovered").is_none());
        assert!(parsed.get("query_truncated").is_none());
        assert_eq!(
            parsed.get("stderr_preview"),
            Some(&serde_json::json!("warn"))
        );
        assert!(parsed.get("stdout").is_none());
        assert!(parsed.get("next_poll_args").is_none());
        assert!(parsed.get("preferred_next_action").is_none());
        assert!(parsed.get("session_id").is_none());
        assert_eq!(
            parsed.get("next_continue_args"),
            Some(&serde_json::json!({"s":"run-1"}))
        );
    }

    #[test]
    fn maybe_inline_spooled_compacts_next_read_duplicates() {
        let serialized = maybe_inline_spooled(
            tool_names::READ_FILE,
            &serde_json::json!({
                "path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
                "spool_path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
                "has_more": true,
                "next_offset": 81,
                "chunk_limit": 40,
                "next_read_args": {
                    "path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
                    "offset": 81,
                    "limit": 40
                }
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert!(parsed.get("spooled_to_file").is_none());
        assert!(parsed.get("has_more").is_none());
        assert!(parsed.get("next_offset").is_none());
        assert!(parsed.get("chunk_limit").is_none());
        assert!(parsed.get("spool_path").is_none());
        assert_eq!(
            parsed.get("path"),
            Some(&serde_json::json!(
                ".vtcode/context/tool_outputs/unified_exec_1.txt"
            ))
        );
        assert_eq!(
            parsed.get("next_read_args"),
            Some(&serde_json::json!({
                "p": ".vtcode/context/tool_outputs/unified_exec_1.txt",
                "o": 81,
                "l": 40
            }))
        );
    }

    #[test]
    fn maybe_inline_spooled_preserves_extra_continue_args_fields() {
        let serialized = maybe_inline_spooled(
            tool_names::UNIFIED_EXEC,
            &serde_json::json!({
                "next_continue_args": {
                    "action": "continue",
                    "session_id": "run-1",
                    "cursor": 42
                }
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert_eq!(
            parsed.get("next_continue_args"),
            Some(&serde_json::json!({
                "s": "run-1",
                "cursor": 42
            }))
        );
    }

    #[test]
    fn maybe_inline_spooled_keeps_loop_recovery_fields_and_drops_notes() {
        let serialized = maybe_inline_spooled(
            tool_names::READ_FILE,
            &serde_json::json!({
                "loop_detected": true,
                "spool_path": ".vtcode/context/tool_outputs/unified_exec_loop.txt",
                "next_read_args": {
                    "path": ".vtcode/context/tool_outputs/unified_exec_loop.txt",
                    "offset": 81,
                    "limit": 40
                },
                "reused_spooled_output": true,
                "spool_ref_only": true,
                "loop_detected_note": "Read the spool file instead of re-running this call.",
                "repeat_count": 4,
                "limit": 3,
                "tool": "read_file"
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert_eq!(parsed.get("loop_detected"), Some(&serde_json::json!(true)));
        assert_eq!(
            parsed.get("spool_path"),
            Some(&serde_json::json!(
                ".vtcode/context/tool_outputs/unified_exec_loop.txt"
            ))
        );
        assert_eq!(
            parsed.get("next_read_args"),
            Some(&serde_json::json!({
                "p": ".vtcode/context/tool_outputs/unified_exec_loop.txt",
                "o": 81,
                "l": 40
            }))
        );
        assert!(parsed.get("reused_spooled_output").is_none());
        assert!(parsed.get("spool_ref_only").is_none());
        assert!(parsed.get("loop_detected_note").is_none());
        assert!(parsed.get("repeat_count").is_none());
        assert!(parsed.get("limit").is_none());
        assert!(parsed.get("tool").is_none());
    }

    #[test]
    fn maybe_inline_spooled_uses_reference_only_for_spooled_exec_output() {
        let serialized = maybe_inline_spooled(
            tool_names::UNIFIED_EXEC,
            &serde_json::json!({
                "output": "preview text",
                "stdout": "preview text",
                "stderr": "warning text",
                "spool_path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
                "exit_code": 0,
                "is_exited": true
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert!(parsed.get("output").is_none());
        assert!(parsed.get("stdout").is_none());
        assert!(parsed.get("stderr").is_none());
        assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(0)));
        assert_eq!(
            parsed.get("spool_path"),
            Some(&serde_json::json!(
                ".vtcode/context/tool_outputs/unified_exec_1.txt"
            ))
        );
        assert_eq!(
            parsed.get("stderr_preview"),
            Some(&serde_json::json!("warning text"))
        );
        assert_eq!(
            parsed.get("result_ref_only"),
            Some(&serde_json::json!(true))
        );
    }

    #[test]
    fn maybe_inline_spooled_drops_terminal_exec_metadata_without_continuation() {
        let serialized = maybe_inline_spooled(
            tool_names::UNIFIED_EXEC,
            &serde_json::json!({
                "output": "ok",
                "command": "cargo check -p vtcode-core",
                "session_id": "run-1",
                "process_id": "run-1",
                "working_directory": "/workspace",
                "is_exited": true,
                "exit_code": 0,
                "rows": 24,
                "cols": 80,
                "wall_time": 0.5
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert_eq!(parsed.get("output"), Some(&serde_json::json!("ok")));
        assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(0)));
        assert!(parsed.get("command").is_none());
        assert!(parsed.get("session_id").is_none());
        assert!(parsed.get("process_id").is_none());
        assert!(parsed.get("working_directory").is_none());
        assert!(parsed.get("is_exited").is_none());
        assert!(parsed.get("rows").is_none());
        assert!(parsed.get("cols").is_none());
        assert!(parsed.get("wall_time").is_none());
    }

    #[test]
    fn maybe_inline_spooled_keeps_exec_recovery_guidance() {
        let serialized = maybe_inline_spooled(
            tool_names::UNIFIED_EXEC,
            &serde_json::json!({
                "output": "bash: pip: command not found",
                "command": "pip install pymupdf",
                "session_id": "run-127",
                "process_id": "run-127",
                "is_exited": true,
                "exit_code": 127,
                "critical_note": "Command `pip` was not found in PATH.",
                "next_action": "Check the command name or install the missing binary, then rerun the command."
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert_eq!(
            parsed.get("output"),
            Some(&serde_json::json!("bash: pip: command not found"))
        );
        assert_eq!(parsed.get("exit_code"), Some(&serde_json::json!(127)));
        assert_eq!(
            parsed.get("critical_note"),
            Some(&serde_json::json!("Command `pip` was not found in PATH."))
        );
        assert_eq!(
            parsed.get("next_action"),
            Some(&serde_json::json!(
                "Check the command name or install the missing binary, then rerun the command."
            ))
        );
        assert!(parsed.get("command").is_none());
        assert!(parsed.get("session_id").is_none());
        assert!(parsed.get("process_id").is_none());
        assert!(parsed.get("is_exited").is_none());
    }

    #[test]
    fn maybe_inline_spooled_keeps_recoverable_failure_next_action() {
        let serialized = maybe_inline_spooled(
            tool_names::READ_FILE,
            &serde_json::json!({
                "error": "Tool preflight validation failed: x",
                "is_recoverable": true,
                "next_action": "Retry with fallback_tool_args.",
                "fallback_tool": tool_names::UNIFIED_SEARCH,
                "fallback_tool_args": {"action":"list","path":"."}
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert_eq!(
            parsed.get("next_action"),
            Some(&serde_json::json!("Retry with fallback_tool_args."))
        );
        assert_eq!(
            parsed.get("fallback_tool"),
            Some(&serde_json::json!(tool_names::UNIFIED_SEARCH))
        );
    }

    #[test]
    fn maybe_inline_spooled_keeps_structural_recovery_success_next_action() {
        let serialized = maybe_inline_spooled(
            tool_names::UNIFIED_SEARCH,
            &serde_json::json!({
                "backend": "ast-grep",
                "matches": [],
                "is_recoverable": true,
                "hint": "Pattern looks like a code fragment.",
                "next_action": "Retry with a larger parseable pattern."
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert_eq!(
            parsed.get("next_action"),
            Some(&serde_json::json!("Retry with a larger parseable pattern."))
        );
        assert_eq!(
            parsed.get("hint"),
            Some(&serde_json::json!("Pattern looks like a code fragment."))
        );
    }

    #[test]
    fn maybe_inline_spooled_drops_non_recoverable_failure_next_action() {
        let serialized = maybe_inline_spooled(
            tool_names::READ_FILE,
            &serde_json::json!({
                "error": "tool permission denied by policy",
                "is_recoverable": false,
                "next_action": "Switch to an allowed tool or mode."
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert!(parsed.get("next_action").is_none());
    }

    #[test]
    fn maybe_inline_spooled_drops_generic_success_recovery_guidance() {
        let serialized = maybe_inline_spooled(
            tool_names::READ_FILE,
            &serde_json::json!({
                "output": "ok",
                "critical_note": "This should not survive for non-exec payloads.",
                "next_action": "This should stay compacted away."
            }),
        );

        let parsed: serde_json::Value =
            serde_json::from_str(&serialized).expect("serialized JSON payload");
        assert_eq!(parsed.get("output"), Some(&serde_json::json!("ok")));
        assert!(parsed.get("critical_note").is_none());
        assert!(parsed.get("next_action").is_none());
    }

    #[tokio::test]
    async fn tool_output_summary_input_uses_spool_file_tail_for_exec_output() {
        let temp = tempdir().unwrap();
        let spool_dir = temp.path().join(".vtcode/context/tool_outputs");
        std::fs::create_dir_all(&spool_dir).unwrap();
        let spool_path = spool_dir.join("unified_exec_1.txt");
        let spool_content = (1..=150)
            .map(|idx| format!("line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&spool_path, spool_content).unwrap();

        let output = serde_json::json!({
            "output": "preview text",
            "stderr_preview": "warning text",
            "spool_path": ".vtcode/context/tool_outputs/unified_exec_1.txt",
            "exit_code": 0,
            "is_exited": true
        });
        let serialized = serialize_json_for_model(&output);

        let input = tool_output_summary_input_or_serialized(
            temp.path(),
            tool_names::UNIFIED_EXEC,
            &output,
            &serialized,
        )
        .await;

        assert!(input.contains("Tool payload:"));
        assert!(input.contains("stderr_preview:\nwarning text"));
        assert!(input.contains("tail_excerpt:"));
        assert!(input.contains("line-150"));
        assert!(!input.contains("line-1\nline-2\nline-3"));
    }

    #[tokio::test]
    async fn tool_output_summary_input_uses_spool_file_excerpt_for_large_reads() {
        let temp = tempdir().unwrap();
        let spool_dir = temp.path().join(".vtcode/context/tool_outputs");
        std::fs::create_dir_all(&spool_dir).unwrap();
        let spool_path = spool_dir.join("read_1.txt");
        let spool_content = (1..=200)
            .map(|idx| format!("read-line-{idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&spool_path, spool_content).unwrap();

        let output = serde_json::json!({
            "path": "src/main.rs",
            "spool_path": ".vtcode/context/tool_outputs/read_1.txt"
        });
        let serialized = serialize_json_for_model(&output);

        let input = tool_output_summary_input_or_serialized(
            temp.path(),
            tool_names::READ_FILE,
            &output,
            &serialized,
        )
        .await;

        assert!(input.contains("source_path: src/main.rs"));
        assert!(input.contains("content_excerpt:"));
        assert!(input.contains("read-line-1"));
        assert!(input.contains("read-line-200"));
    }

    #[tokio::test]
    async fn tool_output_summary_input_falls_back_to_serialized_output_when_spool_missing() {
        let temp = tempdir().unwrap();
        let output = serde_json::json!({
            "spool_path": ".vtcode/context/tool_outputs/missing.txt",
            "exit_code": 0,
            "is_exited": true
        });
        let serialized = serialize_json_for_model(&output);

        let input = tool_output_summary_input_or_serialized(
            temp.path(),
            tool_names::UNIFIED_EXEC,
            &output,
            &serialized,
        )
        .await;

        assert_eq!(input, serialized);
    }

    #[tokio::test]
    async fn tool_output_summary_input_decodes_invalid_utf8_spool_lossily() {
        let temp = tempdir().unwrap();
        let spool_dir = temp.path().join(".vtcode/context/tool_outputs");
        std::fs::create_dir_all(&spool_dir).unwrap();
        let spool_path = spool_dir.join("unified_exec_invalid.txt");
        std::fs::write(&spool_path, b"ok\n\xff\xfe\nlast line\n").unwrap();

        let output = serde_json::json!({
            "output": "preview text",
            "stderr_preview": "warning text",
            "spool_path": ".vtcode/context/tool_outputs/unified_exec_invalid.txt",
            "exit_code": 0,
            "is_exited": true
        });
        let serialized = serialize_json_for_model(&output);

        let input = tool_output_summary_input_or_serialized(
            temp.path(),
            tool_names::UNIFIED_EXEC,
            &output,
            &serialized,
        )
        .await;

        assert!(input.contains("warning text"));
        assert!(input.contains("last line"));
        assert_ne!(input, serialized);
    }

    #[test]
    fn blocked_or_denied_failure_detects_guardable_errors() {
        assert!(is_blocked_or_denied_failure(
            "plan_task_tracker is a Plan Mode compatibility alias. Use task_tracker in Edit mode, or switch to Plan Mode."
        ));
        assert!(is_blocked_or_denied_failure("Tool permission denied"));
        assert!(is_blocked_or_denied_failure(
            "Policy violation: exceeded max tool calls per turn (32)"
        ));
        assert!(is_blocked_or_denied_failure(
            "Safety validation failed: command pattern denied"
        ));
    }

    #[test]
    fn blocked_or_denied_failure_ignores_runtime_execution_failures() {
        assert!(!is_blocked_or_denied_failure(
            "command exited with status 1"
        ));
        assert!(!is_blocked_or_denied_failure(
            "stream request timed out after 30000ms"
        ));
    }

    #[test]
    fn parse_subagent_summary_markdown_reads_fixed_contract() {
        let parsed = parse_subagent_summary_markdown(
            "## Summary\n- Investigated compaction flow\n\n## Facts\n- `context.dynamic.retained_user_messages` defaults to 4\n- `read_file` duplicates are deduped locally\n\n## Touched Files\n- src/agent/runloop/unified/turn/compaction.rs\n\n## Verification\n- Run cargo check\n\n## Open Questions\n- Should batch reads be deduped too?\n",
        )
        .expect("structured summary should parse");

        assert_eq!(parsed.summary, vec!["Investigated compaction flow"]);
        assert_eq!(
            parsed.facts,
            vec![
                "`context.dynamic.retained_user_messages` defaults to 4",
                "`read_file` duplicates are deduped locally",
            ]
        );
        assert_eq!(
            parsed.touched_files,
            vec!["src/agent/runloop/unified/turn/compaction.rs"]
        );
        assert_eq!(parsed.verification, vec!["Run cargo check"]);
        assert_eq!(
            parsed.open_questions,
            vec!["Should batch reads be deduped too?"]
        );
    }

    #[test]
    fn parse_subagent_summary_markdown_treats_none_sections_as_empty() {
        let parsed = parse_subagent_summary_markdown(
            "## Summary\n- None\n\n## Facts\n- None\n\n## Touched Files\n- None\n\n## Verification\n- None\n\n## Open Questions\n- None\n",
        )
        .expect("structured summary should parse");

        assert!(parsed.summary.is_empty());
        assert!(parsed.facts.is_empty());
        assert!(parsed.touched_files.is_empty());
        assert!(parsed.verification.is_empty());
        assert!(parsed.open_questions.is_empty());
    }

    #[test]
    fn parse_subagent_summary_markdown_rejects_unstructured_text() {
        assert!(parse_subagent_summary_markdown("plain paragraph without headings").is_none());
    }

    #[test]
    fn build_subagent_memory_update_aggregates_structured_child_results() {
        let output = serde_json::json!({
            "completed": true,
            "entry": {
                "agent_name": "reviewer",
                "summary": "## Summary\n- Investigated compaction flow\n- Confirmed contract\n\n## Facts\n- Local compaction dedupes repeated reads\n\n## Touched Files\n- src/agent/runloop/unified/turn/compaction.rs\n\n## Verification\n- Run cargo check\n\n## Open Questions\n- None\n"
            }
        });

        let update = build_subagent_memory_update(&output).expect("update");

        assert_eq!(
            update.grounded_facts,
            vec![GroundedFactRecord {
                fact: "Local compaction dedupes repeated reads".to_string(),
                source: "subagent:reviewer".to_string(),
            }]
        );
        assert_eq!(
            update.touched_files,
            vec!["src/agent/runloop/unified/turn/compaction.rs".to_string()]
        );
        assert_eq!(
            update.verification_todo,
            vec!["Run cargo check".to_string()]
        );
        assert_eq!(
            update.delegation_notes,
            vec!["reviewer: Investigated compaction flow | Confirmed contract".to_string()]
        );
    }

    #[test]
    fn build_subagent_memory_update_falls_back_to_raw_summary() {
        let output = serde_json::json!({
            "status": "completed",
            "agent_name": "worker",
            "summary": "plain child summary"
        });

        let update = build_subagent_memory_update(&output).expect("update");

        assert!(update.grounded_facts.is_empty());
        assert!(update.touched_files.is_empty());
        assert_eq!(
            update.delegation_notes,
            vec!["worker: plain child summary".to_string()]
        );
    }

    #[test]
    fn build_subagent_memory_update_ignores_empty_structured_summary() {
        let output = serde_json::json!({
            "status": "completed",
            "agent_name": "worker",
            "summary": "## Summary\n- None\n\n## Facts\n- None\n\n## Touched Files\n- None\n\n## Verification\n- None\n\n## Open Questions\n- None\n"
        });

        assert!(build_subagent_memory_update(&output).is_none());
    }

    #[test]
    fn stderr_preview_truncates_unicode_safely() {
        let stderr = "an’t ".repeat(200);
        let preview = truncate_stderr_preview(&stderr);
        assert!(preview.ends_with("... (truncated)"));
    }
}
