//! Tool execution result handling for turn flow.

use anyhow::Result;
use vtcode_core::config::constants::defaults::DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::error_messages::agent_execution;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::turn_helpers::display_status;

use super::helpers::{
    EXIT_PLAN_MODE_REASON_AUTO_TRIGGER_ON_DENIAL, build_exit_plan_mode_args,
    build_exit_plan_mode_call_id, check_is_argument_error, push_tool_response, serialize_output,
};

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

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

fn max_consecutive_blocked_tool_calls_per_turn(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.vt_cfg
        .map(|cfg| cfg.tools.max_consecutive_blocked_tool_calls_per_turn)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_CONSECUTIVE_BLOCKED_TOOL_CALLS_PER_TURN)
}

fn is_blocked_or_denied_failure(error: &str) -> bool {
    if agent_execution::is_plan_mode_denial(error) {
        return true;
    }

    let lowered = error.to_ascii_lowercase();
    [
        "tool permission denied",
        "policy violation:",
        "safety validation failed",
        "tool argument validation failed",
        "not allowed in plan mode",
        "only available when plan mode is active",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
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

/// Build standardized error content for tool failures.
///
/// This is the canonical error content builder used across all tool execution paths.
pub(crate) fn build_error_content(
    error_msg: String,
    fallback_tool: Option<String>,
    fallback_tool_args: Option<serde_json::Value>,
    failure_kind: &'static str,
) -> serde_json::Value {
    if let Some(tool) = fallback_tool {
        let suggestion = if let Some(args) = fallback_tool_args.as_ref() {
            format!("Try '{}' with args {} as a fallback approach.", tool, args)
        } else {
            format!("Try '{}' as a fallback approach.", tool)
        };
        let mut payload = serde_json::json!({
            "error": error_msg,
            "failure_kind": failure_kind,
            "fallback_tool": tool,
            "fallback_suggestion": suggestion,
        });
        if let Some(args) = fallback_tool_args
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("fallback_tool_args".to_string(), args);
        }
        payload
    } else {
        serde_json::json!({
            "error": error_msg,
            "failure_kind": failure_kind,
        })
    }
}

fn is_valid_pty_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn extract_pty_session_id_from_error(error_msg: &str) -> Option<String> {
    let marker = "session_id=\"";
    let start = error_msg.find(marker)? + marker.len();
    let rest = &error_msg[start..];
    let end = rest.find('"')?;
    let session_id = &rest[..end];
    if is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn extract_patch_target_path_from_error(error_msg: &str) -> Option<String> {
    let markers = [
        "failed to locate expected lines in ",
        "failed to locate expected text in ",
    ];
    for marker in markers {
        let Some(start) = error_msg.find(marker) else {
            continue;
        };
        let rest = &error_msg[start + marker.len()..];
        for quote in ['\'', '"'] {
            let quote_s = quote.to_string();
            let Some(stripped) = rest.strip_prefix(&quote_s) else {
                continue;
            };
            let Some(end_idx) = stripped.find(quote) else {
                continue;
            };
            let path = stripped[..end_idx].trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

fn fallback_from_error(tool_name: &str, error_msg: &str) -> Option<(String, serde_json::Value)> {
    if matches!(
        tool_name,
        tool_names::UNIFIED_FILE | tool_names::READ_FILE | "read file" | "repo_browser.read_file"
    ) && let Some(session_id) = extract_pty_session_id_from_error(error_msg)
    {
        return Some((
            tool_names::READ_PTY_SESSION.to_string(),
            serde_json::json!({ "session_id": session_id }),
        ));
    }

    if matches!(
        tool_name,
        tool_names::APPLY_PATCH | tool_names::UNIFIED_FILE | "apply patch"
    ) && let Some(path) = extract_patch_target_path_from_error(error_msg)
    {
        return Some((
            tool_names::READ_FILE.to_string(),
            serde_json::json!({
                "path": path,
                "offset": 1,
                "limit": 120
            }),
        ));
    }

    None
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
) -> Result<Option<TurnHandlerOutcome>> {
    // 1. Record metrics and outcome
    let is_success = matches!(pipeline_outcome.status, ToolExecutionStatus::Success { .. });
    let is_argument_error = if let ToolExecutionStatus::Failure { error } = &pipeline_outcome.status
    {
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
            if let Some(outcome) = handle_failure(
                t_ctx,
                tool_call_id,
                tool_name,
                args_val,
                error,
                tool_start_time,
            )
            .await?
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

fn maybe_inline_spooled(_tool_name: &str, output: &serde_json::Value) -> String {
    serialize_output(output)
}

fn is_pty_like_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        tool_names::RUN_PTY_CMD
            | tool_names::READ_PTY_SESSION
            | tool_names::SEND_PTY_INPUT
            | tool_names::UNIFIED_EXEC
            | tool_names::SHELL
            | tool_names::EXECUTE_CODE
    )
}

async fn handle_success<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    output: &serde_json::Value,
) -> Result<()> {
    t_ctx.ctx.harness_state.reset_blocked_tool_call_streak();

    let content_for_model = maybe_inline_spooled(tool_name, output);
    push_tool_response(t_ctx.ctx.working_history, tool_call_id, content_for_model);

    if let Some(spool_path) = output.get("spool_path").and_then(|v| v.as_str()) {
        let nudge = if is_pty_like_tool(tool_name) {
            format!(
                "Output was large; only a tail preview is kept inline. Full command output is in \"{}\". Use read_file path=\"{}\" (or read_pty_session for live session polling).",
                spool_path, spool_path
            )
        } else {
            format!(
                "Output was large and condensed. Full content saved to \"{}\". Use read_file or grep_file if you need more.",
                spool_path
            )
        };
        t_ctx.ctx.working_history.push(uni::Message::system(nudge));
    }

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
    args_val: &serde_json::Value,
    error: &anyhow::Error,
    tool_start_time: std::time::Instant,
) -> Result<Option<TurnHandlerOutcome>> {
    let error_str = error.to_string();
    let is_plan_mode_denial = agent_execution::is_plan_mode_denial(&error_str);
    let blocked_or_denied_failure = is_blocked_or_denied_failure(&error_str);
    let should_auto_exit = is_plan_mode_denial
        && t_ctx.ctx.session_stats.is_plan_mode()
        && !t_ctx
            .ctx
            .tool_registry
            .is_plan_mode_allowed(tool_name, args_val);

    let error_msg = format!("Tool '{}' execution failed: {}", tool_name, error);
    tracing::debug!(tool = %tool_name, error = %error, "Tool execution failed");
    if is_plan_mode_denial {
        emit_turn_metric_log(
            t_ctx.ctx,
            "plan_mode_denial",
            tool_name,
            t_ctx.ctx.harness_state.consecutive_blocked_tool_calls,
            max_consecutive_blocked_tool_calls_per_turn(t_ctx.ctx),
        );
    }

    push_tool_error_response(t_ctx, tool_call_id, tool_name, error_msg, "execution").await;

    // Handle auto-exit from Plan Mode if applicable
    if should_auto_exit
        && let Some(outcome) = handle_plan_mode_auto_exit(t_ctx, tool_start_time).await?
    {
        return Ok(Some(outcome));
    }

    if blocked_or_denied_failure {
        let streak = t_ctx.ctx.harness_state.record_blocked_tool_call();
        let max_streak = max_consecutive_blocked_tool_calls_per_turn(t_ctx.ctx);
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
            t_ctx
                .ctx
                .working_history
                .push(uni::Message::system(block_reason.clone()));
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                reason: Some(block_reason),
            })));
        }
    } else {
        t_ctx.ctx.harness_state.reset_blocked_tool_call_streak();
    }

    Ok(None)
}

async fn handle_timeout(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    error: &vtcode_core::tools::registry::ToolExecutionError,
) -> Result<()> {
    let error_msg = format!("Tool '{}' timed out: {}", tool_name, error.message);
    tracing::debug!(tool = %tool_name, error = %error.message, "Tool timed out");

    push_tool_error_response(t_ctx, tool_call_id, tool_name, error_msg, "timeout").await;

    Ok(())
}

async fn push_tool_error_response(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    error_msg: String,
    failure_kind: &'static str,
) {
    let (fallback_tool, fallback_tool_args) =
        if let Some((tool, args)) = fallback_from_error(tool_name, &error_msg) {
            (Some(tool), Some(args))
        } else {
            (
                t_ctx
                    .ctx
                    .tool_registry
                    .suggest_fallback_tool(tool_name)
                    .await,
                None,
            )
        };
    let error_content =
        build_error_content(error_msg, fallback_tool, fallback_tool_args, failure_kind);
    push_tool_response(
        t_ctx.ctx.working_history,
        tool_call_id,
        error_content.to_string(),
    );
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
) -> Result<Option<TurnHandlerOutcome>> {
    if *t_ctx.ctx.auto_exit_plan_mode_attempted {
        display_status(
            t_ctx.ctx.renderer,
            "Plan Mode still active. Call `exit_plan_mode` to review the plan or refine the plan before retrying.",
        )?;
        return Ok(None);
    }
    *t_ctx.ctx.auto_exit_plan_mode_attempted = true;

    let exit_args = build_exit_plan_mode_args(EXIT_PLAN_MODE_REASON_AUTO_TRIGGER_ON_DENIAL);

    // Generate a unique ID for the injected call
    let exit_call_id = build_exit_plan_mode_call_id(
        "call_auto_exit_plan_mode",
        trigger_start_time.elapsed().as_millis(),
    );

    // HP-6: Use the unified execute_and_handle_tool_call so that recording and side-effects happen correctly
    let outcome = super::handlers::execute_and_handle_tool_call(
        t_ctx.ctx,
        t_ctx.repeated_tool_attempts,
        t_ctx.turn_modified_files,
        exit_call_id,
        tool_names::EXIT_PLAN_MODE,
        exit_args,
        None,
    )
    .await?;

    Ok(outcome)
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
        ToolExecutionStatus::Failure { error } => {
            Some(serde_json::json!({"error": error.to_string()}).to_string())
        }
        ToolExecutionStatus::Timeout { error } => {
            Some(serde_json::json!({"error": error.message}).to_string())
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
            mcp_event.failure(Some(error.to_string()));
        }
        ToolExecutionStatus::Timeout { error } => {
            mcp_event.failure(Some(error.message.clone()));
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

    #[test]
    fn fallback_from_error_extracts_read_pty_session() {
        let error =
            "Tool failed. Use read_pty_session with session_id=\"run-ab12\" instead of read_file.";
        let fallback = fallback_from_error(tool_names::UNIFIED_FILE, error);
        assert_eq!(
            fallback,
            Some((
                tool_names::READ_PTY_SESSION.to_string(),
                serde_json::json!({"session_id":"run-ab12"}),
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
    }

    #[test]
    fn blocked_or_denied_failure_detects_guardable_errors() {
        assert!(is_blocked_or_denied_failure(
            "task_tracker is a TODO/checklist tool and is not allowed in Plan mode"
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
}
