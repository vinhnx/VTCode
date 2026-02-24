use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::ui::tui::PlanContent;

use crate::agent::runloop::unified::plan_confirmation::{
    PlanConfirmationOutcome, execute_plan_confirmation, plan_confirmation_outcome_to_json,
};
use crate::agent::runloop::unified::plan_mode_state::{
    render_plan_mode_next_step_hint, transition_to_edit_mode, transition_to_plan_mode,
};
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::plan_content::parse_plan_content_from_json;

use super::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
use super::status::{ToolExecutionStatus, ToolPipelineOutcome};

pub(super) async fn handle_enter_plan_mode(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
) -> Option<ToolPipelineOutcome> {
    if name != tools::ENTER_PLAN_MODE {
        return None;
    }

    let tool_result = execute_tool_with_timeout_ref_prevalidated(
        ctx.tool_registry,
        name,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        None,
        max_tool_retries,
    )
    .await;

    if let ToolExecutionStatus::Success { ref output, .. } = tool_result {
        let status = output.get("status").and_then(|s| s.as_str());
        if status == Some("success") {
            transition_to_plan_mode(
                ctx.tool_registry,
                ctx.session_stats,
                ctx.handle,
                false,
                false,
            )
            .await;
            if let Err(err) = render_plan_mode_next_step_hint(ctx.renderer) {
                tracing::warn!("failed to render plan mode next-step hint: {}", err);
            }
            tracing::info!(
                target: "vtcode.plan_mode",
                "Agent entered Plan Mode with planner profile (read-only, mutating tools blocked)"
            );
        }
    }

    Some(ToolPipelineOutcome::from_status(tool_result))
}

pub(super) async fn handle_exit_plan_mode(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
    vt_cfg: Option<&VTCodeConfig>,
) -> Option<ToolPipelineOutcome> {
    if name != tools::EXIT_PLAN_MODE {
        return None;
    }

    let require_confirmation = vt_cfg
        .map(|cfg| cfg.agent.require_plan_confirmation)
        .unwrap_or(true);

    let tool_result = execute_tool_with_timeout_ref_prevalidated(
        ctx.tool_registry,
        name,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        None,
        max_tool_retries,
    )
    .await;

    if let ToolExecutionStatus::Success { ref output, .. } = tool_result {
        let status = output.get("status").and_then(|s| s.as_str());
        let requires_confirmation_from_result = output
            .get("requires_confirmation")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);

        if status == Some("pending_confirmation")
            && requires_confirmation_from_result
            && require_confirmation
        {
            return Some(
                handle_pending_confirmation(ctx, output, ctrl_c_state, ctrl_c_notify).await,
            );
        }

        if !require_confirmation {
            transition_to_edit_mode(ctx.tool_registry, ctx.session_stats, ctx.handle, true).await;
            tracing::info!(
                target: "vtcode.plan_mode",
                "Plan confirmation disabled via config, auto-approving with coder profile (mutating tools enabled)"
            );
            return Some(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Success {
                    output: serde_json::json!({
                        "status": "approved",
                        "action": "execute",
                        "auto_accept": true,
                        "message": "Plan confirmation disabled. Proceeding with implementation."
                    }),
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                },
            ));
        }
    }

    Some(ToolPipelineOutcome::from_status(tool_result))
}

async fn handle_pending_confirmation(
    ctx: &mut RunLoopContext<'_>,
    output: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> ToolPipelineOutcome {
    let plan_content = build_plan_content(output);
    let confirmation_outcome = execute_plan_confirmation(
        ctx.handle,
        ctx.session,
        plan_content,
        ctrl_c_state,
        ctrl_c_notify,
    )
    .await;

    let final_output = match confirmation_outcome {
        Ok(outcome) => {
            if matches!(
                outcome,
                PlanConfirmationOutcome::Execute | PlanConfirmationOutcome::AutoAccept
            ) {
                transition_to_edit_mode(ctx.tool_registry, ctx.session_stats, ctx.handle, true)
                    .await;
                tracing::info!(
                    target: "vtcode.plan_mode",
                    "User approved plan execution, transitioning to coder profile (mutating tools enabled)"
                );
            } else if matches!(outcome, PlanConfirmationOutcome::EditPlan) {
                tracing::info!(
                    target: "vtcode.plan_mode",
                    "User requested plan edit, remaining in Plan mode"
                );
            }
            plan_confirmation_outcome_to_json(&outcome)
        }
        Err(e) => serde_json::json!({
            "status": "error",
            "error": format!("Plan confirmation failed: {}", e)
        }),
    };

    ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: final_output,
        stdout: None,
        modified_files: vec![],
        command_success: true,
        has_more: false,
    })
}

fn build_plan_content(output: &Value) -> PlanContent {
    if let Some(raw_content) = output.get("plan_content").and_then(|v| v.as_str()) {
        let title = output
            .get("plan_summary")
            .and_then(|s| s.get("title"))
            .and_then(|t| t.as_str())
            .unwrap_or("Implementation Plan")
            .to_string();
        let file_path = output
            .get("plan_file")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string());
        PlanContent::from_markdown(title, raw_content, file_path)
    } else {
        let plan_summary_json = output.get("plan_summary").cloned().unwrap_or_default();
        parse_plan_content_from_json(&plan_summary_json)
    }
}
