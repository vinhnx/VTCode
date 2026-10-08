use super::*;
use crate::tui::core_tui::ThemeConfigParser;
use crate::tui::core_tui::session::list_panel::{
    SharedListPanelSections, SharedListPanelStyles, input_styles_from_theme, render_shared_list_panel,
};
use crate::tui::core_tui::session::{
    inline_list::{InlineListRow, list_cursor},
    list_panel::SharedListWidgetModel,
};
use crate::tui::core_tui::style::ratatui_color_from_ansi;
use crate::tui::core_tui::types::{LocalAgentEntry, LocalAgentKind};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use tracing::warn;
use tui_shimmer::shimmer_spans_with_style_at_phase;

struct LocalAgentsPanelModel {
    entries: Vec<LocalAgentEntry>,
    selected: Option<usize>,
    offset: usize,
    visible_rows: usize,
    highlight_style: Style,
    /// Muted-by-color row style (theme secondary), never `Modifier::DIM` —
    /// ratatui's `Cell::set_style` only inserts modifiers, so a DIM painted
    /// over the panel sticks to every glyph drawn on top of it.
    muted_style: Style,
}

impl SharedListWidgetModel for LocalAgentsPanelModel {
    fn rows(&self, width: u16) -> Vec<(InlineListRow, u16)> {
        if self.entries.is_empty() {
            return vec![(
                InlineListRow::single(
                    Line::from(Span::styled(
                        "No local agents yet".to_owned(),
                        self.muted_style.add_modifier(Modifier::ITALIC),
                    )),
                    self.muted_style,
                ),
                1_u16,
            )];
        }

        let muted_style = self.muted_style;
        let max_chars = width.saturating_sub(3) as usize;
        self.entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let is_selected = self.selected == Some(idx);
                let row_text = truncate_row(
                    format!("{} · {} · {} · {}", entry.display_label, entry.kind.as_str(), entry.status, entry.id),
                    max_chars,
                );
                let cursor = list_cursor(is_selected);
                let cursor_style = if is_selected { self.highlight_style } else { muted_style };
                let text_style = if is_selected { self.highlight_style } else { muted_style };
                (
                    InlineListRow::single(
                        Line::from(vec![Span::styled(cursor, cursor_style), Span::styled(row_text, text_style)]),
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

/// Centered floating window for multi-agent / background-process management.
/// Roughly 82% x 78% of the viewport so live activity stays readable.
pub(crate) fn local_agents_window_area(viewport: Rect) -> Rect {
    if viewport.width == 0 || viewport.height == 0 {
        return viewport;
    }
    let width = ((viewport.width as u32 * 82) / 100).clamp(20, u32::from(viewport.width)) as u16;
    let height = ((viewport.height as u32 * 78) / 100).clamp(8, u32::from(viewport.height)) as u16;
    let x = viewport.x + (viewport.width.saturating_sub(width) / 2);
    let y = viewport.y + (viewport.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}

fn local_agents_header_summary(live: usize, finished: usize) -> String {
    match (live, finished) {
        (0, 0) => "No background agents yet".to_string(),
        (0, finished) => {
            let suffix = if finished == 1 { "" } else { "s" };
            format!("{finished} agent{suffix} finished")
        }
        (live, 0) => format!("{live} running"),
        (live, finished) => format!("{live} running · {finished} finished"),
    }
}

pub fn render_local_agents(session: &mut Session, frame: &mut Frame<'_>, viewport: Rect) {
    if viewport.height == 0 || viewport.width == 0 || !session.inline_lists_visible() || !session.local_agents_visible()
    {
        session.local_agents_state.set_visible_rows(0);
        session.local_agents_state.set_list_area(None);
        session.local_agents_state.set_window_area(None);
        return;
    }

    let window = local_agents_window_area(viewport);
    session.local_agents_state.set_window_area(Some(window));
    frame.render_widget(Clear, window);

    let default_style = default_style(session);
    // Muted by explicit color (theme secondary), never `Modifier::DIM`: DIM
    // doubles as a sticky area style in this pipeline and renders
    // near-invisible on several terminals.
    let muted_style = session.core.styles.muted_text_style();
    let highlight_style = modal_list_highlight_style(session);
    let (selected_index, scroll_offset, entries, live_count, finished_count) = {
        let state = &session.local_agents_state;
        (
            state.selected(),
            state.scroll_offset(),
            state.entries().to_vec(),
            state.loading_count(),
            state.finished_count(),
        )
    };
    let selected_exec_session = selected_index
        .and_then(|index| entries.get(index))
        .is_some_and(|entry| entry.kind == LocalAgentKind::ExecSession);

    let info_line = if entries.is_empty() {
        "Background subagents are opt-in. Configure one, then use Ctrl+B or /subprocesses.".to_string()
    } else if selected_exec_session {
        "↑↓ Navigate · Enter inspect · Ctrl+K stop · Ctrl+X close · Ctrl+R focus · Ctrl+P preview · Esc close"
            .to_string()
    } else {
        "↑↓ Navigate · Enter inspect · Alt+O transcript · Ctrl+K stop · Ctrl+X close · Esc close".to_string()
    };

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(local_agents_divider_style(session, selected_index, &entries))
        .title(Span::styled("Background", highlight_style));
    let inner = block.inner(window);
    frame.render_widget(block, window);

    let [header_area, info_area, body] = match inner.try_layout(&Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])) {
        Ok(areas) => areas,
        Err(_) => {
            warn!(target: "vtcode::tui", height = inner.height, "local agents window layout failed, skipping render");
            session.local_agents_state.set_list_area(None);
            return;
        }
    };

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            local_agents_header_summary(live_count, finished_count),
            highlight_style,
        )))
        .style(default_style),
        header_area,
    );
    frame.render_widget(
        Paragraph::new(info_line)
            .style(session.core.styles.muted_text_style())
            .wrap(Wrap { trim: false }),
        info_area,
    );

    let [list_area, preview_area] = body
        .try_layout(&Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]))
        .unwrap_or([body; 2]);

    let mut list_model = LocalAgentsPanelModel {
        entries: entries.clone(),
        selected: selected_index,
        offset: scroll_offset,
        visible_rows: 0,
        highlight_style,
        muted_style,
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
            input_styles: input_styles_from_theme(&session.core.theme),
            show_divider: false,
        },
        &mut list_model,
    );

    session.local_agents_state.set_visible_rows(list_model.visible_rows);
    session.local_agents_state.set_scroll_offset(list_model.offset);
    session.local_agents_state.set_list_area(Some(list_area));

    let selected_entry = selected_index.and_then(|index| entries.get(index));
    let preview_text = selected_entry
        .map(|entry| format_local_agent_preview(session, entry))
        .unwrap_or_else(|| {
            vec![
                Line::from("No local agents yet."),
                Line::default(),
                Line::from(
                    "Configure a background agent and press Ctrl+B, or use /subprocesses to open this window later.",
                ),
            ]
        });

    frame.render_widget(Paragraph::new(preview_text).style(default_style).wrap(Wrap { trim: false }), preview_area);
}

fn format_local_agent_preview(session: &Session, entry: &LocalAgentEntry) -> Vec<Line<'static>> {
    let mut lines = vec![local_agent_title_line(session, entry)];

    if let Some(summary) = entry.summary.as_deref().filter(|summary| !summary.trim().is_empty()) {
        lines.push(local_agent_status_line(session, summary, entry.is_loading()));
    }

    if let Some(path) = entry.transcript_path.as_ref() {
        lines.push(Line::from(format!("Transcript: {}", path.display())));
    }

    lines.push(Line::default());
    let preview = if entry.preview.trim().is_empty() {
        "Waiting for live transcript output..."
    } else {
        entry.preview.as_str()
    };
    lines.extend(preview.lines().map(|line| Line::from(line.to_string())));
    lines
}

fn local_agent_title_line(session: &Session, entry: &LocalAgentEntry) -> Line<'static> {
    let base = default_style(session);
    let muted = session.core.styles.muted_text_style();
    let mut spans = vec![
        Span::styled(entry.display_label.clone(), base),
        Span::styled(" · ".to_string(), muted),
        Span::styled(entry.kind.as_str().to_string(), muted),
        Span::styled(" · ".to_string(), muted),
    ];

    if entry.is_loading() && session.core.appearance.should_animate_progress_status() {
        spans.extend(shimmer_spans_with_style_at_phase(
            &entry.status,
            accent_style(session),
            session.core.shimmer_state.phase(),
        ));
    } else {
        spans.push(Span::styled(entry.status.clone(), accent_style(session)));
    }

    Line::from(spans)
}

fn local_agent_status_line(session: &Session, text: &str, shimmer: bool) -> Line<'static> {
    let style = session.core.styles.muted_text_style();
    if shimmer && session.core.appearance.should_animate_progress_status() {
        Line::from(shimmer_spans_with_style_at_phase(text, style, session.core.shimmer_state.phase()))
    } else {
        Line::from(Span::styled(text.to_string(), style))
    }
}

fn truncate_row(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }
    vtcode_commons::formatting::truncate_within(&text, max_chars, "…")
}

fn local_agents_divider_style(session: &Session, selected_index: Option<usize>, entries: &[LocalAgentEntry]) -> Style {
    let fallback = session.styles.accent_style().add_modifier(Modifier::BOLD);
    let Some(entry) = selected_index.and_then(|index| entries.get(index)) else {
        return fallback;
    };
    let Some(color_spec) = entry.color.as_deref().filter(|value| !value.trim().is_empty()) else {
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

#[cfg(test)]
mod tests {
    use super::{
        Session, format_local_agent_preview, local_agent_status_line, local_agent_title_line,
        local_agents_header_summary, local_agents_window_area,
    };
    use crate::tui::core_tui::types::{InlineTheme, LocalAgentEntry, LocalAgentKind};
    use ratatui::layout::Rect;
    use std::time::Duration;

    fn sample_entry(status: &str) -> LocalAgentEntry {
        LocalAgentEntry {
            program_status: vtcode_commons::program_status::ProgramState::Idle,
            updated_at: 0,
            id: "thread-1".to_string(),
            display_label: "rust-engineer".to_string(),
            agent_name: "rust-engineer".to_string(),
            color: Some("cyan".to_string()),
            kind: LocalAgentKind::Delegated,
            status: status.to_string(),
            summary: Some("Reviewing the workspace".to_string()),
            preview: "assistant: reviewing the workspace".to_string(),
            transcript_path: None,
        }
    }

    #[test]
    fn loading_preview_shimmers_status_lines() {
        let mut session = Session::new(InlineTheme::default(), None, 14);
        std::thread::sleep(Duration::from_millis(100));
        assert!(session.core.shimmer_state.update());

        let animated = local_agent_status_line(&session, "Reviewing the workspace", true);
        let static_line = local_agent_status_line(&session, "Reviewing the workspace", false);

        assert_ne!(animated.spans, static_line.spans);
    }

    #[test]
    fn terminal_preview_keeps_status_lines_static() {
        let session = Session::new(InlineTheme::default(), None, 14);
        let lines = format_local_agent_preview(&session, &sample_entry("completed"));

        assert_eq!(lines[1].spans.len(), 1);
    }

    #[test]
    fn reduce_motion_keeps_loading_agent_status_static_and_visible() {
        let mut session = Session::new(InlineTheme::default(), None, 14);
        session.core.appearance.reduce_motion_mode = true;
        let mut entry = sample_entry("running");
        entry.summary = Some("Reviewing the workspace".to_string());

        let title = local_agent_title_line(&session, &entry);
        let title_text = title.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
        let summary = local_agent_status_line(&session, "Reviewing the workspace", true);
        let static_summary = local_agent_status_line(&session, "Reviewing the workspace", false);

        assert!(title_text.ends_with("running"), "loading status should remain visible: {title_text}");
        assert_eq!(title.spans.len(), 5, "loading status should use one static label span");
        assert_eq!(summary.spans, static_summary.spans);
        assert_eq!(summary.spans[0].content.as_ref(), "Reviewing the workspace");
    }

    #[test]
    fn header_summary_reports_live_and_finished() {
        assert_eq!(local_agents_header_summary(0, 0), "No background agents yet");
        assert_eq!(local_agents_header_summary(2, 0), "2 running");
        assert_eq!(local_agents_header_summary(0, 1), "1 agent finished");
        assert_eq!(local_agents_header_summary(0, 4), "4 agents finished");
        assert_eq!(local_agents_header_summary(1, 2), "1 running · 2 finished");
    }

    #[test]
    fn window_area_is_centered_large_panel() {
        let area = local_agents_window_area(Rect::new(0, 0, 100, 40));
        assert!(area.width < 100 && area.width >= 80);
        assert!(area.height < 40 && area.height >= 30);
        assert_eq!(area.x, (100 - area.width) / 2);
        assert_eq!(area.y, (40 - area.height) / 2);
    }
}
