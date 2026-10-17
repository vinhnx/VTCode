use super::*;
use crate::tui::config::constants::ui;
use crate::tui::core_tui::session::inline_list::{InlineListRow, list_cursor, selection_padding_width};
use crate::tui::core_tui::session::list_panel::{
    ListPanelLayout, SharedListPanelSections, SharedListPanelStyles, SharedListWidgetModel, SharedSearchField,
    fixed_section_rows, input_styles_from_theme, render_shared_list_panel, rows_to_u16,
};
use ratatui::widgets::Clear;

/// Collapsed single-line display for a history entry.
///
/// Shows the first line only, with `· <time>` and `· +N lines` suffixes.
/// Control characters from the original command are sanitized so embedded
/// newlines/tabs can never break the single-row list layout. The full
/// multiline content is preserved in `HistoryMatch::content` and restored
/// into the composer on accept.
pub(crate) fn collapsed_history_display(content: &str, time_label: &str) -> String {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    let extra_lines = lines.count();
    let sanitized: String = first.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    let first = if sanitized.is_empty() && extra_lines > 0 {
        "…"
    } else {
        sanitized.as_str()
    };
    let mut parts = vec![first.to_owned()];
    if !time_label.is_empty() {
        parts.push(time_label.to_owned());
    }
    if extra_lines > 0 {
        if extra_lines == 1 {
            parts.push("+1 line".to_owned());
        } else {
            parts.push(format!("+{extra_lines} lines"));
        }
    }
    parts.join(" \u{b7} ")
}

struct HistoryPickerPanelModel {
    entries: Vec<(String, String)>,
    selected: Option<usize>,
    offset: usize,
    visible_rows: usize,
    highlight_style: Style,
    /// Muted-by-color row style (theme secondary), never `Modifier::DIM` —
    /// ratatui's `Cell::set_style` only inserts modifiers, so a DIM painted
    /// over the panel sticks to every glyph drawn on top of it.
    muted_style: Style,
}

impl SharedListWidgetModel for HistoryPickerPanelModel {
    fn rows(&self, width: u16) -> Vec<(InlineListRow, u16)> {
        if self.entries.is_empty() {
            return vec![(
                InlineListRow::single(
                    Line::from(Span::styled(
                        "No history matches".to_owned(),
                        self.muted_style.add_modifier(Modifier::ITALIC),
                    )),
                    self.muted_style,
                ),
                1_u16,
            )];
        }

        let muted_style = self.muted_style;

        self.entries
            .iter()
            .enumerate()
            .map(|(idx, (content, time_label))| {
                let is_selected = self.selected == Some(idx);
                // Reserve space for the "┃ " selection gutter so rows never
                // overflow the panel width and wrap.
                let max_chars = (width as usize).saturating_sub(selection_padding_width());

                // Collapsed display: first line only + "· +N lines" indicator.
                // Full multiline content is restored into the composer on accept.
                let display_text = collapsed_history_display(content, time_label);

                let item_len = display_text.chars().count();
                let truncated = if item_len > max_chars {
                    let kept = max_chars.saturating_sub(1);
                    let text: String = display_text.chars().take(kept).collect();
                    format!("{text}…")
                } else {
                    display_text
                };
                let cursor = list_cursor(is_selected);
                let cursor_style = if is_selected { self.highlight_style } else { muted_style };
                let text_style = if is_selected { self.highlight_style } else { muted_style };
                (
                    InlineListRow::single(
                        Line::from(vec![Span::styled(cursor, cursor_style), Span::styled(truncated, text_style)]),
                        muted_style,
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

    let fixed_rows = fixed_section_rows(1, 1, 1);
    let list_rows = if session.history_picker_state.matches.is_empty() {
        1_u16
    } else {
        rows_to_u16(session.history_picker_state.matches.len().min(ui::INLINE_LIST_MAX_ROWS))
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
    if area.height == 0 || area.width == 0 || !session.inline_lists_visible() || !session.history_picker_visible() {
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
    let muted_style = session.core.styles.muted_text_style();
    let highlight_style = modal_list_highlight_style(session);
    let sections = SharedListPanelSections {
        header: vec![Line::from(Span::styled("History".to_owned(), highlight_style))],
        info: vec![Line::from(Span::styled(
            "↑↓ Navigate · Enter accept · Esc cancel".to_owned(),
            default_style,
        ))],
        search: Some(SharedSearchField {
            label: String::new(),
            placeholder: Some("history text".to_owned()),
            query,
        }),
    };

    let entries = matches
        .into_iter()
        .map(|item| (item.content, item.time_label))
        .collect::<Vec<_>>();
    let mut panel_model = HistoryPickerPanelModel {
        entries,
        selected: selected_idx,
        offset: current_offset,
        visible_rows: 0,
        highlight_style,
        muted_style,
    };

    render_shared_list_panel(
        frame,
        area,
        sections,
        SharedListPanelStyles {
            base_style: default_style,
            selected_style: Some(highlight_style),
            text_style: default_style,
            divider_style: None,
            input_styles: input_styles_from_theme(&session.core.theme),
            show_divider: false,
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

#[cfg(test)]
mod tests {
    use super::collapsed_history_display;

    #[test]
    fn single_line_includes_time_without_line_suffix() {
        assert_eq!(collapsed_history_display("cargo test", "3h ago"), "cargo test \u{b7} 3h ago");
        assert_eq!(collapsed_history_display("cargo test", ""), "cargo test");
    }

    #[test]
    fn multiline_collapses_to_first_line_with_indicator() {
        let content = "cargo test \\\n  -- --nocapture\nthird line";
        assert_eq!(collapsed_history_display(content, "just now"), "cargo test \\ \u{b7} just now \u{b7} +2 lines");
        assert_eq!(collapsed_history_display("first\nsecond", ""), "first \u{b7} +1 line");
    }

    #[test]
    fn multiline_display_contains_no_newlines_or_tabs() {
        let content = "first\twith\ttabs\nsecond\rline\nthird";
        let display = collapsed_history_display(content, "1m ago");
        assert!(!display.contains('\n'));
        assert!(!display.contains('\t'));
        assert!(!display.contains('\r'));
        assert!(display.contains("+2 lines"));
    }

    #[test]
    fn display_sanitizes_other_control_characters() {
        let content = "first\x1bwith\x07controls\nsecond";
        let display = collapsed_history_display(content, "");
        assert!(!display.chars().any(|ch| ch.is_control()));
        assert!(display.contains("+1 line"));
    }
}
