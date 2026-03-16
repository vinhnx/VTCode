//! Plan confirmation HITL flow for Plan -> Edit execution.
//!
//! This implementation routes plan confirmation through the shared overlay driver.

use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::sync::Notify;

use vtcode_tui::{
    InlineHandle, InlineListItem, InlineListSelection, InlineMessageKind, InlineSession,
    ListOverlayRequest, OverlayHotkey, OverlayHotkeyAction, OverlayHotkeyKey, OverlayRequest,
    OverlaySubmission, PlanContent,
};

use super::overlay_prompt::{OverlayWaitOutcome, wait_for_overlay_submission};
use super::state::CtrlCState;

/// Result of the plan confirmation flow
#[derive(Debug, Clone)]
pub(crate) enum PlanConfirmationOutcome {
    /// User approved execution with manual edit approvals
    Execute,
    /// User approved with auto-accept enabled for future confirmations
    AutoAccept,
    /// User wants to edit the plan
    EditPlan,
    /// User cancelled
    Cancel,
}

fn line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn append_message(handle: &InlineHandle, kind: InlineMessageKind, text: impl Into<String>) {
    let text = text.into();
    handle.append_pasted_message(kind, text.clone(), line_count(&text));
}

fn render_confirmation_prompt(handle: &InlineHandle, plan: &PlanContent) {
    append_message(handle, InlineMessageKind::Info, "Ready to code?");
    append_message(
        handle,
        InlineMessageKind::Info,
        "A plan is ready to execute. Would you like to proceed?",
    );

    // Keep confirmation compact to avoid duplicating the already-rendered plan content.
    if !plan.summary.trim().is_empty() {
        append_message(handle, InlineMessageKind::Agent, plan.summary.clone());
    } else if !plan.title.trim().is_empty() {
        append_message(
            handle,
            InlineMessageKind::Info,
            format!("Plan: {}", plan.title),
        );
    }

    if let Some(path) = plan.file_path.as_deref()
        && !path.trim().is_empty()
    {
        append_message(
            handle,
            InlineMessageKind::Info,
            format!("Plan file: {path}"),
        );
    }
    append_message(
        handle,
        InlineMessageKind::Info,
        "Use the confirmation list to choose auto-accept, manual approve, or revise.",
    );
}

fn build_plan_confirmation_request(plan: &PlanContent, draft_incomplete: bool) -> OverlayRequest {
    let mut lines: Vec<String> = plan
        .raw_content
        .lines()
        .map(|line| line.to_string())
        .collect();
    if lines.is_empty() && !plan.summary.is_empty() {
        lines.push(plan.summary.clone());
    }
    lines.insert(
        0,
        "A plan is ready to execute. Would you like to proceed?".to_string(),
    );

    let footer_hint = plan
        .file_path
        .as_ref()
        .map(|path| format!("ctrl-g to edit in VS Code · {path}"));
    let items = vec![
        InlineListItem {
            title: "Yes, auto-accept edits".to_string(),
            subtitle: Some("Execute with auto-approval.".to_string()),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalAutoAccept),
            search_value: None,
        },
        InlineListItem {
            title: "Yes, manually approve edits".to_string(),
            subtitle: Some("Keep context and confirm each edit before applying.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalExecute),
            search_value: None,
        },
        InlineListItem {
            title: "Type feedback to revise the plan".to_string(),
            subtitle: Some("Return to plan mode and refine the plan.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalEditPlan),
            search_value: None,
        },
    ];

    let selected = if draft_incomplete {
        InlineListSelection::PlanApprovalEditPlan
    } else {
        InlineListSelection::PlanApprovalAutoAccept
    };

    OverlayRequest::List(ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines,
        footer_hint,
        items,
        selected: Some(selected),
        search: None,
        hotkeys: vec![OverlayHotkey {
            key: OverlayHotkeyKey::CtrlChar('g'),
            action: OverlayHotkeyAction::LaunchEditor,
        }],
    })
}

/// Execute the plan confirmation HITL flow after exit_plan_mode tool.
///
/// The plan is rendered as static transcript markdown plus an inline confirmation list.
pub(crate) async fn execute_plan_confirmation(
    handle: &InlineHandle,
    session: &mut InlineSession,
    plan_content: PlanContent,
    draft_incomplete: bool,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<PlanConfirmationOutcome> {
    handle.show_overlay(build_plan_confirmation_request(
        &plan_content,
        draft_incomplete,
    ));
    render_confirmation_prompt(handle, &plan_content);
    let outcome =
        wait_for_overlay_submission(handle, session, ctrl_c_state, ctrl_c_notify, |submission| {
            match submission {
                OverlaySubmission::Selection(InlineListSelection::PlanApprovalExecute) => {
                    Some(PlanConfirmationOutcome::Execute)
                }
                OverlaySubmission::Selection(InlineListSelection::PlanApprovalAutoAccept) => {
                    Some(PlanConfirmationOutcome::AutoAccept)
                }
                OverlaySubmission::Selection(InlineListSelection::PlanApprovalEditPlan) => {
                    Some(PlanConfirmationOutcome::EditPlan)
                }
                OverlaySubmission::Hotkey(OverlayHotkeyAction::LaunchEditor) => {
                    handle.set_input("/edit".to_string());
                    Some(PlanConfirmationOutcome::EditPlan)
                }
                OverlaySubmission::Selection(_) => Some(PlanConfirmationOutcome::Cancel),
                _ => None,
            }
        })
        .await?;

    Ok(match outcome {
        OverlayWaitOutcome::Submitted(outcome) => outcome,
        OverlayWaitOutcome::Cancelled
        | OverlayWaitOutcome::Interrupted
        | OverlayWaitOutcome::Exit => PlanConfirmationOutcome::Cancel,
    })
}

/// Convert plan confirmation outcome to tool result JSON
pub(crate) fn plan_confirmation_outcome_to_json(outcome: &PlanConfirmationOutcome) -> Value {
    match outcome {
        PlanConfirmationOutcome::Execute => json!({
            "status": "approved",
            "action": "execute",
            "message": "User approved the plan. Proceed with implementation."
        }),
        PlanConfirmationOutcome::AutoAccept => json!({
            "status": "approved",
            "action": "execute",
            "auto_accept": true,
            "message": "User approved with auto-accept. Proceed with implementation."
        }),
        PlanConfirmationOutcome::EditPlan => json!({
            "status": "edit_requested",
            "action": "stay_in_plan_mode",
            "message": "User wants to edit the plan. Remain in plan mode and await further instructions."
        }),
        PlanConfirmationOutcome::Cancel => json!({
            "status": "cancelled",
            "action": "cancel",
            "message": "User cancelled the plan. Do not proceed with implementation."
        }),
    }
}
