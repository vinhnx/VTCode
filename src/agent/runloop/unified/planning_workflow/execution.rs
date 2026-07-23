use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::tools::registry::ExecSettlementMode;

use crate::agent::runloop::unified::planning_workflow_state::{
    render_planning_workflow_next_step_hint, transition_to_planning_workflow,
};
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::state::CtrlCState;

use crate::agent::runloop::unified::planning_workflow::{StartPlanningDecision, present_start_planning_confirmation};
use crate::agent::runloop::unified::tool_pipeline::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
use crate::agent::runloop::unified::tool_pipeline::status::{ToolExecutionStatus, ToolPipelineOutcome};

/// Canonical plan-lifecycle status strings returned by the `start_planning`
/// tool. Centralized so the runloop's disposition logic and the tool contract
/// in `vtcode-core` cannot silently drift apart.
const PLAN_STATUS_PENDING_CONFIRMATION: &str = "pending_confirmation";
const PLAN_STATUS_SUCCESS: &str = "success";

pub(crate) async fn handle_start_planning(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
    prevalidated: bool,
) -> Option<ToolPipelineOutcome> {
    if name != tools::START_PLANNING {
        return None;
    }

    let already_approved = prevalidated && args_val.get("approved").and_then(Value::as_bool).unwrap_or(false);
    let tool_args = if already_approved {
        args_val.clone()
    } else {
        let mut value = args_val.clone();
        if let Some(obj) = value.as_object_mut() {
            obj.insert("require_confirmation".to_string(), Value::Bool(true));
        }
        value
    };

    let tool_result = execute_tool_with_timeout_ref_prevalidated(
        ctx.tool_registry,
        name,
        &tool_args,
        ctrl_c_state,
        ctrl_c_notify,
        None,
        max_tool_retries,
        ExecSettlementMode::Manual,
        false,
    )
    .await;

    if let ToolExecutionStatus::Success { ref output, .. } = tool_result {
        let status = output.get("status").and_then(|s| s.as_str());
        if status == Some(PLAN_STATUS_PENDING_CONFIRMATION) {
            return Some(
                handle_enter_pending_confirmation(ctx, args_val, output, ctrl_c_state, ctrl_c_notify, max_tool_retries)
                    .await,
            );
        }

        if status == Some(PLAN_STATUS_SUCCESS) {
            enter_planning_workflow_after_start(ctx).await;
        }
    }

    Some(ToolPipelineOutcome::from_status(tool_result))
}

/// Transition the session into the Planning workflow after a confirmed start,
/// render the next-step hint, and record the entry. Extracted so both the
/// direct `start_planning` success path and the post-confirmation path share
/// one implementation (DRY; previously duplicated verbatim).
async fn enter_planning_workflow_after_start(ctx: &mut RunLoopContext<'_>) {
    transition_to_planning_workflow(
        ctx.tool_registry,
        ctx.session_stats,
        ctx.plan_session,
        ctx.handle,
        PlanningEntrySource::UserRequest,
        false,
        false,
    )
    .await;
    if let Err(err) = render_planning_workflow_next_step_hint(ctx.renderer) {
        tracing::warn!("failed to render planning workflow next-step hint: {}", err);
    }
    tracing::info!(
        target: "vtcode.planning_workflow",
        "Agent entered Planning workflow with planner profile (read-only, mutating tools blocked)"
    );
}

async fn handle_enter_pending_confirmation(
    ctx: &mut RunLoopContext<'_>,
    original_args: &Value,
    output: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
) -> ToolPipelineOutcome {
    let description = output.get("description").and_then(Value::as_str).map(|s| s.to_string());
    let plan_file = output.get("plan_file").and_then(Value::as_str).map(|s| s.to_string());

    let decision = if ctx.renderer.supports_inline_ui() {
        match present_start_planning_confirmation(
            ctx.handle,
            ctx.session,
            description.as_deref(),
            plan_file.as_deref(),
            ctrl_c_state,
            ctrl_c_notify,
        )
        .await
        {
            Ok(decision) => decision,
            Err(_) => StartPlanningDecision::Stay,
        }
    } else {
        StartPlanningDecision::Enter
    };

    if decision == StartPlanningDecision::Stay {
        return ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: json!({
                "status": "cancelled",
                "action": "continue_without_planning_workflow",
                "message": "User declined Planning workflow entry."
            }),
            stdout: None,
            modified_files: vec![],
            command_success: true,
        });
    }

    let mut approved_args = original_args.clone();
    if let Some(obj) = approved_args.as_object_mut() {
        obj.insert("approved".to_string(), Value::Bool(true));
    }

    let tool_result = execute_tool_with_timeout_ref_prevalidated(
        ctx.tool_registry,
        tools::START_PLANNING,
        &approved_args,
        ctrl_c_state,
        ctrl_c_notify,
        None,
        max_tool_retries,
        ExecSettlementMode::Manual,
        false,
    )
    .await;

    if let ToolExecutionStatus::Success { ref output, .. } = tool_result {
        let status = output.get("status").and_then(|s| s.as_str());
        if status == Some(PLAN_STATUS_SUCCESS) {
            enter_planning_workflow_after_start(ctx).await;
        }
    }

    ToolPipelineOutcome::from_status(tool_result)
}
