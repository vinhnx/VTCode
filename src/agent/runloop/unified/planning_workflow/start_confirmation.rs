//! "Enter Planning workflow?" HITL prompt.
//!
//! Isolates the start-planning confirmation overlay from the plan-approval flow
//! so each can evolve independently and be tested without the other's UI state.

use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;

use std::sync::Arc;

use tokio::sync::Notify;
use vtcode_ui::tui::app::{
    InlineHandle, InlineListSelection, InlineSession, ListOverlayRequest, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_permission_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

const START_PLANNING_APPROVE_ACTION: &str = "planning:start";
const START_PLANNING_STAY_ACTION: &str = "planning:stay";

/// Decision returned by the "enter Planning workflow?" confirmation prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartPlanningDecision {
    /// User chose to enter the Planning workflow.
    Enter,
    /// User chose to continue without the Planning workflow.
    Stay,
}

/// Present the "Enter Planning workflow?" HITL prompt and return the decision.
///
/// This is the UI boundary for the start-planning confirmation: it isolates the
/// TUI overlay construction (list items, selection mapping) from the runloop
/// planning logic so the latter stays decoupled from `vtcode_ui` list types and
/// remains testable without a renderer.
pub(crate) async fn present_start_planning_confirmation(
    handle: &InlineHandle,
    session: &mut InlineSession,
    description: Option<&str>,
    plan_file: Option<&str>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> anyhow::Result<StartPlanningDecision> {
    let mut lines = vec!["The agent wants to enter the Planning workflow before making edits.".to_string()];
    if let Some(description) = description
        && !description.trim().is_empty()
    {
        lines.push(format!("Task: {}", description.trim()));
    }
    if let Some(plan_file) = plan_file {
        lines.push(format!("Plan file: {plan_file}"));
    }
    lines.push("Planning workflow keeps mutating tools disabled until you explicitly approve execution.".to_string());

    let overlay = TransientRequest::List(ListOverlayRequest {
        title: "Enter Planning workflow?".to_string(),
        lines,
        footer_hint: Some("Choose whether to enter the Planning workflow before the agent continues.".to_string()),
        items: vec![
            ui_list::action(
                "Enter Planning workflow",
                "Enter the Planning workflow and persist the draft under .vtcode/plans.".to_string(),
                Some("Recommended".to_string()),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(START_PLANNING_APPROVE_ACTION.to_string())),
            ),
            ui_list::action(
                "Continue without Planning workflow",
                "Continue without entering the Planning workflow.".to_string(),
                None,
                Tone::Neutral,
                Some(InlineListSelection::ConfigAction(START_PLANNING_STAY_ACTION.to_string())),
            ),
        ],
        selected: Some(InlineListSelection::ConfigAction(START_PLANNING_APPROVE_ACTION.to_string())),
        search: None,
        status: None,
        hotkeys: Vec::new(),
    });

    let confirmation =
        show_permission_and_wait(
            handle,
            session,
            overlay,
            ctrl_c_state,
            ctrl_c_notify,
            |submission| match submission {
                TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                    if action == START_PLANNING_APPROVE_ACTION =>
                {
                    Some(StartPlanningDecision::Enter)
                }
                TransientSubmission::Selection(InlineListSelection::ConfigAction(action))
                    if action == START_PLANNING_STAY_ACTION =>
                {
                    Some(StartPlanningDecision::Stay)
                }
                TransientSubmission::Selection(_) => Some(StartPlanningDecision::Stay),
                _ => None,
            },
        )
        .await;

    Ok(match confirmation {
        Ok(OverlayWaitOutcome::Submitted(choice)) => choice,
        Ok(OverlayWaitOutcome::Cancelled)
        | Ok(OverlayWaitOutcome::Interrupted)
        | Ok(OverlayWaitOutcome::Deferred)
        | Ok(OverlayWaitOutcome::Exit)
        | Err(_) => StartPlanningDecision::Stay,
    })
}
