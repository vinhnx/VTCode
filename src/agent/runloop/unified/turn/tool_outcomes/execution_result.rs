//! Tool execution result handling for turn flow.
//! Agent Legibility:
//! - Entrypoint: this root coordinates tool success and failure shaping through `record_tool_execution`, `emit_turn_metric_log`, and the tool-outcome helpers it calls.
//! - Common changes:
//!   - Structured error presentation and fallback content route through sibling modules in `tool_outcomes/`.
//!   - Auto-mode probe handling and failure-path response shaping now live in `execution_result/` support modules.
//!   - Success-path notification and turn-result shaping still flow through this root and remain queued for further decomposition.
//! - Constraints: TD-005 is active here; preserve the root as a coordinator and prefer new responsibility-named support modules over adding more inline branches.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode inline_events::tests`

mod auto_permission_probe;
mod failure_diagnosis;
mod failure_path;

use anyhow::Result;
use vtcode_commons::ErrorCategory;
#[cfg(test)]
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::core::agent::error_recovery::ErrorType as RecoveryErrorType;
use vtcode_core::notifications::notify_tool_success;
#[cfg(test)]
use vtcode_core::persistent_memory::GroundedFactRecord;
use vtcode_core::tools::error_messages::agent_execution;
use vtcode_core::tools::registry::ToolExecutionError;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::MessageStyle;

use self::auto_permission_probe::push_tool_response_with_auto_permission_probe;
pub(crate) use self::failure_diagnosis::{
    ToolFailureDiagnosis, bounded_diagnostic_field, bounded_error_evidence, bounded_output_evidence,
    deterministic_error_diagnosis, deterministic_output_diagnosis, deterministic_preflight_diagnosis,
    escape_untrusted_evidence, render_and_emit, render_diagnosis,
};
use self::failure_diagnosis::{diagnose_error, diagnose_output};
use self::failure_path::{
    finalize_failed_tool_response, log_structured_failure, notify_structured_failure, record_recovery_tool_error,
};
pub(crate) use super::error_handling::build_error_content;
#[cfg(test)]
use super::error_handling::serialize_json_for_model;
#[cfg(test)]
use super::error_handling::{build_structured_error_content, fallback_from_error};
use super::error_handling::{format_structured_tool_error_for_user, is_blocked_or_denied_failure};
use super::helpers::{check_is_argument_error, serialize_output, signature_key_for};
pub(crate) use super::response_content::compact_model_tool_payload;
use super::response_content::prepare_tool_response_content;
#[cfg(test)]
use super::response_content::{
    maybe_inline_spooled, maybe_inline_spooled_with_preview, tool_output_summary_input_or_serialized,
    truncate_stderr_preview,
};
#[cfg(test)]
use super::subagent_memory::{build_subagent_memory_update, parse_subagent_summary_markdown};
use super::subagent_memory::{merge_subagent_completion_into_memory, record_request_user_input_interview_result};
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext};

pub(crate) fn flush_auto_permission_probe_warning(ctx: &mut TurnProcessingContext<'_>) {
    auto_permission_probe::flush_auto_permission_probe_warning(ctx);
}

fn record_tool_execution(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    start_time: std::time::Instant,
    success: bool,
    is_argument_error: bool,
) {
    let duration = start_time.elapsed();
    ctx.tool_health_tracker.record_execution(tool_name, success, duration);
    if !is_argument_error {
        ctx.autonomous_executor.record_execution(tool_name, success);
    }
    ctx.telemetry.record_tool_usage(tool_name, success);
}

/// Record a failed shell execution for cross-turn identical-failure loop
/// detection. Only shell-command tools with a stable signature and non-empty
/// error text are tracked; anything else leaves the streak slot untouched.
/// Successes simply record nothing here — a later turn without a matching
/// failure resets the streak in `CrossTurnTracker`.
fn record_failed_shell_for_cross_turn_tracking(
    ctx: &mut TurnProcessingContext<'_>,
    tool_name: &str,
    args_val: &serde_json::Value,
    status: &ToolExecutionStatus,
) {
    let Some(signature) = super::handlers::shell_run_signature(tool_name, args_val) else {
        return;
    };
    let Some(error_signature) = super::handlers::shell_failure_error_signature(status) else {
        return;
    };
    ctx.harness_state.record_failed_shell_command(signature, error_signature);
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
        planning_workflow = ctx.is_planning_active(),
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
pub(crate) async fn handle_tool_execution_result<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    tool_start_time: std::time::Instant,
) -> Result<Option<TurnHandlerOutcome>> {
    handle_tool_execution_result_with_spinner(
        t_ctx,
        tool_call_id,
        tool_name,
        args_val,
        pipeline_outcome,
        tool_start_time,
        None,
    )
    .await
}

pub(super) async fn handle_tool_execution_result_with_spinner<'a>(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'a, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    tool_start_time: std::time::Instant,
    batch_spinner: Option<&crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner>,
) -> Result<Option<TurnHandlerOutcome>> {
    // 1. Record metrics and outcome
    let is_success = matches!(&pipeline_outcome.status, ToolExecutionStatus::Success { command_success: true, .. });
    let is_argument_error = if let ToolExecutionStatus::Failure { error } = &pipeline_outcome.status {
        check_is_argument_error(&error.message)
    } else {
        false
    };

    record_tool_execution(t_ctx.ctx, tool_name, tool_start_time, is_success, is_argument_error);
    if pipeline_outcome.status.is_failure_like() {
        t_ctx.ctx.harness_state.record_failed_tool_call();
        record_failed_shell_for_cross_turn_tracking(t_ctx.ctx, tool_name, args_val, &pipeline_outcome.status);
    }

    match &pipeline_outcome.status {
        ToolExecutionStatus::Success { output, command_success, .. } => {
            handle_success(
                t_ctx,
                tool_call_id,
                tool_name,
                args_val,
                pipeline_outcome,
                output,
                *command_success,
                batch_spinner,
            )
            .await?;
        }
        ToolExecutionStatus::Failure { error } => {
            if let Some(outcome) =
                handle_failure(t_ctx, tool_call_id, tool_name, args_val, error, batch_spinner).await?
            {
                return Ok(Some(outcome));
            }
        }
        ToolExecutionStatus::Timeout { error } => {
            handle_timeout(t_ctx, tool_call_id, tool_name, args_val, error, batch_spinner).await?;
        }
        ToolExecutionStatus::Cancelled => {
            handle_cancelled(t_ctx, tool_call_id, tool_name, args_val).await?;
            if t_ctx.ctx.ctrl_c_state.is_exit_requested() {
                return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Exit)));
            }
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
    }

    // 2. Record MCP specific events for non-success outcomes. Successes are
    // recorded by `handle_success_common`; failures/timeouts are not rendered
    // through it, so record them here. Use the shared MCP name parser so
    // canonical `mcp::provider::tool` names are covered (the legacy
    // `mcp_`-only prefix missed them), and label the panel with the same bare
    // tool name the success path uses.
    if !matches!(pipeline_outcome.status, ToolExecutionStatus::Success { .. })
        && let Some(display_name) =
            crate::agent::runloop::unified::tool_summary_helpers::mcp_tool_display_name(tool_name)
    {
        record_mcp_tool_event(t_ctx, display_name, &pipeline_outcome.status);
    }

    if pipeline_outcome.stop_after_tool {
        return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Completed {
            plan_approved_execution_pending: false,
        })));
    }

    // 3. If the tool requested a primary-agent handoff (plan-mode "switch to
    //    build/auto agent" decision), surface it so the turn loop can switch
    //    the active agent after this turn.
    if let Some(agent) = pipeline_outcome.pending_primary_agent.clone() {
        return Ok(Some(TurnHandlerOutcome::SwitchPrimaryAgent(agent)));
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
    command_success: bool,
    batch_spinner: Option<&crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner>,
) -> Result<()> {
    if command_success {
        if let Err(err) = notify_tool_success(tool_name, None).await {
            tracing::debug!(
                tool = %tool_name,
                error = %err,
                "Failed to emit tool success notification"
            );
        }
    }

    // Update blocked-streak and record tool response in grouped context form.
    t_ctx.ctx.reset_blocked_tool_call_streak();
    // This provider-facing history update intentionally precedes UI rendering.
    // Compact rendering may collapse or return early, but it must never affect
    // the model or harness context.
    let content_for_model = prepare_tool_response_content(t_ctx.ctx, tool_name, args_val, output).await;
    let diagnosis = if command_success {
        None
    } else {
        Some(diagnose_output(t_ctx.ctx, tool_name, args_val, output).await)
    };
    let response_result = if let Some(diagnosis) = diagnosis.as_ref() {
        failure_diagnosis::push_tool_response_with_diagnosis(
            t_ctx,
            tool_call_id.clone(),
            tool_name,
            content_for_model,
            diagnosis,
            batch_spinner,
        )
        .await
    } else {
        push_tool_response_with_auto_permission_probe(
            t_ctx,
            tool_call_id.clone(),
            tool_name,
            content_for_model,
            batch_spinner,
        )
        .await
    };
    // The execution pipeline has already emitted the canonical ToolOutput
    // event before this handler runs. Emit the diagnosis immediately after
    // the model-facing response attempt so UI/rendering failures cannot
    // suppress the durable diagnosis item or change its ordering.
    if let Some(diagnosis) = diagnosis.as_ref() {
        render_and_emit(t_ctx.ctx, tool_name, diagnosis);
    }
    response_result?;
    // Skip signature recording for loop-detected stubs: a loop-detected result is
    // a cached/short-circuited response, not a genuine successful execution.
    // Recording its signature would cause the turn-local guard
    // (`has_successful_readonly_signature`) to treat future identical calls as
    // duplicates and return the stale loop-detected stub instead of re-executing.
    let is_loop_detected_stub = output.get("loop_detected").and_then(|v| v.as_bool()).unwrap_or(false);
    let is_mutating = vtcode_core::tools::tool_intent::classify_tool_intent(tool_name, args_val).mutating;
    if command_success && !is_loop_detected_stub {
        if !is_mutating {
            let signature = signature_key_for(tool_name, args_val);
            t_ctx.ctx.harness_state.record_successful_readonly_signature(signature);
        } else {
            // Arm the read-after-write guard for every file this mutation
            // touched (`mutation_target_paths` parses apply_patch payloads for
            // their real targets). Failed or stubbed calls arm nothing: they
            // wrote no content, so there is no diff preview to fall back on
            // and reads must stay available for inspecting the failure.
            for target in vtcode_core::tools::mutation_target_paths(tool_name, args_val) {
                let normalized =
                    crate::agent::runloop::git::normalize_workspace_path(&t_ctx.ctx.config.workspace, &target);
                t_ctx.ctx.harness_state.record_written_file(&normalized.to_string_lossy());
            }
        }
    }
    let mut turn_loop_ctx = t_ctx.ctx.as_turn_loop_context();
    let vt_cfg = turn_loop_ctx.vt_cfg;

    // Handle UI output and file modifications
    let (mod_files, _last_stdout) =
        handle_pipeline_output_from_turn_ctx(&mut turn_loop_ctx, tool_name, args_val, pipeline_outcome, vt_cfg).await?;

    for f in mod_files {
        t_ctx.turn_modified_files.insert(f);
    }
    t_ctx
        .ctx
        .session_stats
        .record_touched_files(t_ctx.turn_modified_files.iter().map(|path| path.display().to_string()));
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
    batch_spinner: Option<&crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner>,
) -> Result<Option<TurnHandlerOutcome>> {
    let error_str = error.message.as_str();
    let (user_msg, hint) = format_structured_tool_error_for_user(tool_name, error);
    notify_structured_failure(tool_name, &user_msg, None).await;

    let is_planning_active_denial = matches!(error.category, ErrorCategory::PlanningPolicyViolation)
        || agent_execution::is_planning_active_denial(error_str);
    let blocked_or_denied_failure = matches!(
        error.category,
        ErrorCategory::InvalidParameters
            | ErrorCategory::PermissionDenied
            | ErrorCategory::PolicyViolation
            | ErrorCategory::PlanningPolicyViolation
    ) || is_blocked_or_denied_failure(error_str);
    log_structured_failure(tool_name, error, hint.as_deref(), "Tool execution failed");

    // `request_user_input` failures caused by a permanent capability/policy
    // denial (e.g. the tool is unavailable in this runtime) must never be
    // retried — unlike a user cancelling the modal, the denial recurs on
    // every attempt. Record it so the planning workflow stops re-forcing the
    // interview instead of looping (checkpoint turn_655/turn_660).
    //
    // IMPORTANT: only fire for genuine policy/permission denials — NOT for
    // `InvalidParameters` (the model passed bad args, a transient issue it
    // can fix on retry). Using the broader `blocked_or_denied_failure` here
    // would permanently disable interviews due to a single malformed call.
    let is_permanent_request_user_input_denial = matches!(
        error.category,
        ErrorCategory::PermissionDenied | ErrorCategory::PolicyViolation | ErrorCategory::PlanningPolicyViolation
    );
    if tool_name == vtcode_core::config::constants::tools::REQUEST_USER_INPUT && is_permanent_request_user_input_denial
    {
        t_ctx.ctx.plan_session.mark_interview_denied();
        if t_ctx.ctx.is_planning_active() {
            t_ctx.ctx.harness_state.arm_interview_denial_recovery();
        }
    }

    if is_planning_active_denial {
        let consecutive_blocked_tool_calls = t_ctx.ctx.harness_state.consecutive_blocked_tool_calls;
        let limits = super::handlers::blocked_tool_call_limits(t_ctx.ctx);
        emit_turn_metric_log(
            t_ctx.ctx,
            "planning_denial",
            tool_name,
            consecutive_blocked_tool_calls,
            limits.consecutive_cap,
        );
    }

    // Record genuine tool errors for recovery diagnostics (skip policy denials)
    if !is_planning_active_denial && !blocked_or_denied_failure {
        record_recovery_tool_error(t_ctx.ctx, tool_name, error, RecoveryErrorType::ToolExecution).await;
    }

    let diagnosis = diagnose_error(t_ctx.ctx, tool_name, args_val, error, "execution").await;
    finalize_failed_tool_response(
        t_ctx,
        tool_call_id,
        tool_name,
        args_val,
        error,
        "execution",
        &diagnosis,
        batch_spinner,
    )
    .await;
    render_and_emit(t_ctx.ctx, tool_name, &diagnosis);

    if blocked_or_denied_failure {
        t_ctx.ctx.harness_state.record_denied_tool_call();
        let streak = t_ctx.ctx.record_blocked_tool_call();
        let limits = super::handlers::blocked_tool_call_limits_for_tool(t_ctx.ctx, tool_name);
        // The interview denial has already armed the bounded tool-free
        // synthesis fallback. Do not let the generic blocked-call fuse turn
        // that recoverable transition into a terminal `Blocked` outcome.
        let interview_denial_recovery = t_ctx.ctx.is_planning_active()
            && tool_name == vtcode_core::config::constants::tools::REQUEST_USER_INPUT
            && is_permanent_request_user_input_denial;
        if let Some(fuse_trip) =
            super::handlers::blocked_tool_call_fuse_trip(streak, t_ctx.ctx.harness_state.blocked_tool_calls, limits)
            && !interview_denial_recovery
        {
            let display_tool = tool_action_label(tool_name, args_val);
            let recovery_active = t_ctx.ctx.is_recovery_active();
            let blocked_total = t_ctx.ctx.harness_state.blocked_tool_calls;
            // Captured before arming/breaking so `finalize_turn` can populate
            // the `TurnBlockedEvent` fields instead of `None`/zeros.
            let telemetry = crate::agent::runloop::unified::run_loop_context::BlockedToolRecoveryTelemetry {
                last_tool: display_tool.to_string(),
                consecutive_cap: limits.consecutive_cap,
                total_cap: limits.total_cap,
                blocked_streak: streak,
                blocked_total,
            };
            let (block_reason, _) = super::handlers::blocked_tool_call_messages_detailed(
                fuse_trip,
                recovery_active,
                &display_tool,
                streak,
                blocked_total,
                tool_name,
            );
            emit_turn_metric_log(t_ctx.ctx, fuse_trip.metric(), tool_name, streak, fuse_trip.cap());
            if !recovery_active {
                t_ctx.ctx.harness_state.arm_blocked_tool_recovery(block_reason, telemetry);
                return Ok(Some(TurnHandlerOutcome::Continue));
            }
            t_ctx.ctx.harness_state.record_blocked_tool_recovery_telemetry(telemetry);
            t_ctx.ctx.push_system_message(block_reason.clone());
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(block_reason) })));
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
    batch_spinner: Option<&crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner>,
) -> Result<()> {
    let (user_msg, _) = format_structured_tool_error_for_user(tool_name, error);
    notify_structured_failure(tool_name, &user_msg, Some("timeout")).await;
    log_structured_failure(tool_name, error, None, "Tool timed out");

    record_recovery_tool_error(t_ctx.ctx, tool_name, error, RecoveryErrorType::Timeout).await;

    let diagnosis = diagnose_error(t_ctx.ctx, tool_name, args_val, error, "timeout").await;
    finalize_failed_tool_response(
        t_ctx,
        tool_call_id,
        tool_name,
        args_val,
        error,
        "timeout",
        &diagnosis,
        batch_spinner,
    )
    .await;
    render_and_emit(t_ctx.ctx, tool_name, &diagnosis);

    Ok(())
}

async fn handle_cancelled(
    t_ctx: &mut super::handlers::ToolOutcomeContext<'_, '_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
) -> Result<()> {
    let display_tool = tool_action_label(tool_name, args_val);
    let error_msg = format!("Tool '{display_tool}' execution cancelled");
    t_ctx.ctx.renderer.line(MessageStyle::Info, &error_msg)?;

    let error_content = serde_json::json!({"error": error_msg});
    push_tool_response_with_auto_permission_probe(t_ctx, tool_call_id, tool_name, error_content.to_string(), None)
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
                crate::agent::runloop::unified::turn::utils::render_hook_messages(ctx.renderer, &outcome.messages)?;
                for context in outcome.additional_context {
                    if !context.trim().is_empty() {
                        ctx.push_system_message(context);
                    }
                }
            }
            Err(err) => {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("Failed to run post-tool hooks: {err}"))?;
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
        ToolExecutionStatus::Cancelled => Some(serde_json::json!({"error": "Cancelled"}).to_string()),
    };

    let mut mcp_event = mcp_events::McpEvent::new("mcp".to_string(), tool_name.to_string(), data_preview);

    match status {
        ToolExecutionStatus::Success { command_success, .. } if *command_success => {
            mcp_event.success(None);
        }
        ToolExecutionStatus::Success { .. } => {
            mcp_event.failure(Some("Command returned a non-zero exit code".to_string()));
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

#[cfg(test)]
mod blocked_fuse_tests {
    use super::super::handlers::{
        BlockedToolCallFuseTrip, BlockedToolCallLimits, blocked_tool_call_fuse_trip,
        blocked_tool_call_messages_detailed,
    };

    fn limits(consecutive_cap: usize, total_cap: usize) -> BlockedToolCallLimits {
        BlockedToolCallLimits { consecutive_cap, total_cap }
    }

    #[test]
    fn denied_execution_result_trips_normal_total_cap() {
        assert_eq!(blocked_tool_call_fuse_trip(1, 9, limits(4, 8)), Some(BlockedToolCallFuseTrip::Total { cap: 8 }));
    }

    #[test]
    fn denied_execution_result_allows_planning_total_cap_until_four_times_consecutive() {
        assert_eq!(blocked_tool_call_fuse_trip(1, 16, limits(4, 16)), None);
        assert_eq!(blocked_tool_call_fuse_trip(1, 17, limits(4, 16)), Some(BlockedToolCallFuseTrip::Total { cap: 16 }));
    }

    #[test]
    fn denied_execution_result_trips_consecutive_cap_early() {
        assert_eq!(
            blocked_tool_call_fuse_trip(5, 5, limits(4, 8)),
            Some(BlockedToolCallFuseTrip::Consecutive { cap: 4 })
        );
    }

    #[test]
    fn denied_execution_result_prefers_total_cap_when_both_fuses_trip() {
        assert_eq!(blocked_tool_call_fuse_trip(5, 9, limits(4, 8)), Some(BlockedToolCallFuseTrip::Total { cap: 8 }));
    }

    #[test]
    fn denied_execution_result_diagnostic_uses_actual_total_cap() {
        let trip = blocked_tool_call_fuse_trip(1, 29, limits(7, 28)).expect("total fuse should trip");
        assert_eq!(trip, BlockedToolCallFuseTrip::Total { cap: 28 });
    }

    #[test]
    fn denied_execution_result_uses_shared_total_fuse_message() {
        let (reason, _) = blocked_tool_call_messages_detailed(
            BlockedToolCallFuseTrip::Total { cap: 28 },
            false,
            "read_file 'src/main.rs'",
            1,
            29,
            "exec_command",
        );
        assert!(reason.contains("28 total blocked calls"));
    }
}
