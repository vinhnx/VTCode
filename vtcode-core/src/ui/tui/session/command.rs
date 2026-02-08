use super::Session;
use crate::config::loader::ConfigManager;
use crate::ui::tui::session::config_palette::ConfigPalette;
use crate::ui::tui::session::modal::{ModalListState, ModalSearchState, ModalState};
use crate::ui::tui::types::{
    InlineListSearchConfig, InlineListSelection,
};

use tui_popup::PopupState;

/// Clear the input field and reset related state
pub(super) fn clear_input(session: &mut Session) {
    session.input_manager.clear();
    session.input_compact_mode = false;
    session.scroll_manager.set_offset(0);
    super::slash::update_slash_suggestions(session);
    session.mark_dirty();
}

/// Open the configuration palette
pub fn open_config_palette(session: &mut Session) {
    match ConfigManager::load() {
        Ok(manager) => {
            let palette = ConfigPalette::new(manager);
            session.config_palette = Some(palette);
            session.config_palette_active = true;
            // Disable input while in palette
            session.input_enabled = false;
            session.cursor_visible = false;
            session.mark_dirty();
        }
        Err(e) => {
            // Display error
            let segments = vec![crate::ui::tui::types::InlineSegment {
                text: format!("Failed to load configuration: {}", e),
                style: std::sync::Arc::new(
                    crate::ui::tui::types::InlineTextStyle::default()
                        .with_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red))),
                ),
            }];
            session.push_line(crate::ui::tui::types::InlineMessageKind::Error, segments);
        }
    }
}

/// Show plan confirmation modal.
///
/// Displays the plan markdown and asks for confirmation.
/// User can choose: Execute or stay in plan mode.
pub(crate) fn show_plan_confirmation_modal(
    session: &mut Session,
    plan: crate::ui::tui::types::PlanContent,
) {
    use crate::ui::tui::types::InlineListItem;

    let mut lines: Vec<String> = plan
        .raw_content
        .lines()
        .map(|line| line.to_string())
        .collect();
    if lines.is_empty() && !plan.summary.is_empty() {
        lines.push(plan.summary.clone());
    }

    let items = vec![
        InlineListItem {
            title: "Execute Plan".to_string(),
            selection: Some(InlineListSelection::PlanApprovalExecute),
            ..Default::default()
        },
        InlineListItem {
            title: "Cancel".to_string(),
            selection: Some(InlineListSelection::PlanApprovalCancel),
            ..Default::default()
        },
    ];


    show_list_modal(session, "Plan Confirmation".to_string(), lines, items, None, None);
}

/// Show diff preview modal for file edit approval
pub(super) fn show_diff_preview(
    session: &mut Session,
    file_path: String,
    before: String,
    after: String,
    hunks: Vec<crate::ui::tui::types::DiffHunk>,
    current_hunk: usize,
) {
    use crate::ui::tui::types::DiffPreviewState;

    let mut state = DiffPreviewState::new(file_path, before, after, hunks);
    state.current_hunk = current_hunk;

    session.diff_preview = Some(state);
    session.input_enabled = false;
    session.cursor_visible = false;
    session.mark_dirty();
}

fn show_list_modal(
    session: &mut Session,
    title: String,
    lines: Vec<String>,
    items: Vec<crate::ui::tui::types::InlineListItem>,
    selected: Option<InlineListSelection>,
    search: Option<InlineListSearchConfig>,
) {
    let mut list_state = ModalListState::new(items, selected);
    let search_state = search.map(ModalSearchState::from);
    if let Some(search) = &search_state {
        list_state.apply_search(search.query());
    }
    let state = ModalState {
        title,
        lines,
        list: Some(list_state),
        secure_prompt: None,
        popup_state: PopupState::default(),
        restore_input: session.input_enabled,
        restore_cursor: session.cursor_visible,
        search: search_state,
    };
    session.input_enabled = false;
    session.cursor_visible = false;
    session.modal = Some(state);
    session.mark_dirty();
}
