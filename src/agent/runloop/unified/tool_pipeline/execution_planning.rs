use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::tools::handlers::planning_workflow::PlanLifecyclePhase;
use vtcode_core::tools::registry::ExecSettlementMode;
use vtcode_ui::tui::app::PlanContent;
use vtcode_ui::tui::app::{
    InlineListItem, InlineListSelection, ListOverlayRequest, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::plan_confirmation::{
    PlanConfirmationOutcome, execute_plan_confirmation, plan_confirmation_outcome_to_json,
};
use crate::agent::runloop::unified::planning_workflow_state::{
    finish_planning_workflow, render_planning_workflow_next_step_hint,
    transition_to_planning_workflow,
};
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::turn::plan_content::parse_plan_content_from_json;

use super::execution_attempts::execute_tool_with_timeout_ref_prevalidated;
use super::status::{ToolExecutionStatus, ToolPipelineOutcome};

const START_PLANNING_APPROVE_ACTION: &str = "planning:start";
const START_PLANNING_STAY_ACTION: &str = "planning:stay";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartPlanningConfirmation {
    Enter,
    Stay,
}

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
    if status == Some("pending_confirmation") && requires_confirmation_from_result {
        if require_confirmation {
            FinishPlanningDisposition::ConfirmReview
        } else {
            FinishPlanningDisposition::AutoAccept
        }
    } else {
        FinishPlanningDisposition::Passthrough
    }
}

pub(super) async fn handle_start_planning(
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

        if status == Some("pending_confirmation") && requires_confirmation {
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

        if status == Some("success") {
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
    }

    Some(ToolPipelineOutcome::from_status(tool_result))
}

pub(super) async fn handle_finish_planning(
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
                ctx.tool_registry
                    .planning_workflow_state()
                    .set_phase(PlanLifecyclePhase::ReviewPending);
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
                            "status": "approved",
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
            FinishPlanningDisposition::Passthrough => {}
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

    let final_output = match confirmation_outcome {
        Ok(outcome) => {
            if matches!(
                outcome,
                PlanConfirmationOutcome::Execute | PlanConfirmationOutcome::AutoAccept
            ) {
                finish_planning_workflow(ctx.tool_registry, ctx.plan_session, ctx.handle, true)
                    .await;
                ctx.handle
                    .set_skip_confirmations(matches!(outcome, PlanConfirmationOutcome::AutoAccept));
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "User approved plan execution, transitioning to coder profile (mutating tools enabled)"
                );
            } else if matches!(outcome, PlanConfirmationOutcome::EditPlan) {
                ctx.tool_registry
                    .planning_workflow_state()
                    .set_phase(PlanLifecyclePhase::DraftReady);
                tracing::info!(
                    target: "vtcode.planning_workflow",
                    "User requested plan edit, remaining in Planning workflow"
                );
            } else {
                ctx.tool_registry
                    .planning_workflow_state()
                    .set_phase(PlanLifecyclePhase::DraftReady);
            }
            plan_confirmation_outcome_to_json(&outcome)
        }
        Err(e) => {
            ctx.tool_registry
                .planning_workflow_state()
                .set_phase(PlanLifecyclePhase::DraftReady);
            serde_json::json!({
                "status": "error",
                "error": format!("Plan confirmation failed: {}", e)
            })
        }
    };

    ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: final_output,
        stdout: None,
        modified_files: vec![],
        command_success: true,
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

async fn handle_enter_pending_confirmation(
    ctx: &mut RunLoopContext<'_>,
    original_args: &Value,
    output: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_tool_retries: usize,
) -> ToolPipelineOutcome {
    ctx.tool_registry
        .planning_workflow_state()
        .set_phase(PlanLifecyclePhase::EnterPendingApproval);

    let overlay = TransientRequest::List(ListOverlayRequest {
        title: "Enter Planning workflow?".to_string(),
        lines: build_start_planning_lines(output),
        footer_hint: Some(
            "Choose whether to enter the Planning workflow before the agent continues.".to_string(),
        ),
        items: vec![
            InlineListItem {
                title: "Enter Planning workflow".to_string(),
                subtitle: Some(
                    "Enter the Planning workflow and persist the draft under .vtcode/plans."
                        .to_string(),
                ),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    START_PLANNING_APPROVE_ACTION.to_string(),
                )),
                search_value: None,
            },
            InlineListItem {
                title: "Continue without Planning workflow".to_string(),
                subtitle: Some("Continue without entering the Planning workflow.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    START_PLANNING_STAY_ACTION.to_string(),
                )),
                search_value: None,
            },
        ],
        selected: Some(InlineListSelection::ConfigAction(
            START_PLANNING_APPROVE_ACTION.to_string(),
        )),
        search: None,
        hotkeys: Vec::new(),
    });

    let confirmation = show_overlay_and_wait(
        ctx.handle,
        ctx.session,
        overlay,
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                if action == START_PLANNING_APPROVE_ACTION =>
            {
                Some(StartPlanningConfirmation::Enter)
            }
            TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                if action == START_PLANNING_STAY_ACTION =>
            {
                Some(StartPlanningConfirmation::Stay)
            }
            TransientSubmission::Selection(_) => Some(StartPlanningConfirmation::Stay),
            _ => None,
        },
    )
    .await;

    let decision = match confirmation {
        Ok(OverlayWaitOutcome::Submitted(choice)) => choice,
        Ok(OverlayWaitOutcome::Cancelled)
        | Ok(OverlayWaitOutcome::Interrupted)
        | Ok(OverlayWaitOutcome::Exit)
        | Err(_) => StartPlanningConfirmation::Stay,
    };

    if decision == StartPlanningConfirmation::Stay {
        ctx.tool_registry
            .planning_workflow_state()
            .set_phase(PlanLifecyclePhase::Off);
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
        if status == Some("success") {
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
        }
    }

    ToolPipelineOutcome::from_status(tool_result)
}

fn build_start_planning_lines(output: &Value) -> Vec<String> {
    let mut lines =
        vec!["The agent wants to enter the Planning workflow before making edits.".to_string()];
    if let Some(description) = output.get("description").and_then(Value::as_str)
        && !description.trim().is_empty()
    {
        lines.push(format!("Task: {}", description.trim()));
    }
    if let Some(plan_file) = output.get("plan_file").and_then(Value::as_str) {
        lines.push(format!("Plan file: {plan_file}"));
    }
    lines.push(
        "Planning workflow keeps mutating tools disabled until you explicitly approve execution."
            .to_string(),
    );
    lines
}

#[cfg(test)]
mod tests {
    use super::{FinishPlanningDisposition, finish_planning_disposition};

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
}
