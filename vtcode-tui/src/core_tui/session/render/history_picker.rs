use super::*;
use crate::ui::tui::session::inline_list::{InlineListRow, render_inline_list};

pub fn split_inline_history_picker_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.height == 0
        || area.width == 0
        || session.modal.is_some()
        || session.file_palette_active
        || !session.history_picker_state.active
    {
        session.history_picker_state.visible_rows = 0;
        return (area, None);
    }

    let instruction_rows = 1_u16;
    let list_rows = if session.history_picker_state.matches.is_empty() {
        1_u16
    } else {
        session
            .history_picker_state
            .matches
            .len()
            .min(ui::INLINE_LIST_MAX_ROWS)
            .min(u16::MAX as usize) as u16
    };
    let desired_height = instruction_rows.saturating_add(list_rows);
    let max_panel_height = area.height.saturating_sub(1);
    if max_panel_height <= instruction_rows {
        session.history_picker_state.visible_rows = 0;
        return (area, None);
    }

    let panel_height = desired_height.min(max_panel_height);
    let chunks =
        Layout::vertical([Constraint::Min(1), Constraint::Length(panel_height)]).split(area);
    (chunks[0], Some(chunks[1]))
}

pub fn render_history_picker(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.height == 0
        || area.width == 0
        || !session.history_picker_state.active
        || session.modal.is_some()
        || session.file_palette_active
    {
        session.history_picker_state.visible_rows = 0;
        return;
    }

    frame.render_widget(Clear, area);

    let (query, matches_len, selected_idx, matches) = {
        let picker = &session.history_picker_state;
        (
            picker.search_query.clone(),
            picker.matches.len(),
            picker.list_state.selected(),
            picker.matches.clone(),
        )
    };

    let layout = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
    if layout.len() < 2 {
        session.history_picker_state.visible_rows = 0;
        return;
    }

    let title = if query.is_empty() {
        "History (Ctrl+R) · Enter Accept · Esc Cancel".to_owned()
    } else {
        format!(
            "History (Ctrl+R) · Enter Accept · Esc Cancel · filter: {}",
            query
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            title,
            default_style(session).add_modifier(Modifier::DIM),
        )))
        .wrap(Wrap { trim: true }),
        layout[0],
    );

    let list_area = layout[1];
    if list_area.height == 0 || list_area.width == 0 {
        session.history_picker_state.visible_rows = 0;
        return;
    }

    let visible_rows = (list_area.height as usize).min(ui::INLINE_LIST_MAX_ROWS);
    session.history_picker_state.visible_rows = visible_rows;

    let rendered_rows = if matches_len == 0 {
        vec![(
            InlineListRow::single(
                Line::from(Span::styled(
                    "No history matches".to_owned(),
                    default_style(session).add_modifier(Modifier::DIM | Modifier::ITALIC),
                )),
                default_style(session).add_modifier(Modifier::DIM),
            ),
            1_u16,
        )]
    } else {
        matches
            .into_iter()
            .take(visible_rows)
            .map(|item| {
                let max_chars = list_area.width as usize;
                let item_len = item.content.chars().count();
                let truncated = if item_len > max_chars {
                    let kept = max_chars.saturating_sub(1);
                    let content: String = item.content.chars().take(kept).collect();
                    format!("{content}…")
                } else {
                    item.content
                };
                (
                    InlineListRow::single(
                        Line::from(vec![Span::styled(truncated, default_style(session))]),
                        default_style(session),
                    ),
                    1_u16,
                )
            })
            .collect::<Vec<_>>()
    };

    let selected = selected_idx.filter(|index| *index < rendered_rows.len());
    let widget_state = render_inline_list(
        frame,
        list_area,
        rendered_rows,
        selected,
        Some(modal_list_highlight_style(session)),
    );

    let picker = &mut session.history_picker_state;
    picker.list_state.select(widget_state.selected);
    *picker.list_state.offset_mut() = widget_state.scroll_offset_index();
}
