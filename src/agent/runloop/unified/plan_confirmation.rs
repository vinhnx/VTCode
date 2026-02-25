//! Plan confirmation HITL tool for exit_plan_mode
//!
//! This module handles the "Execute After Confirmation" pattern from Claude Code's
//! plan mode workflow. When the agent calls `exit_plan_mode`, this shows the user
//! an Implementation Blueprint panel and waits for confirmation before proceeding.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::task;

use vtcode_core::ui::tui::{
    InlineEvent, InlineHandle, InlineListSelection, InlineSession, PlanConfirmationResult,
    PlanContent,
};

use super::state::{CtrlCSignal, CtrlCState};

/// Result of the plan confirmation flow
#[derive(Debug, Clone)]
pub enum PlanConfirmationOutcome {
    /// User approved execution with manual edit approvals
    Execute,
    /// User approved with auto-accept enabled for future confirmations
    AutoAccept,
    /// User approved with context clear and auto-accept enabled
    ClearContextAutoAccept,
    /// User wants to edit the plan
    EditPlan,
    /// User cancelled
    Cancel,
}

/// Execute the plan confirmation HITL flow after exit_plan_mode tool.
///
/// This shows the Implementation Blueprint panel with the plan summary
/// and waits for user to choose: Execute or Stay in Plan Mode.
pub(crate) async fn execute_plan_confirmation(
    handle: &InlineHandle,
    session: &mut InlineSession,
    plan_content: PlanContent,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<PlanConfirmationOutcome> {
    // Show the plan confirmation modal
    handle.show_plan_confirmation(plan_content);
    handle.force_redraw();
    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
            handle.close_modal();
            handle.force_redraw();
            task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            return Ok(PlanConfirmationOutcome::Cancel);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            handle.close_modal();
            handle.force_redraw();
            task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            return Ok(PlanConfirmationOutcome::Cancel);
        };

        match event {
            InlineEvent::Interrupt => {
                let signal = if ctrl_c_state.is_exit_requested() {
                    CtrlCSignal::Exit
                } else if ctrl_c_state.is_cancel_requested() {
                    CtrlCSignal::Cancel
                } else {
                    ctrl_c_state.register_signal()
                };
                ctrl_c_notify.notify_waiters();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                match signal {
                    CtrlCSignal::Exit => return Ok(PlanConfirmationOutcome::Cancel),
                    CtrlCSignal::Cancel => return Ok(PlanConfirmationOutcome::Cancel),
                }
            }
            InlineEvent::PlanConfirmation(result) => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                return Ok(match result {
                    PlanConfirmationResult::Execute => PlanConfirmationOutcome::Execute,
                    PlanConfirmationResult::AutoAccept => PlanConfirmationOutcome::AutoAccept,
                    PlanConfirmationResult::ClearContextAutoAccept => {
                        PlanConfirmationOutcome::ClearContextAutoAccept
                    }
                    PlanConfirmationResult::EditPlan => PlanConfirmationOutcome::EditPlan,
                    PlanConfirmationResult::Cancel => PlanConfirmationOutcome::Cancel,
                });
            }
            // Handle direct list modal submissions (when plan approval selections are chosen)
            InlineEvent::ListModalSubmit(selection) => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                return Ok(match selection {
                    InlineListSelection::PlanApprovalExecute => PlanConfirmationOutcome::Execute,
                    InlineListSelection::PlanApprovalClearContextAutoAccept => {
                        PlanConfirmationOutcome::ClearContextAutoAccept
                    }
                    InlineListSelection::PlanApprovalAutoAccept => {
                        PlanConfirmationOutcome::AutoAccept
                    }
                    InlineListSelection::PlanApprovalEditPlan => PlanConfirmationOutcome::EditPlan,
                    InlineListSelection::PlanApprovalCancel => PlanConfirmationOutcome::Cancel,
                    _ => PlanConfirmationOutcome::Cancel, // Unknown selection = cancel
                });
            }
            InlineEvent::ListModalCancel | InlineEvent::Cancel => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(PlanConfirmationOutcome::Cancel);
            }
            InlineEvent::Exit => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(PlanConfirmationOutcome::Cancel);
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => {
                // Ignore text input while modal is shown.
                continue;
            }
            _ => {}
        }
    }
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
        PlanConfirmationOutcome::ClearContextAutoAccept => json!({
            "status": "approved",
            "action": "execute",
            "auto_accept": true,
            "clear_context": true,
            "message": "User approved with context clear and auto-accept. Proceed with implementation."
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
