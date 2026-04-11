use super::*;
use crate::core_tui::ThemeConfigParser;
use crate::core_tui::session::list_panel::{
    ListPanelLayout, SharedListPanelSections, SharedListPanelStyles, fixed_section_rows,
    render_shared_list_panel, rows_to_u16,
};
use crate::core_tui::session::{inline_list::InlineListRow, list_panel::SharedListWidgetModel};
use crate::core_tui::style::ratatui_color_from_ansi;
use crate::core_tui::types::LocalAgentEntry;
use ratatui::widgets::{Clear, Paragraph, Wrap};

struct LocalAgentsPanelModel {
    entries: Vec<LocalAgentEntry>,
    selected: Option<usize>,
    offset: usize,
    visible_rows: usize,
    base_style: Style,
}

impl SharedListWidgetModel for LocalAgentsPanelModel {
    fn rows(&self, width: u16) -> Vec<(InlineListRow, u16)> {
        if self.entries.is_empty() {
            return vec![(
                InlineListRow::single(
                    Line::from(Span::styled(
                        "No local agents yet".to_owned(),
                        self.base_style
                            .add_modifier(Modifier::DIM | Modifier::ITALIC),
                    )),
                    self.base_style.add_modifier(Modifier::DIM),
                ),
                1_u16,
            )];
        }

        let max_chars = width.saturating_sub(3) as usize;
        self.entries
            .iter()
            .map(|entry| {
                let row_text = truncate_row(
                    format!(
                        "{} · {} · {}",
                        entry.display_label,
                        entry.kind.as_str(),
                        entry.status
                    ),
                    max_chars,
                );
                (
                    InlineListRow::single(
                        Line::from(Span::styled(row_text, self.base_style)),
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

pub(crate) fn local_agents_panel_layout(session: &Session) -> Option<ListPanelLayout> {
    if !session.local_agents_visible() || !session.inline_lists_visible() {
        return None;
    }

    let visible_entries = session.local_agents_state.entries().len().max(1);
    let fixed_rows = fixed_section_rows(2, 1, false);
    let desired_rows = rows_to_u16(visible_entries.min(ui::INLINE_LIST_MAX_ROWS));
    Some(ListPanelLayout::new(fixed_rows, desired_rows))
}

pub fn split_inline_local_agents_area(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.height == 0 || area.width == 0 {
        session.local_agents_state.set_visible_rows(0);
        return (area, None);
    }

    let Some(layout) = local_agents_panel_layout(session) else {
        session.local_agents_state.set_visible_rows(0);
        return (area, None);
    };

    let (transcript_area, panel_area) = layout.split(area);
    if panel_area.is_none() {
        session.local_agents_state.set_visible_rows(0);
        return (transcript_area, None);
    }

    (transcript_area, panel_area)
}

pub fn render_local_agents(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.height == 0
        || area.width == 0
        || !session.inline_lists_visible()
        || !session.local_agents_visible()
    {
        session.local_agents_state.set_visible_rows(0);
        return;
    }

    frame.render_widget(Clear, area);

    let default_style = default_style(session);
    let highlight_style = modal_list_highlight_style(session);
    let (selected_index, scroll_offset, entries) = {
        let state = &session.local_agents_state;
        (
            state.selected(),
            state.scroll_offset(),
            state.entries().to_vec(),
        )
    };

    let info_line = if entries.is_empty() {
        "Background subagents are opt-in. Configure one, then use Ctrl+B or /subprocesses."
            .to_string()
    } else {
        format!(
            "{} local agent{} • Enter inspect • Alt+O transcript • Ctrl+K stop • Ctrl+X cancel • Esc close",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" }
        )
    };

    let header_rows = SharedListPanelSections {
        header: vec![Line::from(Span::styled(
            "Local Agents".to_owned(),
            default_style,
        ))],
        info: vec![Line::from(Span::styled(info_line, default_style))],
        search: None,
    };

    let mut list_model = LocalAgentsPanelModel {
        entries: entries.clone(),
        selected: selected_index,
        offset: scroll_offset,
        visible_rows: 0,
        base_style: default_style,
    };

    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(area);

    let divider_style = local_agents_divider_style(session, selected_index, &entries);
    frame.render_widget(
        Paragraph::new(Line::from("─".repeat(area.width as usize))).style(divider_style),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new(header_rows.header)
            .style(default_style)
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new(header_rows.info)
            .style(default_style.add_modifier(Modifier::DIM))
            .wrap(Wrap { trim: false }),
        chunks[2],
    );

    let body = chunks[3];
    let [list_area, preview_area] = Layout::horizontal([
        Constraint::Percentage(38),
        Constraint::Percentage(62),
    ])
    .split(body)[..] else {
        session.local_agents_state.set_visible_rows(0);
        return;
    };

    render_shared_list_panel(
        frame,
        list_area,
        SharedListPanelSections::default(),
        SharedListPanelStyles {
            base_style: default_style,
            selected_style: Some(highlight_style),
            text_style: default_style,
            divider_style: None,
        },
        &mut list_model,
    );

    session
        .local_agents_state
        .set_visible_rows(list_model.visible_rows.max(1));

    let selected_entry = selected_index.and_then(|index| entries.get(index));
    let preview_text = selected_entry
        .map(format_local_agent_preview)
        .unwrap_or_else(|| {
            "No local agents are running.\n\nUse /subprocesses to open this drawer later, or configure a background agent and press Ctrl+B.".to_string()
        });

    frame.render_widget(
        Paragraph::new(preview_text)
            .style(default_style)
            .wrap(Wrap { trim: false }),
        preview_area,
    );
}

fn format_local_agent_preview(entry: &LocalAgentEntry) -> String {
    let mut lines = vec![format!(
        "{} · {} · {}",
        entry.display_label,
        entry.kind.as_str(),
        entry.status
    )];

    if let Some(summary) = entry
        .summary
        .as_deref()
        .filter(|summary| !summary.trim().is_empty())
    {
        lines.push(summary.to_string());
    }

    if let Some(path) = entry.transcript_path.as_ref() {
        lines.push(format!("Transcript: {}", path.display()));
    }

    lines.push(String::new());
    if entry.preview.trim().is_empty() {
        lines.push("Waiting for live transcript output...".to_string());
    } else {
        lines.push(entry.preview.clone());
    }

    lines.join("\n")
}

fn truncate_row(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn local_agents_divider_style(
    session: &Session,
    selected_index: Option<usize>,
    entries: &[LocalAgentEntry],
) -> Style {
    let fallback = session.styles.accent_style().add_modifier(Modifier::BOLD);
    let Some(entry) = selected_index.and_then(|index| entries.get(index)) else {
        return fallback;
    };
    let Some(color_spec) = entry
        .color
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return fallback;
    };

    let parser = ThemeConfigParser::default();
    let Some(parsed) = parser.parse_flexible(color_spec).ok() else {
        return fallback;
    };
    let Some(color) = parsed.get_bg_color().or(parsed.get_fg_color()) else {
        return fallback;
    };

    fallback.fg(ratatui_color_from_ansi(color))
}
