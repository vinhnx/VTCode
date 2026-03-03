use super::super::types::{DiffHunk, InlineListItem, InlineListSelection, PlanContent};
use super::{
    Session,
    modal::{ModalListState, ModalState},
};

fn mark_dirty(session: &mut Session) {
    session.needs_redraw = true;
}

pub(super) fn clear_input(session: &mut Session) {
    session.input_manager.clear();
    session.input_compact_mode = false;
    session.scroll_manager.set_offset(0);
    super::slash::update_slash_suggestions(session);
    session.mark_dirty();
}

/// Show plan confirmation modal.
///
/// Displays the plan markdown and asks for confirmation.
/// User can choose from execute variants or return to plan editing.
pub(crate) fn show_plan_confirmation_modal(session: &mut Session, plan: PlanContent) {
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
            title: "Yes, auto-accept edits (Recommended)".to_string(),
            subtitle: Some("Keep context and execute with auto-approval.".to_string()),
            badge: None,
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
            title: "No, stay in Plan mode".to_string(),
            subtitle: Some("Keep planning without executing yet.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalCancel),
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

    let list_state = ModalListState::new(items, Some(InlineListSelection::PlanApprovalAutoAccept));

    session.modal = Some(ModalState {
        title: "Ready to code?".to_string(),
        lines,
        footer_hint,
        list: Some(list_state),
        secure_prompt: None,
        is_plan_confirmation: true,
        restore_input: session.input_enabled,
        restore_cursor: session.cursor_visible,
        search: None,
    });
    session.input_enabled = false;
    session.cursor_visible = false;
    mark_dirty(session);
}

/// Show diff preview modal for file edit approval.
pub(super) fn show_diff_preview(
    session: &mut Session,
    file_path: String,
    before: String,
    after: String,
    hunks: Vec<DiffHunk>,
    current_hunk: usize,
) {
    let mut state = crate::ui::tui::types::DiffPreviewState::new(file_path, before, after, hunks);
    state.current_hunk = current_hunk;

    session.diff_preview = Some(state);
    session.input_enabled = false;
    session.cursor_visible = false;
    mark_dirty(session);
}
