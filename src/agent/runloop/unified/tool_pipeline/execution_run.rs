use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::ToolCatalog;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::exec::events::ToolCallStatus;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::tools::ToolInvocationId;
use vtcode_core::tools::command_args;
use vtcode_core::tools::registry::{ExecSettlementMode, ToolExecutionError};
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::run_loop_context::{RunLoopContext, full_auto_loop_grants_enabled};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_call_safety::invocation_id_from_call_id;
use crate::agent::runloop::unified::tool_routing::{
    PreToolHookPhaseResult, ToolPermissionFlow, ensure_tool_permission_with_call_id,
};

use super::execute_hitl_tool;
use super::execution_events::{
    emit_tool_completion_for_status, emit_tool_completion_status, emit_tool_outcome_observation,
    emit_tool_start_if_needed,
};
use super::execution_runtime::execute_with_cache_and_streaming;
use super::file_conflict_prompt::resolve_file_conflict_status;
use super::status::{ToolExecutionStatus, ToolPipelineOutcome};
use super::validation::{SafetyValidationFailure, validate_tool_call_with_limit_prompt};
use crate::agent::runloop::unified::planning_workflow::handle_start_planning;
use vtcode_commons::canonicalize;

pub(crate) fn resolve_harness_item_identity(tool_item_id: &str) -> (ToolInvocationId, String) {
    match ToolInvocationId::parse(tool_item_id) {
        Ok(invocation_id) => (invocation_id, tool_item_id.to_string()),
        Err(_) => {
            let invocation_id = invocation_id_from_call_id(tool_item_id);
            (invocation_id, format!("{tool_item_id}:{}", invocation_id.short()))
        }
    }
}

fn structured_failure_from_message(tool_name: &str, message: impl Into<String>) -> ToolExecutionError {
    let message = message.into();
    ToolExecutionError::from_anyhow(tool_name, &anyhow!(message), 0, false, false, Some("unified_runloop"))
}

fn structured_failure(tool_name: &str, error: &anyhow::Error) -> ToolExecutionError {
    ToolExecutionError::from_anyhow(tool_name, error, 0, false, false, Some("unified_runloop"))
}

fn excludes_wait_from_turn_clock(tool_name: &str, args: &Value) -> bool {
    tool_name == tools::REQUEST_USER_INPUT
        || (tool_intent::canonical_command_session_tool_name(tool_name)
            .is_some_and(|canonical| canonical == tools::UNIFIED_EXEC)
            && tool_intent::command_session_action_is(args, "wait"))
}

#[cfg_attr(feature = "profiling", hotpath::measure)]
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // pipeline entry point, all params needed
pub(crate) async fn run_tool_call(
    ctx: &mut RunLoopContext<'_>,
    call: &vtcode_core::llm::provider::ToolCall,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    lifecycle_hooks: Option<&LifecycleHookEngine>,
    skip_confirmations: bool,
    vt_cfg: Option<&VTCodeConfig>,
    turn_index: usize,
    prevalidated: bool,
) -> Result<ToolPipelineOutcome, anyhow::Error> {
    let requested_name = call.tool_name().unwrap_or(call.call_type.as_str());
    if call.function.is_none() {
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: structured_failure_from_message("tool", "Tool call missing function"),
        });
        emit_tool_outcome_observation(ctx.harness_emitter, requested_name, &outcome);
        return Ok(outcome);
    }

    let args_val = match call.execution_arguments() {
        Ok(args) => args,
        Err(err) => {
            let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
                error: structured_failure("tool", &anyhow!(err)),
            });
            emit_tool_outcome_observation(ctx.harness_emitter, requested_name, &outcome);
            return Ok(outcome);
        }
    };

    run_tool_call_with_args(
        ctx,
        call.id.clone(),
        requested_name,
        &args_val,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        lifecycle_hooks,
        skip_confirmations,
        vt_cfg,
        turn_index,
        prevalidated,
    )
    .await
}

#[cfg_attr(feature = "profiling", hotpath::measure)]
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // pipeline entry point, all params needed
pub(crate) async fn run_tool_call_with_args(
    ctx: &mut RunLoopContext<'_>,
    tool_item_id: String,
    requested_name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    lifecycle_hooks: Option<&LifecycleHookEngine>,
    skip_confirmations: bool,
    vt_cfg: Option<&VTCodeConfig>,
    turn_index: usize,
    prevalidated: bool,
) -> Result<ToolPipelineOutcome, anyhow::Error> {
    ctx.handle
        .set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::CheckingPermissions);
    let invocation_started_at = std::time::Instant::now();
    let mut effective_args = std::borrow::Cow::Borrowed(args_val);
    let mut canonical_name = None;
    let tool_call_id = tool_item_id.as_str();
    let (safety_invocation_id, fallback_harness_item_id) = resolve_harness_item_identity(&tool_item_id);

    if !prevalidated {
        // Control-plane exec calls (wait/inspect) bypass the per-turn budget
        // exhaustion rejection so a long build stays observable; the safety
        // gateway still bounds them via its own control-plane cap.
        if !tool_intent::is_turn_budget_exempt_call(requested_name, args_val)
            && let Some(exhaustion) = ctx.harness_state.tool_budget_exhaustion()
        {
            let mut outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
                error: structured_failure_from_message(requested_name, exhaustion.policy_violation_message()),
            });
            outcome.total_duration = invocation_started_at.elapsed();
            emit_tool_outcome_observation(ctx.harness_emitter, requested_name, &outcome);
            return Ok(outcome);
        }

        match ctx.tool_registry.admit_public_tool_call(requested_name, args_val) {
            Ok(prepared) => {
                canonical_name = Some(prepared.canonical_name);
                effective_args = std::borrow::Cow::Owned(prepared.effective_args);
            }
            Err(err) => {
                let mut outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
                    error: structured_failure(requested_name, &anyhow!("Tool argument validation failed: {err}")),
                });
                outcome.total_duration = invocation_started_at.elapsed();
                emit_tool_outcome_observation(ctx.harness_emitter, requested_name, &outcome);
                return Ok(outcome);
            }
        }
    } else if let Some(tool) = ctx.tool_registry.get_tool(requested_name) {
        canonical_name = Some(match ctx.tool_registry.resolve_tool_name(requested_name) {
            Ok(resolved_name) => resolved_name,
            Err(_) => tool.name().to_string(),
        });
    }
    let name = canonical_name.as_deref().unwrap_or(requested_name);

    let harness_emitter = ctx.harness_emitter;
    let streamed_harness_item_id = ctx
        .harness_state
        .take_streamed_tool_call_item_id(tool_call_id)
        .map(|streamed| streamed.item_id);
    let already_started = streamed_harness_item_id.is_some();
    let harness_item_id = streamed_harness_item_id.unwrap_or(fallback_harness_item_id);
    let tool_started_emitted = emit_tool_start_if_needed(
        harness_emitter,
        already_started,
        &harness_item_id,
        tool_call_id,
        name,
        effective_args.as_ref(),
    );
    let max_tool_retries = ctx.harness_state.max_tool_retries as usize;
    let finish_with_status = |status: ToolExecutionStatus, tool_execution_started: bool, args: &Value| {
        let mut outcome = ToolPipelineOutcome::from_status(status);
        if outcome.total_duration.is_zero() {
            outcome.total_duration = invocation_started_at.elapsed();
        }
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            tool_execution_started,
            &harness_item_id,
            tool_call_id,
            name,
            args,
            &outcome.status,
        );
        emit_tool_outcome_observation(harness_emitter, name, &outcome);
        outcome
    };

    if !prevalidated {
        // PreToolUse hooks run before the safety gateway so that rewritten
        // arguments (e.g. hook-wrapped commands) are what every downstream
        // check — safety, policy, permissions — evaluates and approves.
        let hook_phase = crate::agent::runloop::unified::tool_routing::pipeline_pre_tool_hooks(
            lifecycle_hooks,
            ctx.renderer,
            name,
            effective_args.as_ref(),
            tool_call_id,
        )
        .await;
        let hook_phase = match hook_phase {
            Ok(Some(PreToolHookPhaseResult::Deny)) => {
                ctx.harness_state.record_denied_tool_call();
                return Ok(finish_with_status(
                    ToolExecutionStatus::Failure {
                        error: structured_failure_from_message(name, "Tool permission denied"),
                    },
                    false,
                    effective_args.as_ref(),
                ));
            }
            Ok(Some(PreToolHookPhaseResult::Proceed { rewritten_args: Some(rewritten), requires_prompt })) => {
                // Re-validate the rewritten arguments: preflight ran on the
                // original payload, so a rewrite could otherwise bypass schema
                // checks. A hook-produced invalid payload is a hook error —
                // block instead of executing malformed input.
                if let Err(err) = ctx.tool_registry.preflight_validate_harness_call(name, &rewritten) {
                    return Ok(finish_with_status(
                        ToolExecutionStatus::Failure {
                            error: structured_failure(
                                name,
                                &anyhow!("PreToolUse hook produced invalid arguments: {err}"),
                            ),
                        },
                        false,
                        effective_args.as_ref(),
                    ));
                }
                effective_args = std::borrow::Cow::Owned(rewritten);
                // Forward the phase with the rewrite stripped so the
                // permission flow neither re-runs hooks (double invocation,
                // double rewrite) nor loses the Ask decision. The rewritten
                // args are already reflected in effective_args below.
                Some(PreToolHookPhaseResult::Proceed { rewritten_args: None, requires_prompt })
            }
            Ok(phase) => phase,
            Err(err) => {
                return Ok(finish_with_status(
                    ToolExecutionStatus::Failure { error: structured_failure(name, &err) },
                    false,
                    effective_args.as_ref(),
                ));
            }
        };

        let safety_approval_justification = match check_tool_safety(
            ctx,
            name,
            effective_args.as_ref(),
            safety_invocation_id,
            ctrl_c_state,
            ctrl_c_notify,
            full_auto_loop_grants_enabled(ctx.full_auto, vt_cfg),
        )
        .await
        {
            Ok(justification) => justification,
            Err(safety_failure) => {
                return Ok(finish_with_status(*safety_failure, false, effective_args.as_ref()));
            }
        };

        match check_tool_permission(
            ctx,
            tool_call_id,
            name,
            effective_args.as_ref(),
            ctrl_c_state,
            ctrl_c_notify,
            default_placeholder,
            lifecycle_hooks,
            skip_confirmations,
            vt_cfg,
            safety_approval_justification.as_deref(),
            hook_phase,
        )
        .await
        {
            Ok(Some(updated_args)) => {
                // A PermissionRequest hook may supply its own rewrite via
                // `updated_input`; it replaces the arguments the safety
                // gateway, policy checks, approvals, and argument-dependent
                // guards evaluated. Validate the schema and re-run the safety
                // gateway against the final arguments so the replacement does
                // not execute under decisions made for earlier arguments.
                if let Err(err) = ctx.tool_registry.preflight_validate_harness_call(name, &updated_args) {
                    return Ok(finish_with_status(
                        ToolExecutionStatus::Failure {
                            error: structured_failure(
                                name,
                                &anyhow!("PermissionRequest hook produced invalid arguments: {err}"),
                            ),
                        },
                        false,
                        effective_args.as_ref(),
                    ));
                }
                let rewritten_differ = effective_args.as_ref() != &updated_args;
                effective_args = std::borrow::Cow::Owned(updated_args);
                if rewritten_differ
                    && let Err(safety_failure) = check_tool_safety(
                        ctx,
                        name,
                        effective_args.as_ref(),
                        safety_invocation_id,
                        ctrl_c_state,
                        ctrl_c_notify,
                        full_auto_loop_grants_enabled(ctx.full_auto, vt_cfg),
                    )
                    .await
                {
                    return Ok(finish_with_status(*safety_failure, false, effective_args.as_ref()));
                }
            }
            Ok(None) => {}
            Err(permission_failure) => {
                return Ok(finish_with_status(*permission_failure, false, effective_args.as_ref()));
            }
        }

        // Control-plane exec calls (wait/inspect) do not consume the per-turn
        // tool-call budget; see `record_tool_call_budget_usage` for rationale.
        if !tool_intent::is_turn_budget_exempt_call(name, effective_args.as_ref())
            && let Some(warning) = ctx.harness_state.record_tool_call_with_default_warning()
        {
            warning.log_threshold_reached("Tool-call budget warning threshold reached in tool pipeline path");
        }
    }

    // Safety gateway admission is always completed before registry execution
    // in the unified runloop: the tool-outcome handlers (validate_tool_call)
    // and the copilot runtime validate every call through the shared gateway
    // before invoking this pipeline with prevalidated == true, and the
    // non-prevalidated branch above runs check_tool_safety itself. Passing
    // the negation of prevalidated here made the registry re-admit every
    // interactive call on the SAME shared gateway, doubling the counter and
    // halving the effective per-turn tool budget (checkpoint turn_942/943:
    // 16 admitted calls then "Per-turn tool limit reached (max: 32)" with
    // max_tool_calls_per_turn = 32). The registry only performs its own
    // check when neither the caller nor this pipeline has run it.
    let safety_prevalidated = prevalidated || ctx.safety_validator.is_some();

    let request_user_input_enabled = FeatureSet::from_config(vt_cfg)
        .request_user_input_enabled(ctx.tool_registry.is_planning_active(), ctx.renderer.supports_inline_ui())
        // Also reject if the interview was permanently denied this session
        // (prevents a hallucinated tool call from reaching the HITL path).
        && !ctx.plan_session.is_interview_denied();
    let excludes_hitl_wait = name == tools::REQUEST_USER_INPUT;
    if excludes_hitl_wait {
        ctx.harness_state.begin_budget_excluded_wait();
    }
    if excludes_hitl_wait {
        ctx.handle
            .set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::WaitingForApproval);
    }
    let hitl_result = execute_hitl_tool(
        name,
        ctx.handle,
        ctx.session,
        effective_args.as_ref(),
        ctrl_c_state,
        ctrl_c_notify,
        request_user_input_enabled,
    )
    .await;
    if excludes_hitl_wait {
        ctx.harness_state.end_budget_excluded_wait();
    }
    if let Some(hitl_result) = hitl_result {
        let status = match hitl_result {
            Ok(value) => ToolExecutionStatus::Success {
                output: value,
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
            Err(error) => ToolExecutionStatus::Failure { error: structured_failure(name, &error) },
        };
        return Ok(finish_with_status(status, true, effective_args.as_ref()));
    }

    if let Some(mut outcome) = handle_start_planning(
        ctx,
        name,
        effective_args.as_ref(),
        ctrl_c_state,
        ctrl_c_notify,
        max_tool_retries,
        prevalidated,
    )
    .await
    {
        if outcome.total_duration.is_zero() {
            outcome.total_duration = invocation_started_at.elapsed();
        }
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            true,
            &harness_item_id,
            tool_call_id,
            name,
            effective_args.as_ref(),
            &outcome.status,
        );
        emit_tool_outcome_observation(harness_emitter, name, &outcome);
        return Ok(outcome);
    }
    let budget_excluded_wait = excludes_wait_from_turn_clock(name, effective_args.as_ref());
    if budget_excluded_wait {
        ctx.harness_state.begin_budget_excluded_wait();
    }
    let show_live_pty_preview = !ctx.renderer.supports_inline_ui() || !ctx.renderer.is_compact_display();
    ctx.handle
        .set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::RunningTools);
    let execution = execute_with_cache_and_streaming(
        ctx.tool_registry,
        ctx.tool_result_cache,
        name,
        &harness_item_id,
        tool_call_id,
        effective_args.as_ref(),
        ctrl_c_state,
        ctrl_c_notify,
        ctx.handle,
        harness_emitter.cloned(),
        vt_cfg,
        max_tool_retries,
        exec_settlement_mode_for_tool_call(prevalidated, name, effective_args.as_ref()),
        safety_prevalidated,
        show_live_pty_preview,
    )
    .await;
    if budget_excluded_wait {
        ctx.harness_state.end_budget_excluded_wait();
    }
    let execution_status = match resolve_file_conflict_status(
        ctx.tool_registry,
        ctx.tool_result_cache,
        ctx.session,
        ctx.handle,
        name,
        &harness_item_id,
        tool_call_id,
        effective_args.as_ref(),
        execution,
        ctrl_c_state,
        ctrl_c_notify,
        harness_emitter.cloned(),
        vt_cfg,
        max_tool_retries,
        safety_prevalidated,
        show_live_pty_preview,
    )
    .await
    {
        Ok(status) => status,
        Err(error) => {
            let mut outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
                error: structured_failure(name, &error),
            });
            outcome.total_duration = invocation_started_at.elapsed();
            emit_tool_completion_for_status(
                harness_emitter,
                tool_started_emitted,
                true,
                &harness_item_id,
                tool_call_id,
                name,
                effective_args.as_ref(),
                &outcome.status,
            );
            emit_tool_outcome_observation(harness_emitter, name, &outcome);
            return Err(error);
        }
    };

    let mut pipeline_outcome = ToolPipelineOutcome::from_status(execution_status);
    if pipeline_outcome.total_duration.is_zero() {
        pipeline_outcome.total_duration = invocation_started_at.elapsed();
    }
    if let Err(error) = apply_post_execution_side_effects(
        ctx,
        &harness_item_id,
        tool_call_id,
        name,
        effective_args.as_ref(),
        turn_index,
        skip_confirmations,
        harness_emitter,
        tool_started_emitted,
        &mut pipeline_outcome,
    )
    .await
    {
        let mut outcome =
            ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure { error: structured_failure(name, &error) });
        outcome.total_duration = invocation_started_at.elapsed();
        emit_tool_outcome_observation(harness_emitter, name, &outcome);
        return Err(error);
    }

    emit_tool_completion_for_status(
        harness_emitter,
        tool_started_emitted,
        true,
        &harness_item_id,
        tool_call_id,
        name,
        effective_args.as_ref(),
        &pipeline_outcome.status,
    );
    emit_tool_outcome_observation(harness_emitter, name, &pipeline_outcome);
    Ok(pipeline_outcome)
}

pub(crate) fn exec_settlement_mode_for_tool_call(prevalidated: bool, name: &str, args: &Value) -> ExecSettlementMode {
    if !prevalidated || name != tools::UNIFIED_EXEC {
        return ExecSettlementMode::Manual;
    }

    let Some(action) = tool_intent::command_session_action(args) else {
        return ExecSettlementMode::Manual;
    };

    if action.eq_ignore_ascii_case("run") {
        return if !args.get("tty").and_then(Value::as_bool).unwrap_or(false) {
            ExecSettlementMode::SettleNonInteractive
        } else {
            ExecSettlementMode::Manual
        };
    }

    if action.eq_ignore_ascii_case("poll") {
        return ExecSettlementMode::SettleNonInteractive;
    }

    if action.eq_ignore_ascii_case("continue") && command_args::interactive_input_text(args).is_none() {
        ExecSettlementMode::SettleNonInteractive
    } else {
        ExecSettlementMode::Manual
    }
}

async fn check_tool_safety(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &Value,
    invocation_id: ToolInvocationId,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    auto_grant: bool,
) -> Result<Option<String>, Box<ToolExecutionStatus>> {
    let Some(safety_validator) = ctx.safety_validator else {
        return Ok(None);
    };

    match validate_tool_call_with_limit_prompt(
        safety_validator,
        ctx.handle,
        ctx.session,
        ctrl_c_state,
        ctrl_c_notify,
        name,
        args_val,
        invocation_id,
        Some(ctx.harness_state),
        ctx.harness_emitter,
        ctx.agent_name.as_deref(),
        auto_grant,
        ctx.traj,
        ctx.tool_registry.is_planning_active(),
    )
    .await
    {
        Ok(()) => Ok(None),
        Err(SafetyValidationFailure::SessionLimitNotIncreased) => Err(Box::new(ToolExecutionStatus::Failure {
            error: structured_failure_from_message(name, "Session tool limit reached and not increased by user"),
        })),
        Err(SafetyValidationFailure::SessionLimitPromptFailed(error)) => Err(Box::new(ToolExecutionStatus::Failure {
            error: structured_failure(name, &anyhow!("Failed while requesting a session tool-limit increase: {error}")),
        })),
        Err(SafetyValidationFailure::NeedsApproval(justification)) => Ok(Some(justification)),
        Err(SafetyValidationFailure::Validation(error)) => Err(Box::new(ToolExecutionStatus::Failure {
            error: structured_failure(name, &anyhow!("Safety validation failed: {error}")),
        })),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // internal pipeline function, all params needed
async fn check_tool_permission(
    ctx: &mut RunLoopContext<'_>,
    tool_call_id: &str,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    lifecycle_hooks: Option<&LifecycleHookEngine>,
    skip_confirmations: bool,
    vt_cfg: Option<&VTCodeConfig>,
    safety_approval_justification: Option<&str>,
    hook_phase: Option<PreToolHookPhaseResult>,
) -> Result<Option<Value>, Box<ToolExecutionStatus>> {
    // PreToolUse hooks already ran upstream (see run_tool_call_with_args), so
    // the permission flow consumes the forwarded phase result instead of
    // running them again. The engine is still passed through for the later
    // PermissionRequest hook phase.
    let permissions_ctx = build_tool_permissions_context(
        ctx,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        lifecycle_hooks,
        skip_confirmations,
        vt_cfg,
        safety_approval_justification,
    );

    match ensure_tool_permission_with_call_id(permissions_ctx, name, Some(args_val), Some(tool_call_id), hook_phase)
        .await
    {
        Ok(ToolPermissionFlow::Approved { updated_args }) => Ok(updated_args),
        Ok(ToolPermissionFlow::Denied) => Err(Box::new(ToolExecutionStatus::Failure {
            error: structured_failure_from_message(name, "Tool permission denied"),
        })),
        Ok(ToolPermissionFlow::Blocked { reason }) => Err(Box::new(ToolExecutionStatus::Failure {
            error: structured_failure_from_message(name, reason),
        })),
        Ok(ToolPermissionFlow::Interrupted | ToolPermissionFlow::Exit) => Err(Box::new(ToolExecutionStatus::Cancelled)),
        Err(error) => Err(Box::new(ToolExecutionStatus::Failure { error: structured_failure(name, &error) })),
    }
}

fn build_tool_permissions_context<'a>(
    ctx: &'a mut RunLoopContext<'_>,
    ctrl_c_state: &'a Arc<CtrlCState>,
    ctrl_c_notify: &'a Arc<Notify>,
    default_placeholder: Option<String>,
    lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    skip_confirmations: bool,
    vt_cfg: Option<&'a VTCodeConfig>,
    safety_approval_justification: Option<&str>,
) -> crate::agent::runloop::unified::tool_routing::ToolPermissionsContext<'a, vtcode_ui::tui::app::InlineSession> {
    let auto_permission_runtime = ctx.auto_permission.as_mut().map(|auto_permission| {
        crate::agent::runloop::unified::run_loop_context::AutoPermissionRuntimeContext {
            config: auto_permission.config,
            vt_cfg,
            provider_client: &mut *auto_permission.provider_client,
            working_history: auto_permission.working_history,
        }
    });

    crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
        tool_registry: ctx.tool_registry,
        renderer: ctx.renderer,
        handle: ctx.handle,
        session: ctx.session,
        active_thread_label: None,
        default_placeholder,
        ctrl_c_state,
        ctrl_c_notify,
        hooks: lifecycle_hooks,
        justification: None,
        approval_recorder: Some(ctx.approval_recorder),
        decision_ledger: Some(ctx.decision_ledger),
        tool_permission_cache: Some(ctx.tool_permission_cache),
        permissions_state: Some(ctx.permissions_state),
        active_agent_permissions: ctx
            .active_agent_permissions
            .or_else(|| vt_cfg.and_then(|cfg| cfg.runtime_agent_permissions.as_ref())),
        hitl_notification_bell: vt_cfg.map(|cfg| cfg.security.hitl_notification_bell).unwrap_or(true),
        approval_policy: vt_cfg
            .map(|cfg| cfg.security.human_in_the_loop)
            .map(approval_policy_from_human_in_the_loop)
            .unwrap_or(vtcode_core::exec_policy::AskForApproval::OnRequest),
        skip_confirmations,
        permissions_config: vt_cfg.map(|cfg| &cfg.permissions),
        auto_permission_runtime,
        session_stats: Some(ctx.session_stats),
        safety_approval_justification: safety_approval_justification.map(String::from),
        harness_emitter: ctx.harness_emitter,
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // internal pipeline function, all params needed
async fn apply_post_execution_side_effects(
    ctx: &mut RunLoopContext<'_>,
    tool_item_id: &str,
    tool_call_id: &str,
    name: &str,
    args_val: &Value,
    turn_index: usize,
    skip_confirmations: bool,
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    pipeline_outcome: &mut ToolPipelineOutcome,
) -> Result<(), anyhow::Error> {
    if !pipeline_outcome.modified_files().is_empty() {
        let modified_files = pipeline_outcome.modified_files().to_vec();
        let keep_changes = match confirm_changes_with_git_diff(&modified_files, skip_confirmations).await {
            Ok(value) => value,
            Err(error) => {
                emit_tool_completion_status(
                    harness_emitter,
                    tool_started_emitted,
                    true,
                    tool_item_id,
                    tool_call_id,
                    name,
                    args_val,
                    ToolCallStatus::Failed,
                    None,
                    None,
                    error.to_string(),
                );
                return Err(error);
            }
        };

        if keep_changes {
            ctx.traj.log_tool_call(
                turn_index,
                name,
                args_val,
                pipeline_outcome.command_success,
                ctx.agent_name.as_deref(),
                ctx.is_subagent,
            );
            if pipeline_outcome.command_success {
                let invalidation_paths =
                    cache_invalidation_paths(ctx.tool_registry.workspace_root(), pipeline_outcome.modified_files());
                let mut cache = ctx.tool_result_cache.write().await;
                cache.invalidate_for_paths(&invalidation_paths);
            }
        } else {
            if let Some(files) = pipeline_outcome.modified_files_mut() {
                files.clear();
            }
            pipeline_outcome.set_command_success(false);
        }
    } else {
        ctx.traj.log_tool_call(
            turn_index,
            name,
            args_val,
            pipeline_outcome.command_success,
            ctx.agent_name.as_deref(),
            ctx.is_subagent,
        );
    }

    Ok(())
}

fn cache_invalidation_paths(workspace_root: &Path, changed_paths: &[String]) -> Vec<String> {
    let workspace_root = canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    changed_paths
        .iter()
        .map(Path::new)
        .map(|path| {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                workspace_root.join(path)
            };
            canonicalize(&absolute).unwrap_or(absolute)
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        cache_invalidation_paths, excludes_wait_from_turn_clock, exec_settlement_mode_for_tool_call,
        resolve_harness_item_identity,
    };
    use serde_json::json;
    use vtcode_commons::canonicalize;
    use vtcode_core::tools::registry::ExecSettlementMode;
    use vtcode_core::{config::constants::tools, tools::ToolInvocationId};

    #[test]
    fn cache_invalidation_paths_resolve_relative_edits_against_workspace() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let source = workspace.path().join("src");
        std::fs::create_dir(&source).expect("source directory");
        let changed = source.join("widget.rs");
        std::fs::write(&changed, "struct Widget;\n").expect("fixture source");

        let paths = cache_invalidation_paths(workspace.path(), &["src/widget.rs".to_string()]);

        assert_eq!(paths, vec![canonicalize(changed).expect("canonical source").to_string_lossy()]);
    }

    #[test]
    fn settles_prevalidated_noninteractive_run() {
        assert_eq!(
            exec_settlement_mode_for_tool_call(
                true,
                tools::UNIFIED_EXEC,
                &json!({"action": "run", "command": "cargo check"})
            ),
            ExecSettlementMode::SettleNonInteractive
        );
    }

    #[test]
    fn skips_interactive_or_non_prevalidated_exec_calls() {
        assert_eq!(
            exec_settlement_mode_for_tool_call(
                false,
                tools::UNIFIED_EXEC,
                &json!({"action": "run", "command": "cargo check"})
            ),
            ExecSettlementMode::Manual
        );
        assert_eq!(
            exec_settlement_mode_for_tool_call(
                true,
                tools::UNIFIED_EXEC,
                &json!({"action": "run", "command": "cargo check", "tty": true})
            ),
            ExecSettlementMode::Manual
        );
        assert_eq!(
            exec_settlement_mode_for_tool_call(
                true,
                tools::UNIFIED_EXEC,
                &json!({"action": "continue", "session_id": "run-1", "input": "y"})
            ),
            ExecSettlementMode::Manual
        );
    }

    #[test]
    fn excludes_user_interview_and_command_wait_from_turn_clock() {
        assert!(excludes_wait_from_turn_clock(tools::REQUEST_USER_INPUT, &json!({})));
        assert!(excludes_wait_from_turn_clock(
            tools::UNIFIED_EXEC,
            &json!({"action": "wait", "session_id": "run-1"})
        ));
        assert!(!excludes_wait_from_turn_clock(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "cargo check"})
        ));
    }

    #[test]
    fn resolve_harness_item_identity_suffixes_non_uuid_ids() {
        let (invocation_id, harness_id) = resolve_harness_item_identity("tool_call_0");

        assert!(harness_id.starts_with("tool_call_0:"));
        assert!(harness_id.ends_with(invocation_id.short().as_str()));
    }

    #[test]
    fn resolve_harness_item_identity_preserves_uuid_ids() {
        let invocation_id = ToolInvocationId::new();
        let raw_id = invocation_id.to_string();

        let (resolved, harness_id) = resolve_harness_item_identity(&raw_id);

        assert_eq!(resolved, invocation_id);
        assert_eq!(harness_id, raw_id);
    }
}
