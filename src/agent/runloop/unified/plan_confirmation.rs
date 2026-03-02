//! Plan confirmation HITL flow for Plan -> Edit execution.
//!
//! This implementation renders the proposed plan directly in the transcript and
//! captures inline typed choices (1/2/3/4 or feedback text), without modal/palette UI.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::task;

use vtcode_tui::{
    InlineEvent, InlineHandle, InlineListSelection, InlineMessageKind, InlineSession,
    PlanConfirmationResult, PlanContent,
};

use super::state::{CtrlCSignal, CtrlCState};

/// Result of the plan confirmation flow
#[derive(Debug, Clone)]
pub enum PlanConfirmationOutcome {
    /// User approved execution with manual edit approvals
    Execute,
    /// User approved with auto-accept enabled for future confirmations
    AutoAccept,
    /// User wants to edit the plan
    EditPlan,
    /// User cancelled
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedPlanChoice {
    AutoAccept,
    ManualApprove,
    StayInPlanMode,
    Revise,
}

fn line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn append_message(handle: &InlineHandle, kind: InlineMessageKind, text: impl Into<String>) {
    let text = text.into();
    handle.append_pasted_message(kind, text.clone(), line_count(&text));
}

fn normalize_choice_text(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_plan_choice(input: &str) -> (ParsedPlanChoice, Option<String>) {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return (ParsedPlanChoice::Revise, None);
    }

    let parse_numbered = |choice_number: char| -> Option<Option<String>> {
        let mut chars = trimmed.chars();
        let first = chars.next()?;
        if first != choice_number {
            return None;
        }
        let rest = chars.as_str().trim_start_matches(['.', ')', ':', '-', ' ']);
        if rest.is_empty() {
            Some(None)
        } else {
            Some(Some(rest.to_string()))
        }
    };

    if parse_numbered('1').is_some() {
        return (ParsedPlanChoice::AutoAccept, None);
    }
    if parse_numbered('2').is_some() {
        return (ParsedPlanChoice::ManualApprove, None);
    }
    if parse_numbered('3').is_some() {
        return (ParsedPlanChoice::StayInPlanMode, None);
    }
    if let Some(feedback) = parse_numbered('4') {
        return (ParsedPlanChoice::Revise, feedback);
    }

    let normalized = normalize_choice_text(trimmed);
    let auto_accept_aliases = [
        "yes",
        "y",
        "continue",
        "go",
        "start",
        "implement",
        "execute",
        "auto accept",
        "auto accept edits",
        "yes auto accept edits",
    ];
    if auto_accept_aliases.contains(&normalized.as_str()) {
        return (ParsedPlanChoice::AutoAccept, None);
    }

    let manual_aliases = [
        "manual",
        "manually approve edits",
        "manual approve edits",
        "yes manually approve edits",
        "approve manually",
    ];
    if manual_aliases.contains(&normalized.as_str()) {
        return (ParsedPlanChoice::ManualApprove, None);
    }

    let stay_aliases = [
        "no",
        "stay in plan mode",
        "keep in plan mode",
        "keep planning",
        "continue planning",
        "stay in plan",
    ];
    if stay_aliases.contains(&normalized.as_str()) {
        return (ParsedPlanChoice::StayInPlanMode, None);
    }

    let revise_aliases = [
        "revise",
        "feedback",
        "edit plan",
        "revise plan",
        "type feedback to revise the plan",
    ];
    if revise_aliases.contains(&normalized.as_str()) {
        return (ParsedPlanChoice::Revise, None);
    }

    (ParsedPlanChoice::Revise, Some(trimmed.to_string()))
}

fn render_confirmation_prompt(handle: &InlineHandle, plan: &PlanContent) {
    append_message(handle, InlineMessageKind::Info, "Ready to code?");
    append_message(
        handle,
        InlineMessageKind::Info,
        "A plan is ready to execute. Would you like to proceed?",
    );

    if !plan.raw_content.trim().is_empty() {
        append_message(handle, InlineMessageKind::Agent, plan.raw_content.clone());
    } else if !plan.summary.trim().is_empty() {
        append_message(handle, InlineMessageKind::Agent, plan.summary.clone());
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
        "1. Yes, auto-accept edits (Recommended)",
    );
    append_message(
        handle,
        InlineMessageKind::Info,
        "2. Yes, manually approve edits",
    );
    append_message(handle, InlineMessageKind::Info, "3. No, stay in Plan mode");
    append_message(
        handle,
        InlineMessageKind::Info,
        "4. Type feedback to revise the plan",
    );
}

/// Execute the plan confirmation HITL flow after exit_plan_mode tool.
///
/// The plan is rendered as static transcript markdown plus an inline 4-way choice list.
pub(crate) async fn execute_plan_confirmation(
    handle: &InlineHandle,
    session: &mut InlineSession,
    plan_content: PlanContent,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<PlanConfirmationOutcome> {
    render_confirmation_prompt(handle, &plan_content);
    handle.force_redraw();
    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
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
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                match signal {
                    CtrlCSignal::Exit => return Ok(PlanConfirmationOutcome::Cancel),
                    CtrlCSignal::Cancel => return Ok(PlanConfirmationOutcome::Cancel),
                }
            }
            InlineEvent::Submit(text) | InlineEvent::QueueSubmit(text) => {
                ctrl_c_state.disarm_exit();
                let (choice, feedback) = parse_plan_choice(&text);
                if let Some(feedback) = feedback
                    && !feedback.trim().is_empty()
                {
                    handle.set_input(feedback);
                }
                return Ok(match choice {
                    ParsedPlanChoice::AutoAccept => PlanConfirmationOutcome::AutoAccept,
                    ParsedPlanChoice::ManualApprove => PlanConfirmationOutcome::Execute,
                    ParsedPlanChoice::StayInPlanMode | ParsedPlanChoice::Revise => {
                        PlanConfirmationOutcome::EditPlan
                    }
                });
            }
            InlineEvent::PlanConfirmation(result) => {
                ctrl_c_state.disarm_exit();
                return Ok(match result {
                    PlanConfirmationResult::Execute => PlanConfirmationOutcome::Execute,
                    PlanConfirmationResult::AutoAccept => PlanConfirmationOutcome::AutoAccept,
                    PlanConfirmationResult::EditPlan => PlanConfirmationOutcome::EditPlan,
                    PlanConfirmationResult::Cancel => PlanConfirmationOutcome::Cancel,
                });
            }
            InlineEvent::ListModalSubmit(selection) => {
                ctrl_c_state.disarm_exit();
                return Ok(match selection {
                    InlineListSelection::PlanApprovalExecute => PlanConfirmationOutcome::Execute,
                    InlineListSelection::PlanApprovalAutoAccept => {
                        PlanConfirmationOutcome::AutoAccept
                    }
                    InlineListSelection::PlanApprovalEditPlan
                    | InlineListSelection::PlanApprovalCancel => PlanConfirmationOutcome::EditPlan,
                    _ => PlanConfirmationOutcome::Cancel,
                });
            }
            InlineEvent::ListModalCancel | InlineEvent::Cancel | InlineEvent::Exit => {
                ctrl_c_state.disarm_exit();
                return Ok(PlanConfirmationOutcome::Cancel);
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
