//! Pure hit-testing for modal lists: maps a terminal row to a visible item index.
//!
//! This mirrors the render geometry so the two cannot drift:
//! - `render_shared_list_panel` reserves summary/info rows above the items
//!   inside the same area, so those rows must not map to an item.
//! - `modal_list_item_lines` row heights include the inline custom-note editor
//!   row, so hit-testing must measure with the same editor input.
//!
//! Both render and hit-test go through [`visible_index_at_row`]; callers only
//! supply the list state that render used (same `footer_hint`, same editor).

use ratatui::layout::Rect;

use super::layout::ModalRenderStyles;
use super::render::{ModalInlineEditor, modal_list_item_lines};
use super::state::ModalListState;
use crate::tui::core_tui::session::inline_list;

/// Map a terminal `row` to a visible list index, or `None` when the row holds
/// no item (summary row, padding past the last item, or outside `area`).
///
/// `footer_hint`, `inline_editor`, and `show_numbers` must match what render
/// passed to `render_modal_list`: plain modals pass their footer hint and no
/// editor, wizard steps pass no hint and the step's [`super::render::inline_editor_for_step`].
/// `is_selected` is always `false` here: it only affects styling, not row
/// height (the blank spacer depends on `item.selection`, not selection state).
/// Number badges render inline without adding rows, so the flag preserves
/// the render/hit-test contract rather than changing geometry.
pub(crate) fn visible_index_at_row(
    list: &ModalListState,
    footer_hint: Option<&str>,
    inline_editor: Option<&ModalInlineEditor>,
    styles: &ModalRenderStyles,
    area: Rect,
    row: u16,
    show_numbers: bool,
    has_status: bool,
) -> Option<usize> {
    if row < area.y || row >= area.y.saturating_add(area.height) {
        return None;
    }
    let content_width = area.width.saturating_sub(inline_list::selection_padding_width() as u16) as usize;
    let info_rows = list.summary_line_rows(footer_hint, has_status);
    let relative_row = usize::from(row.saturating_sub(area.y));
    if relative_row < info_rows {
        return None;
    }
    let relative_row = relative_row.saturating_sub(info_rows);
    let list_height = usize::from(area.height).saturating_sub(info_rows);
    let offset = list.list_state.offset();
    let mut consumed_rows = 0usize;
    // Same one-shot gate as the render path: crowded lists measure without
    // numbers, keeping hit geometry identical to what was painted.
    let numbered = show_numbers && list.numbered_shortcuts();
    for (visible_index, &item_index) in list.visible_indices.iter().enumerate().skip(offset) {
        let number = numbered.then(|| list.shortcut_number(visible_index)).flatten();
        let lines =
            modal_list_item_lines(list, visible_index, item_index, styles, content_width, inline_editor, false, number);
        let height = usize::from(inline_list::row_height(&lines));
        if relative_row < consumed_rows + height {
            return Some(visible_index);
        }
        consumed_rows += height;
        if consumed_rows >= list_height {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::ui::tui::InlineListItem;
    use crate::tui::ui::tui::types::InlineListSelection;
    use ratatui::style::Style;

    fn test_styles() -> ModalRenderStyles {
        ModalRenderStyles {
            border: Style::default(),
            highlight: Style::default(),
            badge: Style::default(),
            header: Style::default(),
            selectable: Style::default(),
            detail: Style::default(),
            search_match: Style::default(),
            title: Style::default(),
            divider: Style::default(),
            background: Style::default(),
            instruction_border: Style::default(),
            instruction_title: Style::default(),
            instruction_bullet: Style::default(),
            instruction_body: Style::default(),
            hint: Style::default(),
            success: Style::default(),
            warning: Style::default(),
            danger: Style::default(),
            accent: Style::default(),
        }
    }

    fn selectable_item(title: &str) -> InlineListItem {
        InlineListItem {
            title: title.to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SlashCommand(title.to_string())),
            search_value: Some(title.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn summary_row_maps_to_none_and_items_shift_down() {
        let styles = test_styles();
        let list = ModalListState::new(vec![selectable_item("a"), selectable_item("b")], None);
        // Adjustable density + footer hint renders one summary row above the items.
        let area = Rect::new(0, 10, 40, 6);

        assert_eq!(visible_index_at_row(&list, Some("hint"), None, &styles, area, 10, false, false), None);
        assert_eq!(visible_index_at_row(&list, Some("hint"), None, &styles, area, 11, false, false), Some(0));
        assert_eq!(visible_index_at_row(&list, Some("hint"), None, &styles, area, 13, false, false), Some(1));
        // Without the hint there is no summary row, so the first item is at the top.
        assert_eq!(visible_index_at_row(&list, None, None, &styles, area, 10, false, false), Some(0));
    }

    #[test]
    fn inline_editor_row_shifts_later_items() {
        let styles = test_styles();
        let mut custom_note = selectable_item("other");
        custom_note.selection = Some(InlineListSelection::RequestUserInputAnswer {
            question_id: "q".to_string(),
            selected: Vec::new(),
            other: Some(String::new()),
        });
        let list = ModalListState::new(vec![custom_note, selectable_item("b")], None);
        let area = Rect::new(0, 10, 40, 8);
        let editor = ModalInlineEditor {
            item_index: 0,
            label: "Other".to_string(),
            text: String::new(),
            placeholder: None,
            active: false,
        };

        // The first item occupies title + editor + padding rows. Without the
        // editor the same rows would (wrongly) map one item lower.
        assert_eq!(visible_index_at_row(&list, None, Some(&editor), &styles, area, 10, false, false), Some(0));
        assert_eq!(visible_index_at_row(&list, None, Some(&editor), &styles, area, 11, false, false), Some(0));
        assert_eq!(visible_index_at_row(&list, None, Some(&editor), &styles, area, 12, false, false), Some(0));
        assert_eq!(visible_index_at_row(&list, None, Some(&editor), &styles, area, 13, false, false), Some(1));
        assert_eq!(visible_index_at_row(&list, None, None, &styles, area, 12, false, false), Some(1));
    }

    #[test]
    fn rows_outside_area_map_to_none() {
        let styles = test_styles();
        let list = ModalListState::new(vec![selectable_item("a")], None);
        let area = Rect::new(0, 10, 40, 3);

        assert_eq!(visible_index_at_row(&list, None, None, &styles, area, 9, false, false), None);
        assert_eq!(visible_index_at_row(&list, None, None, &styles, area, 13, false, false), None);
    }

    #[test]
    fn numbered_badges_do_not_shift_hit_geometry() {
        // Badges render inline without adding rows: the same rows must map
        // identically with numbers shown or hidden.
        let styles = test_styles();
        let list = ModalListState::new(vec![selectable_item("a"), selectable_item("b")], None);
        let area = Rect::new(0, 10, 40, 6);
        for row in 10..16 {
            assert_eq!(
                visible_index_at_row(&list, None, None, &styles, area, row, true, false),
                visible_index_at_row(&list, None, None, &styles, area, row, false, false),
                "row {row} must map identically"
            );
        }
        assert_eq!(visible_index_at_row(&list, None, None, &styles, area, 10, true, false), Some(0));
        assert_eq!(visible_index_at_row(&list, None, None, &styles, area, 12, true, false), Some(1));
    }

    #[test]
    fn summary_line_rows_count_status_strip() {
        let items = vec![InlineListItem {
            title: "a".to_string(),
            selection: Some(InlineListSelection::SlashCommand("a".to_string())),
            ..Default::default()
        }];
        let list = ModalListState::new(items, None);
        assert_eq!(list.summary_line_rows(Some("hint"), false), 1);
        assert_eq!(list.summary_line_rows(Some("hint"), true), 2);
        assert_eq!(list.summary_line_rows(None, true), 1);
    }
}
