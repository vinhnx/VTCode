use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Notify;
use vtcode_config::{builtin_primary_auto_agent, builtin_primary_build_agent};
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::tools::registry::ExecSettlementMode;
use vtcode_ui::tui::app::PlanContent;

use crate::agent::runloop::unified::planning_workflow_state::{
    finish_planning_workflow, render_planning_workflow_next_step_hint,
    transition_to_planning_workflow,
};
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::plan_content::parse_plan_content_from_json;

use crate::agent::runloop::unified::planning_workflow::{
    PlanConfirmationOutcome, StartPlanningDecision, execute_plan_confirmation,
    plan_confirmation_outcome_to_json, present_start_planning_confirmation,
};
use crate::agent::runloop::unified::tool_pipeline::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
use crate::agent::runloop::unified::tool_pipeline::status::{
    ToolExecutionStatus, ToolPipelineOutcome,
};

/// Canonical plan-lifecycle status strings returned by the `start_planning` /
/// `finish_planning` tools. Centralized so the runloop's disposition logic and
/// the tool contract in `vtcode-core` cannot silently drift apart.
const PLAN_STATUS_PENDING_CONFIRMATION: &str = "pending_confirmation";
const PLAN_STATUS_NOT_READY: &str = "not_ready";
const PLAN_STATUS_SUCCESS: &str = "success";
const PLAN_STATUS_APPROVED: &str = "approved";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinishPlanningDisposition {
    ConfirmReview,
    AutoAccept,
    Passthrough,
}

fn finish_planning_disposition(
    status: Option<&str>,
    requires_confirmation_from_result: bool,
    require_confirmation: bool,
) -> FinishPlanningDisposition {
    if status == Some(PLAN_STATUS_PENDING_CONFIRMATION) && requires_confirmation_from_result {
        if require_confirmation {
            FinishPlanningDisposition::ConfirmReview
        } else {
            FinishPlanningDisposition::AutoAccept
        }
    } else {
        FinishPlanningDisposition::Passthrough
    }
}

pub(crate) async fn handle_start_planning(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
    allow_preapproved: bool,
) -> Option<ToolPipelineOutcome> {
    if name != tools::START_PLANNING {
        return None;
    }

    let already_approved = allow_preapproved
        && args_val
            .get("approved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
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
        let requires_confirmation = output
            .get("requires_confirmation")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if status == Some(PLAN_STATUS_PENDING_CONFIRMATION) && requires_confirmation {
            return Some(
                handle_enter_pending_confirmation(
                    ctx,
                    args_val,
                    output,
                    ctrl_c_state,
                    ctrl_c_notify,
                    max_tool_retries,
                )
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

pub(crate) async fn handle_finish_planning(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
    vt_cfg: Option<&VTCodeConfig>,
) -> Option<ToolPipelineOutcome> {
    if name != tools::FINISH_PLANNING {
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
        ExecSettlementMode::Manual,
        false,
    )
    .await;

    if let ToolExecutionStatus::Success { ref output, .. } = tool_result {
        let status = output.get("status").and_then(|s| s.as_str());
        let requires_confirmation_from_result = output
            .get("requires_confirmation")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);

        match finish_planning_disposition(
            status,
            requires_confirmation_from_result,
            require_confirmation,
        ) {
            FinishPlanningDisposition::ConfirmReview => {
                return Some(
                    handle_pending_confirmation(ctx, output, ctrl_c_state, ctrl_c_notify).await,
                );
            }
            FinishPlanningDisposition::AutoAccept => {
                finish_planning_workflow(ctx.tool_registry, ctx.plan_session, ctx.handle, true)
                    .await;
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "Plan confirmation disabled via config, auto-approving with coder profile (mutating tools enabled)"
                );
                return Some(ToolPipelineOutcome::from_status(
                    ToolExecutionStatus::Success {
                        output: serde_json::json!({
                            "status": PLAN_STATUS_APPROVED,
                            "action": "execute",
                            "auto_accept": true,
                            "message": "Plan confirmation disabled. Proceeding with implementation."
                        }),
                        stdout: None,
                        modified_files: vec![],
                        command_success: true,
                    },
                ));
            }
            FinishPlanningDisposition::Passthrough => {
                // When the plan is not ready but the user explicitly requested
                // finish_planning, show the plan confirmation dialog with
                // draft_incomplete so the user can decide to edit or proceed.
                // The `not_ready` tool response omits `draft_incomplete`, so we
                // set it explicitly here to default the dialog to "edit plan".
                if status == Some(PLAN_STATUS_NOT_READY) && require_confirmation {
                    let mut incomplete_output = output.clone();
                    if let Some(obj) = incomplete_output.as_object_mut() {
                        obj.insert("draft_incomplete".to_string(), Value::Bool(true));
                    }
                    return Some(
                        handle_pending_confirmation(
                            ctx,
                            &incomplete_output,
                            ctrl_c_state,
                            ctrl_c_notify,
                        )
                        .await,
                    );
                }
            }
        }
    }

    Some(ToolPipelineOutcome::from_status(tool_result))
}

/// Map a plan-confirmation outcome to the primary-agent name the turn loop
/// should hand off to, if any.
///
/// `SwitchBuild`/`SwitchAuto` request a real primary-agent switch (the chosen
/// agent executes the plan); every other outcome keeps the current agent. Kept
/// as a pure function so the `pending_primary_agent` threading decision is
/// unit-testable without a full [`RunLoopContext`].
fn plan_confirmation_outcome_to_pending_agent(outcome: &PlanConfirmationOutcome) -> Option<String> {
    match outcome {
        PlanConfirmationOutcome::SwitchBuild => Some(builtin_primary_build_agent().name),
        PlanConfirmationOutcome::SwitchAuto => Some(builtin_primary_auto_agent().name),
        PlanConfirmationOutcome::Execute
        | PlanConfirmationOutcome::AutoAccept
        | PlanConfirmationOutcome::EditPlan
        | PlanConfirmationOutcome::Cancel => None,
    }
}

async fn handle_pending_confirmation(
    ctx: &mut RunLoopContext<'_>,
    output: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> ToolPipelineOutcome {
    let plan_content = build_plan_content(output);
    let draft_incomplete = output
        .get("draft_incomplete")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let confirmation_outcome = execute_plan_confirmation(
        ctx.handle,
        ctx.session,
        plan_content,
        draft_incomplete,
        ctrl_c_state,
        ctrl_c_notify,
    )
    .await;

    let (final_output, agent_switch) = match confirmation_outcome {
        Ok(outcome) => {
            let switch = match outcome {
                PlanConfirmationOutcome::Execute | PlanConfirmationOutcome::AutoAccept => {
                    finish_planning_workflow(ctx.tool_registry, ctx.plan_session, ctx.handle, true)
                        .await;
                    ctx.handle.set_skip_confirmations(matches!(
                        outcome,
                        PlanConfirmationOutcome::AutoAccept
                    ));
                    tracing::info!(
                        target: "vtcode.planning_workflow",
                        "User approved plan execution, transitioning to coder profile (mutating tools enabled)"
                    );
                    None
                }
                PlanConfirmationOutcome::SwitchBuild | PlanConfirmationOutcome::SwitchAuto => {
                    finish_planning_workflow(ctx.tool_registry, ctx.plan_session, ctx.handle, true)
                        .await;
                    // Build agent executes with per-step HITL (manual edit
                    // approvals); the auto agent auto-executes the plan.
                    ctx.handle.set_skip_confirmations(matches!(
                        outcome,
                        PlanConfirmationOutcome::SwitchAuto
                    ));
                    tracing::info!(
                        target: "vtcode.planning_workflow",
                        agent = plan_confirmation_outcome_to_pending_agent(&outcome)
                            .unwrap_or_default(),
                        "User handed plan off to a primary agent; switching primary agent"
                    );
                    plan_confirmation_outcome_to_pending_agent(&outcome)
                }
                PlanConfirmationOutcome::EditPlan => {
                    tracing::info!(
                        target: "vtcode.planning_workflow",
                        "User requested plan edit, remaining in Planning workflow"
                    );
                    None
                }
                PlanConfirmationOutcome::Cancel => None,
            };
            (plan_confirmation_outcome_to_json(&outcome), switch)
        }
        Err(e) => (
            serde_json::json!({
                "status": "error",
                "error": format!("Plan confirmation failed: {}", e)
            }),
            None,
        ),
    };

    let mut final_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: final_output,
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    final_outcome.pending_primary_agent = agent_switch;
    final_outcome
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

async fn handle_enter_pending_confirmation(
    ctx: &mut RunLoopContext<'_>,
    original_args: &Value,
    output: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
) -> ToolPipelineOutcome {
    let description = output
        .get("description")
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let plan_file = output
        .get("plan_file")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let decision = match present_start_planning_confirmation(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_planning_requires_pending_confirmation_to_auto_accept() {
        assert_eq!(
            finish_planning_disposition(Some("not_ready"), false, false),
            FinishPlanningDisposition::Passthrough
        );
        assert_eq!(
            finish_planning_disposition(Some("not_ready"), true, false),
            FinishPlanningDisposition::Passthrough
        );
        assert_eq!(
            finish_planning_disposition(Some("pending_confirmation"), true, false),
            FinishPlanningDisposition::AutoAccept
        );
    }

    #[test]
    fn finish_planning_keeps_review_overlay_when_confirmation_enabled() {
        assert_eq!(
            finish_planning_disposition(Some("pending_confirmation"), true, true),
            FinishPlanningDisposition::ConfirmReview
        );
    }

    #[test]
    fn switch_outcomes_request_primary_agent_handoff() {
        // Switch outcomes request a real primary-agent handoff; every other
        // outcome keeps the current agent (no pending switch).
        assert_eq!(
            plan_confirmation_outcome_to_pending_agent(&PlanConfirmationOutcome::SwitchBuild),
            Some(builtin_primary_build_agent().name)
        );
        assert_eq!(
            plan_confirmation_outcome_to_pending_agent(&PlanConfirmationOutcome::SwitchAuto),
            Some(builtin_primary_auto_agent().name)
        );
        assert_eq!(
            plan_confirmation_outcome_to_pending_agent(&PlanConfirmationOutcome::Execute),
            None
        );
        assert_eq!(
            plan_confirmation_outcome_to_pending_agent(&PlanConfirmationOutcome::AutoAccept),
            None
        );
        assert_eq!(
            plan_confirmation_outcome_to_pending_agent(&PlanConfirmationOutcome::EditPlan),
            None
        );
        assert_eq!(
            plan_confirmation_outcome_to_pending_agent(&PlanConfirmationOutcome::Cancel),
            None
        );
    }
}
