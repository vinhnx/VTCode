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
use vtcode_core::tools::command_args;
use vtcode_core::tools::handlers::plan_mode::PlanLifecyclePhase;
use vtcode_core::tools::registry::{ExecSettlementMode, ToolExecutionError};
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_started_event,
};
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};

use super::execute_hitl_tool;
use super::execution_events::{emit_tool_completion_for_status, emit_tool_completion_status};
use super::execution_plan_mode::{handle_enter_plan_mode, handle_exit_plan_mode};
use super::execution_runtime::execute_with_cache_and_streaming;
use super::file_conflict_prompt::resolve_file_conflict_status;
use super::status::{ToolExecutionStatus, ToolPipelineOutcome};

fn resolve_harness_item_identity(tool_item_id: &str) -> (ToolInvocationId, String) {
    match ToolInvocationId::parse(tool_item_id) {
        Ok(invocation_id) => (invocation_id, tool_item_id.to_string()),
        Err(_) => {
            let invocation_id = ToolInvocationId::new();
            (
                invocation_id,
                format!("{tool_item_id}:{}", invocation_id.short()),
            )
        }
    }
}

fn structured_failure_from_message(
    tool_name: &str,
    message: impl Into<String>,
) -> ToolExecutionError {
    let message = message.into();
    ToolExecutionError::from_anyhow(
        tool_name,
        &anyhow!(message),
        0,
        false,
        false,
        Some("unified_runloop"),
    )
}

fn structured_failure(tool_name: &str, error: &anyhow::Error) -> ToolExecutionError {
    ToolExecutionError::from_anyhow(tool_name, error, 0, false, false, Some("unified_runloop"))
}

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
        return Ok(ToolPipelineOutcome::from_status(
            ToolExecutionStatus::Failure {
                error: structured_failure_from_message("tool", "Tool call missing function"),
            },
        ));
    }

    let args_val = match call.execution_arguments() {
        Ok(args) => args,
        Err(err) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: structured_failure("tool", &anyhow!(err)),
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
    let mut effective_args = args_val.clone();
    let mut canonical_name = requested_name.to_string();
    let tool_call_id = tool_item_id.as_str();
    let (safety_invocation_id, fallback_harness_item_id) =
        resolve_harness_item_identity(&tool_item_id);

    if !prevalidated {
        if let Some(max_tool_calls) = ctx.harness_state.exhausted_tool_call_limit() {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: structured_failure(
                        requested_name,
                        &anyhow::anyhow!(
                            "Policy violation: exceeded max tool calls per turn ({})",
                            max_tool_calls
                        ),
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
                        error: structured_failure(
                            requested_name,
                            &anyhow!("Tool argument validation failed: {}", err),
                        ),
                    },
                ));
            }
        }

        let _ = safety_invocation_id;
    } else if let Some(tool) = ctx.tool_registry.get_tool(requested_name) {
        canonical_name = tool.name().to_string();
    }
    let name = canonical_name.as_str();

    let harness_emitter = ctx.harness_emitter;
    let streamed_harness_item_id = ctx
        .harness_state
        .take_streamed_tool_call_item_id(tool_call_id);
    let mut tool_started_emitted = streamed_harness_item_id.is_some();
    let harness_item_id = streamed_harness_item_id.unwrap_or(fallback_harness_item_id);
    if !tool_started_emitted && let Some(emitter) = harness_emitter {
        let _ = emitter.emit(tool_started_event(
            harness_item_id.clone(),
            name,
            Some(&effective_args),
            Some(tool_call_id),
        ));
        tool_started_emitted = true;
    }
    let max_tool_retries = ctx.harness_state.max_tool_retries as usize;
    let finish_with_status =
        |status: ToolExecutionStatus, tool_execution_started: bool, args: &Value| {
            let outcome = ToolPipelineOutcome::from_status(status);
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
            outcome
        };

    if !ctx.session_stats.is_plan_mode() && name == tools::PLAN_TASK_TRACKER {
        return Ok(finish_with_status(
            ToolExecutionStatus::Failure {
                error: structured_failure(
                    name,
                    &anyhow!(
                        "plan_task_tracker is a Plan Mode compatibility alias. Use task_tracker in Edit mode, or switch to Plan Mode."
                    ),
                ),
            },
            false,
            &effective_args,
        ));
    }

    if !prevalidated {
        match check_tool_permission(
            ctx,
            name,
            &effective_args,
            ctrl_c_state,
            ctrl_c_notify,
            default_placeholder,
            lifecycle_hooks,
            skip_confirmations,
            vt_cfg,
        )
        .await
        {
            Ok(Some(updated_args)) => effective_args = updated_args,
            Ok(None) => {}
            Err(permission_failure) => {
                return Ok(finish_with_status(
                    permission_failure,
                    false,
                    &effective_args,
                ));
            }
        }

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
    }

    let request_user_input_enabled = FeatureSet::from_config(vt_cfg)
        .request_user_input_enabled(ctx.session_stats.is_plan_mode(), true);
    if ctx.session_stats.is_plan_mode() && name == tools::REQUEST_USER_INPUT {
        ctx.tool_registry
            .plan_mode_state()
            .set_phase(PlanLifecyclePhase::InterviewPending);
    }
    if let Some(hitl_result) = execute_hitl_tool(
        name,
        ctx.handle,
        ctx.session,
        &effective_args,
        ctrl_c_state,
        ctrl_c_notify,
        request_user_input_enabled,
    )
    .await
    {
        if ctx.session_stats.is_plan_mode() && name == tools::REQUEST_USER_INPUT {
            ctx.tool_registry
                .plan_mode_state()
                .set_phase(PlanLifecyclePhase::ActiveDrafting);
        }
        let status = match hitl_result {
            Ok(value) => ToolExecutionStatus::Success {
                output: value,
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
            Err(error) => ToolExecutionStatus::Failure {
                error: structured_failure(name, &error),
            },
        };
        return Ok(finish_with_status(status, true, &effective_args));
    }

    if let Some(outcome) = handle_enter_plan_mode(
        ctx,
        name,
        &effective_args,
        ctrl_c_state,
        ctrl_c_notify,
        max_tool_retries,
        prevalidated,
    )
    .await
    {
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            true,
            &harness_item_id,
            tool_call_id,
            name,
            &effective_args,
            &outcome.status,
        );
        return Ok(outcome);
    }
    if let Some(outcome) = handle_exit_plan_mode(
        ctx,
        name,
        &effective_args,
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
            true,
            &harness_item_id,
            tool_call_id,
            name,
            &effective_args,
            &outcome.status,
        );
        return Ok(outcome);
    }

    let execution = execute_with_cache_and_streaming(
        ctx.tool_registry,
        ctx.tool_result_cache,
        name,
        &harness_item_id,
        tool_call_id,
        &effective_args,
        ctrl_c_state,
        ctrl_c_notify,
        ctx.handle,
        harness_emitter.cloned(),
        vt_cfg,
        max_tool_retries,
        exec_settlement_mode_for_tool_call(prevalidated, name, &effective_args),
    )
    .await;
    let execution_status = resolve_file_conflict_status(
        ctx.tool_registry,
        ctx.tool_result_cache,
        ctx.session,
        ctx.handle,
        name,
        &harness_item_id,
        tool_call_id,
        &effective_args,
        execution,
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
        &harness_item_id,
        tool_call_id,
        name,
        &effective_args,
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
        true,
        &harness_item_id,
        tool_call_id,
        name,
        &effective_args,
        &pipeline_outcome.status,
    );
    Ok(pipeline_outcome)
}

pub(crate) fn exec_settlement_mode_for_tool_call(
    prevalidated: bool,
    name: &str,
    args: &Value,
) -> ExecSettlementMode {
    if !prevalidated || name != tools::UNIFIED_EXEC {
        return ExecSettlementMode::Manual;
    }

    let Some(action) = tool_intent::unified_exec_action(args) else {
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

    if action.eq_ignore_ascii_case("continue")
        && command_args::interactive_input_text(args).is_none()
    {
        ExecSettlementMode::SettleNonInteractive
    } else {
        ExecSettlementMode::Manual
    }
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
) -> std::result::Result<Option<Value>, ToolExecutionStatus> {
    let auto_mode_runtime = ctx.auto_mode.as_mut().map(|auto_mode| {
        crate::agent::runloop::unified::run_loop_context::AutoModeRuntimeContext {
            config: auto_mode.config,
            vt_cfg,
            provider_client: &mut *auto_mode.provider_client,
            working_history: auto_mode.working_history,
        }
    });

    match ensure_tool_permission(
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
            hitl_notification_bell: vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
            approval_policy: vt_cfg
                .map(|cfg| cfg.security.human_in_the_loop)
                .map(approval_policy_from_human_in_the_loop)
                .unwrap_or(vtcode_core::exec_policy::AskForApproval::OnRequest),
            skip_confirmations,
            permissions_config: vt_cfg.map(|cfg| &cfg.permissions),
            auto_mode_runtime,
            session_stats: Some(ctx.session_stats),
        },
        name,
        Some(args_val),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved { updated_args }) => Ok(updated_args),
        Ok(ToolPermissionFlow::Denied) => Err(ToolExecutionStatus::Failure {
            error: structured_failure_from_message(name, "Tool permission denied"),
        }),
        Ok(ToolPermissionFlow::Blocked { reason }) => Err(ToolExecutionStatus::Failure {
            error: structured_failure_from_message(name, reason),
        }),
        Ok(ToolPermissionFlow::Interrupted | ToolPermissionFlow::Exit) => {
            Err(ToolExecutionStatus::Cancelled)
        }
        Err(error) => Err(ToolExecutionStatus::Failure {
            error: structured_failure(name, &error),
        }),
    }
}

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
        let keep_changes =
            match confirm_changes_with_git_diff(&modified_files, skip_confirmations).await {
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

#[cfg(test)]
mod tests {
    use super::{exec_settlement_mode_for_tool_call, resolve_harness_item_identity};
    use serde_json::json;
    use vtcode_core::tools::registry::ExecSettlementMode;
    use vtcode_core::{config::constants::tools, tools::ToolInvocationId};

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
