//! Plan confirmation HITL flow for Plan -> Edit execution.
//!
//! This implementation routes plan confirmation through the shared overlay driver.

use std::sync::Arc;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::sync::Notify;

use vtcode_ui::tui::app::{
    InlineHandle, InlineListItem, InlineListSelection, InlineMessageKind, InlineSession, ListOverlayRequest,
    PlanContent, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey, TransientRequest, TransientSubmission,
};

use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;

/// Decision returned by the "enter Planning workflow?" confirmation prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartPlanningDecision {
    /// User chose to enter the Planning workflow.
    Enter,
    /// User chose to continue without the Planning workflow.
    Stay,
}

const START_PLANNING_APPROVE_ACTION: &str = "planning:start";
const START_PLANNING_STAY_ACTION: &str = "planning:stay";

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
) -> Result<StartPlanningDecision> {
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
            InlineListItem {
                title: "Enter Planning workflow".to_string(),
                subtitle: Some("Enter the Planning workflow and persist the draft under .vtcode/plans.".to_string()),
                badge: Some("Recommended".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(START_PLANNING_APPROVE_ACTION.to_string())),
                search_value: None,
            },
            InlineListItem {
                title: "Continue without Planning workflow".to_string(),
                subtitle: Some("Continue without entering the Planning workflow.".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(START_PLANNING_STAY_ACTION.to_string())),
                search_value: None,
            },
        ],
        selected: Some(InlineListSelection::ConfigAction(START_PLANNING_APPROVE_ACTION.to_string())),
        search: None,
        hotkeys: Vec::new(),
    });

    let confirmation =
        show_overlay_and_wait(handle, session, overlay, ctrl_c_state, ctrl_c_notify, |submission| match submission {
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
        })
        .await;

    Ok(match confirmation {
        Ok(OverlayWaitOutcome::Submitted(choice)) => choice,
        Ok(OverlayWaitOutcome::Cancelled)
        | Ok(OverlayWaitOutcome::Interrupted)
        | Ok(OverlayWaitOutcome::Exit)
        | Err(_) => StartPlanningDecision::Stay,
    })
}

/// Result of the plan confirmation flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlanConfirmationOutcome {
    /// User approved execution with manual edit approvals
    Execute,
    /// User approved with auto-accept enabled for future confirmations
    AutoAccept,
    /// User wants to edit the plan
    EditPlan,
    /// User cancelled
    Cancel,
    /// User chose to hand off execution to the build primary agent.
    SwitchBuild,
    /// User chose to hand off execution to the auto primary agent.
    SwitchAuto,
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
    append_message(handle, InlineMessageKind::Info, "A plan is ready to execute. Would you like to proceed?");

    // Keep confirmation compact to avoid duplicating the already-rendered plan content.
    if !plan.summary.trim().is_empty() {
        append_message(handle, InlineMessageKind::Agent, plan.summary.clone());
    } else if !plan.title.trim().is_empty() {
        append_message(handle, InlineMessageKind::Info, format!("Plan: {}", plan.title));
    }

    if let Some(path) = plan.file_path.as_deref()
        && !path.trim().is_empty()
    {
        append_message(handle, InlineMessageKind::Info, format!("Plan file: {path}"));
    }
    append_message(
        handle,
        InlineMessageKind::Info,
        "Use the confirmation list to choose auto-accept, manual approve, or revise.",
    );
}

/// Render a robust, structured summary of the plan for the confirmation overlay.
///
/// Prefers the parsed `phases`/`steps` shape so the plan reads as a clear,
/// scannable checklist. Falls back to the raw content or summary when the
/// structured data is absent, so a malformed or partially synthesized plan
/// still renders something useful instead of a blank panel.
fn render_structured_plan(plan: &PlanContent) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    if !plan.title.trim().is_empty() {
        lines.push(plan.title.trim().to_string());
        lines.push(String::new());
    }

    if !plan.summary.trim().is_empty() {
        for line in plan.summary.trim().lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new());
    }

    let has_phases = plan.phases.iter().any(|phase| !phase.steps.is_empty());
    if has_phases {
        for phase in &plan.phases {
            if phase.steps.is_empty() {
                continue;
            }
            if !phase.name.trim().is_empty() {
                lines.push(format!("## {}", phase.name.trim()));
            }
            for step in &phase.steps {
                let marker = if step.completed { "[x]" } else { "[ ]" };
                lines.push(format!("{} {} {}", marker, step.number, step.description));
                if let Some(details) = step.details.as_ref().filter(|d| !d.trim().is_empty()) {
                    for detail_line in details.lines() {
                        lines.push(format!("      {detail_line}"));
                    }
                }
                if !step.files.is_empty() {
                    lines.push(format!("      files: {}", step.files.join(", ")));
                }
            }
            lines.push(String::new());
        }
    } else if !plan.raw_content.trim().is_empty() {
        for line in plan.raw_content.lines() {
            lines.push(line.to_string());
        }
        lines.push(String::new());
    }

    if !plan.open_questions.is_empty() {
        lines.push("Open questions:".to_string());
        for question in &plan.open_questions {
            lines.push(format!("- {question}"));
        }
        lines.push(String::new());
    }

    if lines.is_empty() {
        lines.push(plan.title.trim().to_string());
    }

    lines
}

fn build_plan_confirmation_request(plan: &PlanContent, draft_incomplete: bool) -> TransientRequest {
    let mut lines: Vec<String> = render_structured_plan(plan);
    lines.insert(0, "A plan is ready to execute. Would you like to proceed?".to_string());

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
            subtitle: Some("Return to planning workflow and refine the plan.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalEditPlan),
            search_value: None,
        },
        InlineListItem {
            title: "Switch to build agent".to_string(),
            subtitle: Some("Hand off to the build agent to execute the plan with manual edit approvals.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalSwitchBuild),
            search_value: None,
        },
        InlineListItem {
            title: "Switch to auto agent".to_string(),
            subtitle: Some(
                "Hand off to the auto agent to auto-execute the plan (skip per-step confirmations).".to_string(),
            ),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalSwitchAuto),
            search_value: None,
        },
    ];

    let selected = if draft_incomplete {
        InlineListSelection::PlanApprovalEditPlan
    } else {
        InlineListSelection::PlanApprovalAutoAccept
    };

    TransientRequest::List(ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines,
        footer_hint,
        items,
        selected: Some(selected),
        search: None,
        hotkeys: vec![TransientHotkey {
            key: TransientHotkeyKey::CtrlChar('g'),
            action: TransientHotkeyAction::LaunchEditor,
        }],
    })
}

/// Map a plan-confirmation overlay submission to its [`PlanConfirmationOutcome`].
///
/// Kept as a pure function so the selection→outcome mapping (including the
/// `SwitchBuild`/`SwitchAuto` handoff outcomes) is unit-testable without the
/// TUI driver. The `LaunchEditor` hotkey maps to `EditPlan`; any other
/// unrecognized selection cancels.
pub(crate) fn plan_confirmation_submission_to_outcome(
    submission: &TransientSubmission,
) -> Option<PlanConfirmationOutcome> {
    match submission {
        TransientSubmission::Selection(InlineListSelection::PlanApprovalExecute) => {
            Some(PlanConfirmationOutcome::Execute)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalAutoAccept) => {
            Some(PlanConfirmationOutcome::AutoAccept)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalEditPlan) => {
            Some(PlanConfirmationOutcome::EditPlan)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalSwitchBuild) => {
            Some(PlanConfirmationOutcome::SwitchBuild)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalSwitchAuto) => {
            Some(PlanConfirmationOutcome::SwitchAuto)
        }
        TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor) => Some(PlanConfirmationOutcome::EditPlan),
        TransientSubmission::Selection(_) => Some(PlanConfirmationOutcome::Cancel),
        _ => None,
    }
}

/// Execute the plan confirmation HITL flow after finish_planning tool.
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
    render_confirmation_prompt(handle, &plan_content);
    let outcome = show_overlay_and_wait(
        handle,
        session,
        build_plan_confirmation_request(&plan_content, draft_incomplete),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| {
            if let TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor) = submission {
                handle.set_input("/edit".to_string());
            }
            plan_confirmation_submission_to_outcome(&submission)
        },
    )
    .await?;

    Ok(match outcome {
        OverlayWaitOutcome::Submitted(outcome) => outcome,
        OverlayWaitOutcome::Cancelled | OverlayWaitOutcome::Interrupted | OverlayWaitOutcome::Exit => {
            PlanConfirmationOutcome::Cancel
        }
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
            "action": "stay_in_planning_workflow",
            "message": "User wants to edit the plan. Remain in planning workflow and await further instructions."
        }),
        PlanConfirmationOutcome::SwitchBuild => json!({
            "status": "approved",
            "action": "switch_to_build_agent",
            "message": "User handed off the plan to the build agent. Switch primary agent and execute."
        }),
        PlanConfirmationOutcome::SwitchAuto => json!({
            "status": "approved",
            "action": "switch_to_auto_agent",
            "message": "User handed off the plan to the auto agent. Switch primary agent and execute with per-step HITL."
        }),
        PlanConfirmationOutcome::Cancel => json!({
            "status": "cancelled",
            "action": "cancel",
            "message": "User cancelled the plan. Do not proceed with implementation."
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PlanConfirmationOutcome, build_plan_confirmation_request, plan_confirmation_outcome_to_json,
        plan_confirmation_submission_to_outcome, render_structured_plan,
    };
    use vtcode_ui::tui::app::{
        InlineListSelection, ListOverlayRequest, TransientHotkeyAction, TransientRequest, TransientSubmission,
    };
    use vtcode_ui::tui::app::{PlanContent, PlanPhase, PlanStep};

    fn sample_plan() -> PlanContent {
        PlanContent {
            title: "Add retry to synthesize".to_string(),
            summary: "Make plan-mode synthesize resilient to transient errors.".to_string(),
            file_path: Some("docs/plan.md".to_string()),
            phases: vec![PlanPhase {
                name: "Phase 1: Resilience".to_string(),
                completed: false,
                steps: vec![
                    PlanStep {
                        number: 1,
                        description: "Wrap generate in retry".to_string(),
                        details: Some("Use RetryPolicy::default()".to_string()),
                        files: vec!["src/a.rs".to_string()],
                        completed: false,
                    },
                    PlanStep {
                        number: 2,
                        description: "Add tests".to_string(),
                        details: None,
                        files: vec![],
                        completed: false,
                    },
                ],
            }],
            open_questions: vec!["Should we cap retries?".to_string()],
            raw_content: "RAW fallback content".to_string(),
            total_steps: 2,
            completed_steps: 0,
        }
    }

    // --- B: render_structured_plan ----------------------------------------

    #[test]
    fn render_structured_plan_prefers_phases_over_raw() {
        let lines = render_structured_plan(&sample_plan());
        let joined = lines.join("\n");
        assert!(joined.contains("## Phase 1: Resilience"));
        assert!(joined.contains("[ ] 1 Wrap generate in retry"));
        assert!(joined.contains("Use RetryPolicy::default()"));
        assert!(joined.contains("files: src/a.rs"));
        assert!(joined.contains("Open questions:"));
        assert!(joined.contains("Should we cap retries?"));
        assert!(!joined.contains("RAW fallback content"), "structured phases must take precedence over raw_content");
    }

    #[test]
    fn render_structured_plan_falls_back_to_raw_content() {
        let mut plan = sample_plan();
        plan.phases = vec![];
        let lines = render_structured_plan(&plan);
        let joined = lines.join("\n");
        assert!(joined.contains("RAW fallback content"));
        assert!(joined.contains("Make plan-mode synthesize resilient"));
        assert!(!joined.contains("## Phase 1"));
    }

    #[test]
    fn render_structured_plan_falls_back_to_title_when_empty() {
        let plan = PlanContent {
            title: "Only a title".to_string(),
            summary: String::new(),
            file_path: None,
            phases: vec![],
            open_questions: vec![],
            raw_content: String::new(),
            total_steps: 0,
            completed_steps: 0,
        };
        assert_eq!(render_structured_plan(&plan), vec!["Only a title".to_string(), String::new()]);
    }

    // --- C: switch outcomes (json, submission mapping, request items) -------

    #[test]
    fn plan_confirmation_outcome_to_json_emits_switch_actions() {
        let build = plan_confirmation_outcome_to_json(&PlanConfirmationOutcome::SwitchBuild);
        assert_eq!(build["status"], "approved");
        assert_eq!(build["action"], "switch_to_build_agent");

        let auto = plan_confirmation_outcome_to_json(&PlanConfirmationOutcome::SwitchAuto);
        assert_eq!(auto["status"], "approved");
        assert_eq!(auto["action"], "switch_to_auto_agent");

        let execute = plan_confirmation_outcome_to_json(&PlanConfirmationOutcome::Execute);
        assert_eq!(execute["action"], "execute");
    }

    #[test]
    fn plan_confirmation_submission_maps_switch_outcomes() {
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalSwitchBuild
            )),
            Some(PlanConfirmationOutcome::SwitchBuild)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalSwitchAuto
            )),
            Some(PlanConfirmationOutcome::SwitchAuto)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalExecute
            )),
            Some(PlanConfirmationOutcome::Execute)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalAutoAccept
            )),
            Some(PlanConfirmationOutcome::AutoAccept)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::PlanApprovalEditPlan
            )),
            Some(PlanConfirmationOutcome::EditPlan)
        );
        // Unrecognized selection cancels.
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Selection(
                InlineListSelection::ConfigAction("x".to_string())
            )),
            Some(PlanConfirmationOutcome::Cancel)
        );
        assert_eq!(
            plan_confirmation_submission_to_outcome(&TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor)),
            Some(PlanConfirmationOutcome::EditPlan)
        );
    }

    #[test]
    fn plan_confirmation_request_includes_switch_items() {
        let req = build_plan_confirmation_request(&sample_plan(), false);
        let ListOverlayRequest { items, .. } = match req {
            TransientRequest::List(list) => list,
            _ => panic!("expected a list overlay request"),
        };
        let selections: Vec<InlineListSelection> = items.into_iter().filter_map(|item| item.selection).collect();
        assert!(
            selections
                .iter()
                .any(|s| matches!(s, InlineListSelection::PlanApprovalSwitchBuild))
        );
        assert!(
            selections
                .iter()
                .any(|s| matches!(s, InlineListSelection::PlanApprovalSwitchAuto))
        );
        assert!(selections.iter().any(|s| matches!(s, InlineListSelection::PlanApprovalExecute)));
    }
}
