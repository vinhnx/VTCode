use super::*;
use crate::config::constants::ui;
use crate::core_tui::session::inline_list::{InlineListRow, selection_padding};
use crate::core_tui::session::list_panel::{
    ListPanelLayout, SharedListPanelSections, SharedListPanelStyles, SharedListWidgetModel,
    SharedSearchField, fixed_section_rows, input_styles_from_theme, render_shared_list_panel,
    rows_to_u16,
};
use ratatui::widgets::Clear;

struct HistoryPickerPanelModel {
    entries: Vec<String>,
    selected: Option<usize>,
    offset: usize,
    visible_rows: usize,
    base_style: Style,
    highlight_style: Style,
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

        let dim_style = self.base_style.add_modifier(Modifier::DIM);
        let blank_gutter = selection_padding();

        self.entries
            .iter()
            .enumerate()
            .map(|(idx, content)| {
                let is_selected = self.selected == Some(idx);
                let max_chars = width as usize;
                let item_len = content.chars().count();
                let truncated = if item_len > max_chars {
                    let kept = max_chars.saturating_sub(1);
                    let text: String = content.chars().take(kept).collect();
                    format!("{text}…")
                } else {
                    content.clone()
                };
                let cursor = if is_selected {
                    format!("{} ", ui::MODAL_LIST_HIGHLIGHT_SYMBOL)
                } else {
                    blank_gutter.clone()
                };
                let cursor_style = if is_selected {
                    self.highlight_style
                } else {
                    dim_style
                };
                let text_style = if is_selected {
                    self.highlight_style
                } else {
                    dim_style
                };
                (
                    InlineListRow::single(
                        Line::from(vec![
                            Span::styled(cursor, cursor_style),
                            Span::styled(truncated, text_style),
                        ]),
                        dim_style,
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

pub(crate) fn history_picker_panel_layout(session: &Session) -> Option<ListPanelLayout> {
    if !session.history_picker_visible() || !session.inline_lists_visible() {
        return None;
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

    Some(ListPanelLayout::new(fixed_rows, list_rows))
}

pub fn split_inline_history_picker_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.height == 0 || area.width == 0 {
        session.history_picker_state.navigator.set_visible_rows(0);
        return (area, None);
    }

    let Some(layout) = history_picker_panel_layout(session) else {
        session.history_picker_state.navigator.set_visible_rows(0);
        return (area, None);
    };
    let (transcript_area, panel_area) = layout.split(area);
    if panel_area.is_none() {
        session.history_picker_state.navigator.set_visible_rows(0);
        return (transcript_area, None);
    }
    (transcript_area, panel_area)
}

pub fn render_history_picker(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.height == 0
        || area.width == 0
        || !session.inline_lists_visible()
        || !session.history_picker_visible()
    {
        session.history_picker_state.navigator.set_visible_rows(0);
        return;
    }

    frame.render_widget(Clear, area);

    let (query, selected_idx, current_offset, matches) = {
        let picker = &session.history_picker_state;
        (
            picker.search_query.clone(),
            picker.navigator.selected(),
            picker.navigator.scroll_offset(),
            picker.matches.clone(),
        )
    };
    let default_style = default_style(session);
    let dim_style = default_style.add_modifier(Modifier::DIM);
    let highlight_style = modal_list_highlight_style(session);
    let sections = SharedListPanelSections {
        header: vec![Line::from(Span::styled("History".to_owned(), dim_style))],
        info: vec![Line::from(Span::styled(
            "Ctrl+R open • Enter accept • Esc cancel".to_owned(),
            dim_style,
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
        highlight_style,
    };

    render_shared_list_panel(
        frame,
        area,
        sections,
        SharedListPanelStyles {
            base_style: dim_style,
            selected_style: Some(highlight_style),
            text_style: dim_style,
            divider_style: None,
            input_styles: input_styles_from_theme(&session.core.theme),
        },
        &mut panel_model,
    );

    let picker = &mut session.history_picker_state;
    picker
        .navigator
        .set_visible_rows(panel_model.visible_rows.min(ui::INLINE_LIST_MAX_ROWS));
    picker.navigator.set_selected(panel_model.selected);
    picker.navigator.set_scroll_offset(panel_model.offset);
}
