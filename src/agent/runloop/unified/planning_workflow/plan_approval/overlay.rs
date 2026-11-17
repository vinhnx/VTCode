//! Plan-approval overlay construction and submission mapping.
//!
//! Pure functions: the synopsis, the decision list (with context-dependent
//! choices), and the selection→outcome mapping. Kept separate from the async
//! HITL driver in `mod.rs` so each can be tested without the TUI runtime.

use super::*;

// The confirmation prompt header consumes one of the modal's instruction
// rows, so the summary plus steps share the remaining budget. Entries keep
// their full text and wrap to the modal width (never cut mid-sentence with
// an ellipsis); only the step *count* is bounded, with omitted steps
// collapsed into the explicit `… and N more plan steps` overflow row.
// Budgeting uses estimated visual rows at a conservative 76-column content
// width so a long summary may span up to four wrapped rows (per user
// approval) while steps yield room instead of clipping the viewport.
const PLAN_PREVIEW_MAX_LINES: usize = 5;
const PLAN_PREVIEW_VISUAL_BUDGET_ROWS: usize = 7;
const PLAN_SUMMARY_MAX_VISUAL_ROWS: usize = 4;
const PLAN_PREVIEW_ESTIMATED_CONTENT_WIDTH: usize = 76;

/// Collapse internal whitespace so a plan entry stays on one raw modal line.
/// Wrapping (not truncation) handles narrow widths downstream.
fn collapse_plan_line(text: &str) -> String {
    vtcode_commons::formatting::collapse_whitespace(text)
}

/// Estimate wrapped visual rows for a single raw modal line at the
/// conservative preview width. Real terminals are usually wider, so actual
/// wrapped rows are at most the estimate and the budget never over-promises.
fn estimated_plan_visual_rows(text: &str) -> usize {
    text.chars().count().div_ceil(PLAN_PREVIEW_ESTIMATED_CONTENT_WIDTH).max(1)
}

fn numbered_step_description(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let (number, description) = trimmed.split_once('.')?;
    if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }

    let description = description.trim();
    (!description.is_empty()).then(|| format!("{number}. {description}"))
}

/// Render a bounded, decision-ready plan synopsis for the inline approval UI.
///
/// The plan file and plan events retain the complete markdown. The approval
/// modal has a small instruction viewport, so it receives the full summary
/// overview paragraph plus as many numbered steps as fit a seven visual-row
/// budget (the header takes one more row). Entries keep their full text and
/// wrap to the modal width; only the step *count* is bounded, with omitted
/// steps collapsed into an explicit `… and N more plan steps` row. The
/// `Summary:` row renders as a label/value overview section (no bullet) so
/// users grasp the change without reading every step.
pub(super) fn render_plan_summary(plan: &PlanContent) -> Vec<String> {
    let mut lines = Vec::new();
    let mut used_visual_rows = 0usize;
    if !plan.summary.trim().is_empty() {
        let summary_line = format!("Summary: {}", collapse_plan_line(&plan.summary));
        used_visual_rows = estimated_plan_visual_rows(&summary_line).min(PLAN_SUMMARY_MAX_VISUAL_ROWS);
        lines.push(summary_line);
    }

    let structured_steps: Vec<String> = plan
        .phases
        .iter()
        .flat_map(|phase| phase.steps.iter())
        .map(|step| format!("{}. {}", step.number, step.description))
        .collect();
    let step_lines = if structured_steps.is_empty() {
        plan.raw_content.lines().filter_map(numbered_step_description).collect()
    } else {
        structured_steps
    };

    // Fill steps while both the raw-line cap and the visual-row budget hold,
    // reserving one row for the overflow evidence row when steps remain.
    let mut visible_step_count = 0usize;
    while visible_step_count < step_lines.len() {
        let is_last = visible_step_count + 1 == step_lines.len();
        let prospective_raw = lines.len() + visible_step_count + 1 + usize::from(!is_last);
        if prospective_raw > PLAN_PREVIEW_MAX_LINES {
            break;
        }
        let reserve_visual_rows = usize::from(!is_last);
        if used_visual_rows + estimated_plan_visual_rows(&step_lines[visible_step_count]) + reserve_visual_rows
            > PLAN_PREVIEW_VISUAL_BUDGET_ROWS
        {
            break;
        }
        used_visual_rows += estimated_plan_visual_rows(&step_lines[visible_step_count]);
        visible_step_count += 1;
    }
    lines.extend(step_lines.iter().take(visible_step_count).map(|line| collapse_plan_line(line)));
    if visible_step_count < step_lines.len() {
        lines.push(format!("… and {} more plan steps", step_lines.len() - visible_step_count));
    }

    if lines.is_empty() {
        let fallback = if plan.title.trim().is_empty() {
            "Plan details are available in the plan file.".to_string()
        } else {
            format!("Plan: {}", plan.title.trim())
        };
        lines.push(collapse_plan_line(&fallback));
    }

    lines
}

/// Build the approval overlay. Two choices are context-dependent: clearing
/// context is pointless at 0% usage, and switching to Auto is pointless when
/// Auto is already the active primary agent. Hotkey numbers follow item order,
/// so removing either choice renumbers the remaining ones automatically.
pub(super) fn build_plan_confirmation_request_with_context(
    plan: &PlanContent,
    draft_incomplete: bool,
    context_usage_percent: u8,
    auto_agent_active: bool,
) -> TransientRequest {
    tracing::info!(
        target: "vtcode.planning_workflow",
        draft_incomplete,
        "build_plan_confirmation_request: building overlay request"
    );
    let mut lines = render_plan_summary(plan);
    lines.insert(0, "A plan is ready to execute. Would you like to proceed?".to_string());

    let footer_hint = plan
        .file_path
        .as_ref()
        .map(|path| format!("ctrl-g to edit in VS Code · {path}"));
    let fresh_recommended = context_usage_percent >= 50;
    let mut items = vec![InlineListItem {
        title: "Yes, implement this plan".to_string(),
        subtitle: Some("Continue with the current context and confirmation policy.".to_string()),
        badge: (!fresh_recommended).then(|| "Recommended".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::PlanApprovalExecute),
        search_value: None,
        ..Default::default()
    }];
    if context_usage_percent > 0 {
        items.push(InlineListItem {
            title: "Yes, clear context and implement".to_string(),
            subtitle: Some(format!("Fresh thread. Context: {context_usage_percent}% used.")),
            badge: fresh_recommended.then(|| "Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalFreshContext),
            search_value: None,
            ..Default::default()
        });
    }
    if !auto_agent_active {
        let autonomous_agent = vtcode_config::builtin_primary_auto_agent();
        items.push(InlineListItem {
            title: format!("Yes, switch to {} and implement", autonomous_agent.display_name()),
            subtitle: Some(autonomous_agent.description),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalSwitchAuto),
            search_value: None,
            ..Default::default()
        });
    }
    items.push(InlineListItem {
        title: "No, stay in Plan mode".to_string(),
        subtitle: Some("Return to planning and revise the plan.".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::PlanApprovalEditPlan),
        search_value: None,
        ..Default::default()
    });

    let selected = if draft_incomplete {
        InlineListSelection::PlanApprovalEditPlan
    } else if fresh_recommended {
        InlineListSelection::PlanApprovalFreshContext
    } else {
        InlineListSelection::PlanApprovalExecute
    };

    // Non-terminal hosts never see the transcript copy, so they receive the
    // same normalized markdown the TUI appends there. Empty-after-cleanup
    // yields `None`: clients hide expansion UI instead of showing a blank.
    let (display_markdown, _warnings) =
        crate::agent::runloop::unified::plan_blocks::prepare_plan_markdown_for_display(&plan.raw_content);
    let plan_markdown = (!display_markdown.trim().is_empty()).then_some(display_markdown);

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
        status: None,
        plan_markdown,
    })
}

/// Map a plan-confirmation overlay submission to its [`PlanConfirmationOutcome`].
///
/// Kept as a pure function so the selection→outcome mapping (including the
/// `SwitchBuild`/`SwitchAuto` handoff outcomes) is unit-testable without the
/// TUI driver. `execute_plan_confirmation_with_context` intercepts the
/// `LaunchEditor` hotkey before this mapping to open the plan file; the
/// `EditPlan` fallback below is retained so a missed interception still keeps
/// the user in Plan mode rather than silently executing. Any other
/// unrecognized selection cancels.
pub(super) fn plan_confirmation_submission_to_outcome(
    submission: &TransientSubmission,
) -> Option<PlanConfirmationOutcome> {
    match submission {
        TransientSubmission::Selection(InlineListSelection::PlanApprovalExecute) => {
            Some(PlanConfirmationOutcome::Execute)
        }
        TransientSubmission::Selection(InlineListSelection::PlanApprovalFreshContext) => {
            Some(PlanConfirmationOutcome::FreshContext)
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

/// Internal wait result: a real user decision, or a request to open the plan
/// draft in the configured external editor and then re-show the approval
/// overlay with any saved edits.
pub(super) enum PlanConfirmationWait {
    Outcome(PlanConfirmationOutcome),
    EditInExternalEditor,
}

/// Map an approval-overlay submission to its internal wait result.
/// Shared by the overlay-first initial wait and the re-show path after an
/// external-editor edit so the two cannot diverge.
pub(super) fn map_plan_confirmation_submission(submission: TransientSubmission) -> Option<PlanConfirmationWait> {
    match submission {
        TransientSubmission::Hotkey(TransientHotkeyAction::LaunchEditor) => {
            Some(PlanConfirmationWait::EditInExternalEditor)
        }
        other => plan_confirmation_submission_to_outcome(&other).map(PlanConfirmationWait::Outcome),
    }
}
