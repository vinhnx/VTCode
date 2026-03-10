use std::sync::Arc;

use anyhow::anyhow;
use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::exec::events::ToolCallStatus;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::tools::ToolInvocationId;

use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_started_event,
};
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_routing::ToolPermissionFlow;
use crate::agent::runloop::unified::tool_routing::ensure_tool_permission;

use super::execute_hitl_tool;
use super::execution_events::{emit_tool_completion_for_status, emit_tool_completion_status};
use super::execution_plan_mode::{handle_enter_plan_mode, handle_exit_plan_mode};
use super::execution_runtime::execute_with_cache_and_streaming;
use super::file_conflict_prompt::resolve_file_conflict_status;
use super::status::{ToolExecutionStatus, ToolPipelineOutcome};

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
    let function = match call.function.as_ref() {
        Some(func) => func,
        None => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow!("Tool call missing function"),
                },
            ));
        }
    };

    let requested_name = function.name.as_str();
    let args_val = match call.parsed_arguments() {
        Ok(args) => args,
        Err(err) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow!(err),
                },
            ));
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
    let mut canonical_name = requested_name.to_string();

    if !prevalidated {
        if ctx.harness_state.tool_budget_exhausted() {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow::anyhow!(
                        "Policy violation: exceeded max tool calls per turn ({})",
                        ctx.harness_state.max_tool_calls
                    ),
                },
            ));
        }

        match ctx
            .tool_registry
            .preflight_validate_call(requested_name, args_val)
        {
            Ok(preflight) => canonical_name = preflight.normalized_tool_name,
            Err(err) => {
                return Ok(ToolPipelineOutcome::from_status(
                    ToolExecutionStatus::Failure {
                        error: anyhow!("Tool argument validation failed: {}", err),
                    },
                ));
            }
        }

        if let Some(safety_validator) = ctx.safety_validator {
            let safety_invocation_id =
                ToolInvocationId::parse(&tool_item_id).unwrap_or_else(|_| ToolInvocationId::new());
            let validation = {
                let mut validator = safety_validator.write().await;
                validator
                    .validate_call_with_invocation_id(
                        &canonical_name,
                        args_val,
                        safety_invocation_id,
                    )
                    .await
            };
            if let Err(err) = validation {
                return Ok(ToolPipelineOutcome::from_status(
                    ToolExecutionStatus::Failure {
                        error: anyhow!("Safety validation failed: {}", err),
                    },
                ));
            }
        }
    } else if let Some(tool) = ctx.tool_registry.get_tool(requested_name) {
        canonical_name = tool.name().to_string();
    }
    let name = canonical_name.as_str();

    let harness_emitter = ctx.harness_emitter;
    let mut tool_started_emitted = false;
    if let Some(emitter) = harness_emitter {
        let _ = emitter.emit(tool_started_event(tool_item_id.clone(), name, args_val));
        tool_started_emitted = true;
    }
    let max_tool_retries = ctx.harness_state.max_tool_retries as usize;
    let finish_with_status = |status: ToolExecutionStatus| {
        let outcome = ToolPipelineOutcome::from_status(status);
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            &tool_item_id,
            name,
            args_val,
            &outcome.status,
        );
        outcome
    };

    if !ctx.session_stats.is_plan_mode() && name == tools::PLAN_TASK_TRACKER {
        return Ok(finish_with_status(ToolExecutionStatus::Failure {
            error: anyhow!(
                "plan_task_tracker is a Plan Mode compatibility alias. Use task_tracker in Edit mode, or switch to Plan Mode."
            ),
        }));
    }

    if !prevalidated {
        ctx.harness_state.record_tool_call();
        if ctx.harness_state.should_emit_tool_budget_warning(0.75) {
            let used = ctx.harness_state.tool_calls;
            let max = ctx.harness_state.max_tool_calls;
            let remaining = ctx.harness_state.remaining_tool_calls();
            tracing::info!(
                used,
                max,
                remaining,
                "Tool-call budget warning threshold reached in tool pipeline path"
            );
            ctx.harness_state.mark_tool_budget_warning_emitted();
        }
        if let Some(permission_failure) = check_tool_permission(
            ctx,
            name,
            args_val,
            ctrl_c_state,
            ctrl_c_notify,
            default_placeholder,
            lifecycle_hooks,
            skip_confirmations,
            vt_cfg,
        )
        .await
        {
            return Ok(finish_with_status(permission_failure));
        }
    }

    let request_user_input_enabled = FeatureSet::from_config(vt_cfg)
        .request_user_input_enabled(ctx.session_stats.is_plan_mode(), true);
    if let Some(hitl_result) = execute_hitl_tool(
        name,
        ctx.handle,
        ctx.session,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        request_user_input_enabled,
    )
    .await
    {
        let status = match hitl_result {
            Ok(value) => ToolExecutionStatus::Success {
                output: value,
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
            Err(error) => ToolExecutionStatus::Failure { error },
        };
        return Ok(finish_with_status(status));
    }

    if let Some(outcome) = handle_enter_plan_mode(
        ctx,
        name,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        max_tool_retries,
    )
    .await
    {
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            &tool_item_id,
            name,
            args_val,
            &outcome.status,
        );
        return Ok(outcome);
    }
    if let Some(outcome) = handle_exit_plan_mode(
        ctx,
        name,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        max_tool_retries,
        vt_cfg,
    )
    .await
    {
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            &tool_item_id,
            name,
            args_val,
            &outcome.status,
        );
        return Ok(outcome);
    }

    let execution_status = execute_with_cache_and_streaming(
        ctx.tool_registry,
        ctx.tool_result_cache,
        name,
        &tool_item_id,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        ctx.handle,
        harness_emitter.cloned(),
        vt_cfg,
        max_tool_retries,
    )
    .await;
    let execution_status = resolve_file_conflict_status(
        ctx.tool_registry,
        ctx.tool_result_cache,
        ctx.session,
        ctx.handle,
        name,
        &tool_item_id,
        args_val,
        execution_status,
        ctrl_c_state,
        ctrl_c_notify,
        harness_emitter.cloned(),
        vt_cfg,
        max_tool_retries,
    )
    .await?;

    let mut pipeline_outcome = ToolPipelineOutcome::from_status(execution_status);
    apply_post_execution_side_effects(
        ctx,
        &tool_item_id,
        name,
        args_val,
        turn_index,
        skip_confirmations,
        harness_emitter,
        tool_started_emitted,
        &mut pipeline_outcome,
    )
    .await?;

    emit_tool_completion_for_status(
        harness_emitter,
        tool_started_emitted,
        &tool_item_id,
        name,
        args_val,
        &pipeline_outcome.status,
    );
    Ok(pipeline_outcome)
}

async fn check_tool_permission(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    lifecycle_hooks: Option<&LifecycleHookEngine>,
    skip_confirmations: bool,
    vt_cfg: Option<&VTCodeConfig>,
) -> Option<ToolExecutionStatus> {
    match ensure_tool_permission(
        crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
            tool_registry: ctx.tool_registry,
            renderer: ctx.renderer,
            handle: ctx.handle,
            session: ctx.session,
            default_placeholder,
            ctrl_c_state,
            ctrl_c_notify,
            hooks: lifecycle_hooks,
            justification: None,
            approval_recorder: Some(ctx.approval_recorder),
            decision_ledger: Some(ctx.decision_ledger),
            tool_permission_cache: Some(ctx.tool_permission_cache),
            hitl_notification_bell: vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
            autonomous_mode: ctx.session_stats.is_autonomous_mode(),
            approval_policy: vt_cfg
                .map(|cfg| cfg.security.human_in_the_loop)
                .map(approval_policy_from_human_in_the_loop)
                .unwrap_or(vtcode_core::exec_policy::AskForApproval::OnRequest),
            skip_confirmations,
        },
        name,
        Some(args_val),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => None,
        Ok(ToolPermissionFlow::Denied) => Some(ToolExecutionStatus::Failure {
            error: anyhow!("Tool permission denied"),
        }),
        Ok(ToolPermissionFlow::Interrupted | ToolPermissionFlow::Exit) => {
            Some(ToolExecutionStatus::Cancelled)
        }
        Err(error) => Some(ToolExecutionStatus::Failure { error }),
    }
}

async fn apply_post_execution_side_effects(
    ctx: &mut RunLoopContext<'_>,
    tool_item_id: &str,
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
        let keep_changes =
            match confirm_changes_with_git_diff(&modified_files, skip_confirmations).await {
                Ok(value) => value,
                Err(error) => {
                    emit_tool_completion_status(
                        harness_emitter,
                        tool_started_emitted,
                        tool_item_id,
                        name,
                        args_val,
                        ToolCallStatus::Failed,
                        None,
                        error.to_string(),
                    );
                    return Err(error);
                }
            };

        if keep_changes {
            ctx.traj
                .log_tool_call(turn_index, name, args_val, pipeline_outcome.command_success);
            if pipeline_outcome.command_success {
                let mut cache = ctx.tool_result_cache.write().await;
                cache.invalidate_for_paths(pipeline_outcome.modified_files().iter());
            }
        } else {
            if let Some(files) = pipeline_outcome.modified_files_mut() {
                files.clear();
            }
            pipeline_outcome.set_command_success(false);
        }
    } else {
        ctx.traj
            .log_tool_call(turn_index, name, args_val, pipeline_outcome.command_success);
    }

    Ok(())
}
