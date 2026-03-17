use super::*;
use ratatui::widgets::Clear;
use crate::ui::tui::session::inline_list::InlineListRow;
use crate::ui::tui::session::list_panel::{
    SharedListPanelSections, SharedListPanelStyles, SharedListWidgetModel, SharedSearchField,
    fixed_section_rows, render_shared_list_panel, rows_to_u16, split_bottom_list_panel,
};

struct HistoryPickerPanelModel {
    entries: Vec<String>,
    selected: Option<usize>,
    offset: usize,
    visible_rows: usize,
    base_style: Style,
}

impl SharedListWidgetModel for HistoryPickerPanelModel {
    fn rows(&self, width: u16) -> Vec<(InlineListRow, u16)> {
        if self.entries.is_empty() {
            return vec![(
                InlineListRow::single(
                    Line::from(Span::styled(
                        "No history matches".to_owned(),
                        self.base_style
                            .add_modifier(Modifier::DIM | Modifier::ITALIC),
                    )),
                    self.base_style.add_modifier(Modifier::DIM),
                ),
                1_u16,
            )];
        }

        self.entries
            .iter()
            .map(|content| {
                let max_chars = width as usize;
                let item_len = content.chars().count();
                let truncated = if item_len > max_chars {
                    let kept = max_chars.saturating_sub(1);
                    let text: String = content.chars().take(kept).collect();
                    format!("{text}…")
                } else {
                    content.clone()
                };
                (
                    InlineListRow::single(
                        Line::from(vec![Span::styled(truncated, self.base_style)]),
                        self.base_style,
                    ),
                    1_u16,
                )
            })
            .collect()
    }

    fn selected(&self) -> Option<usize> {
        self.selected
    }

    fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }

    fn set_scroll_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    fn set_viewport_rows(&mut self, rows: u16) {
        self.visible_rows = rows as usize;
    }
}

pub fn split_inline_history_picker_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.height == 0
        || area.width == 0
        || session.has_active_overlay()
        || !session.inline_lists_visible()
        || session.file_palette_active
        || !session.history_picker_state.active
    {
        session.history_picker_state.visible_rows = 0;
        return (area, None);
    }

    let fixed_rows = fixed_section_rows(1, 1, true);
    let list_rows = if session.history_picker_state.matches.is_empty() {
        1_u16
    } else {
        rows_to_u16(
            session
                .history_picker_state
                .matches
                .len()
                .min(ui::INLINE_LIST_MAX_ROWS),
        )
    };
    let (transcript_area, panel_area) = split_bottom_list_panel(area, fixed_rows, list_rows);
    if panel_area.is_none() {
        session.history_picker_state.visible_rows = 0;
        return (transcript_area, None);
    }
    (transcript_area, panel_area)
}

pub fn render_history_picker(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.height == 0
        || area.width == 0
        || !session.inline_lists_visible()
        || !session.history_picker_state.active
        || session.has_active_overlay()
        || session.file_palette_active
    {
        session.history_picker_state.visible_rows = 0;
        return;
    }

    frame.render_widget(Clear, area);

    let (query, selected_idx, current_offset, matches) = {
        let picker = &session.history_picker_state;
        (
            picker.search_query.clone(),
            picker.list_state.selected(),
            picker.list_state.offset(),
            picker.matches.clone(),
        )
    };
    let default_style = default_style(session);
    let sections = SharedListPanelSections {
        header: vec![Line::from(Span::styled(
            "History".to_owned(),
            default_style,
        ))],
        info: vec![Line::from(Span::styled(
            "Ctrl+R open • Enter accept • Esc cancel".to_owned(),
            default_style,
        ))],
        search: Some(SharedSearchField {
            label: "Search history".to_owned(),
            placeholder: Some("history text".to_owned()),
            query,
        }),
    };

    let entries = matches
        .into_iter()
        .map(|item| item.content)
        .collect::<Vec<_>>();
    let mut panel_model = HistoryPickerPanelModel {
        entries,
        selected: selected_idx,
        offset: current_offset,
        visible_rows: 0,
        base_style: default_style,
    };

    render_shared_list_panel(
        frame,
        area,
        sections,
        SharedListPanelStyles {
            base_style: default_style,
            selected_style: Some(modal_list_highlight_style(session)),
            text_style: default_style,
        },
        &mut panel_model,
    );

    let picker = &mut session.history_picker_state;
    picker.visible_rows = panel_model.visible_rows.min(ui::INLINE_LIST_MAX_ROWS);
    picker.list_state.select(panel_model.selected);
    *picker.list_state.offset_mut() = panel_model.offset;
}
