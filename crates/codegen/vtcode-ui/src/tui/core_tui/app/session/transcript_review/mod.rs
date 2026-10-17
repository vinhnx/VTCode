use std::collections::{HashMap, HashSet};

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use ratatui_cheese::input::{Input, InputState};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{Session, ToolOutputBlock};
use crate::tui::config::constants::ui;
use crate::tui::core_tui::session::action::Action;
use crate::tui::core_tui::session::list_panel::input_styles_from_theme;
use crate::tui::core_tui::session::text_utils::strip_ansi_codes;
use crate::tui::core_tui::style::{ratatui_color_from_ansi, ratatui_style_from_inline};
use crate::tui::core_tui::types::InlineMessageKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TranscriptRenderMode {
    #[default]
    Rich,
    Raw,
}

impl TranscriptRenderMode {
    fn label(self) -> &'static str {
        match self {
            Self::Rich => "rich",
            Self::Raw => "raw",
        }
    }

    fn toggle(self) -> Self {
        match self {
            Self::Rich => Self::Raw,
            Self::Raw => Self::Rich,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ToolOutputSearchState {
    active: bool,
    pending_query: String,
    query: String,
    matches: Vec<usize>,
    current_match: Option<usize>,
    restore_scroll_top: usize,
    restore_query: String,
    restore_match: Option<usize>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum ReviewBlockKey {
    Core(usize),
    Tool(u64),
    OrphanTool(usize),
}

impl Default for ReviewBlockKey {
    fn default() -> Self {
        Self::Core(0)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ReviewSourceKind {
    Core(usize),
    Tool(usize),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReviewSource {
    key: ReviewBlockKey,
    revision: u64,
    kind: ReviewSourceKind,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CachedToolOutputBlock {
    key: ReviewBlockKey,
    revision: u64,
    /// ANSI-free lines used for search, copying, editor handoff, and raw export.
    lines: Vec<String>,
    /// Width-aware styled lines used by the default rich review mode.
    rich_lines: Vec<Line<'static>>,
    /// Evidence target for each wrapped tool row, shared across its continuations.
    evidence_links: Vec<Option<std::sync::Arc<str>>>,
    lowered_lines: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReviewRevision {
    transcript: u64,
    tool_output: u64,
    presentation: u64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ToolOutputViewerState {
    width: u16,
    height: u16,
    source_revision: ReviewRevision,
    messages: Vec<CachedToolOutputBlock>,
    row_offsets: Vec<usize>,
    total_lines: usize,
    cached_export_text: Option<String>,
    scroll_top: usize,
    search: ToolOutputSearchState,
    mode: TranscriptRenderMode,
    focus_target: Option<u64>,
    viewer_area: Rect,
    content_area: Rect,
    title_mode_hit_region: Option<Rect>,
    hovered_mode_control: bool,
    title_close_hit_region: Option<Rect>,
    hovered_close_control: bool,
}

mod render;
mod scroll;
mod search;
mod sources;
mod state;

pub(crate) use search::next_match_index;
pub(crate) use sources::{build_cached_block, collect_review_sources, line_text, wrap_output_line};

const COMPACT_ACTIVITY_HINT_TAIL: &str = "transcript · click to expand";

fn rect_contains_or_empty(area: Rect, column: u16, row: u16) -> bool {
    (area.width == 0 || area.height == 0) || area.contains(Position { x: column, y: row })
}

fn opt_rect_contains(region: Option<Rect>, column: u16, row: u16) -> bool {
    region.is_some_and(|area| area.contains(Position { x: column, y: row }))
}

fn set_hover(current: &mut bool, hovered: bool) -> bool {
    if *current == hovered {
        return false;
    }
    *current = hovered;
    true
}

fn review_hint_binding(session: &Session) -> Option<&str> {
    if !session.core.transcript_review_hints_visible() {
        return None;
    }
    session.core.primary_binding_label(Action::OpenTranscriptReview)
}

pub(super) fn compact_activity_hint_text(session: &Session) -> Option<String> {
    review_hint_binding(session).map(|binding| format!("{binding} {COMPACT_ACTIVITY_HINT_TAIL}"))
}

pub(super) fn compact_activity_segments(
    session: &Session,
    metadata: &vtcode_commons::ui_protocol::CompactActivityMetadata,
) -> Vec<crate::tui::core_tui::types::InlineSegment> {
    let styles = crate::tui::ui::shell_syntax::ShellLineStyles::from_session(session);
    let mut segments = crate::tui::ui::shell_syntax::line_to_compact_segments(metadata, &styles);

    if let Some(binding) = review_hint_binding(session) {
        let separator_style = session.core.styles.default_inline_style().dim();
        segments.push(crate::tui::core_tui::types::InlineSegment {
            text: " · ".to_string(),
            style: std::sync::Arc::new(separator_style),
        });
        let binding_style = session.core.styles.accent_inline_style().underline().bold();
        segments.push(crate::tui::core_tui::types::InlineSegment {
            text: binding.to_string(),
            style: std::sync::Arc::new(binding_style),
        });
        // Keep the explanatory words dimmed but underline the click affordance
        // itself so it reads as clickable. Hit regions are derived from
        // underlined spans, so both the binding and this action open review.
        let rest_style = session.core.styles.default_inline_style().dim();
        segments.push(crate::tui::core_tui::types::InlineSegment {
            text: " transcript · ".to_string(),
            style: std::sync::Arc::new(rest_style),
        });
        let action_style = session.core.styles.accent_inline_style().underline();
        segments.push(crate::tui::core_tui::types::InlineSegment {
            text: "click to expand".to_string(),
            style: std::sync::Arc::new(action_style),
        });
    }

    segments
}

pub(crate) fn render_tool_output_viewer(
    session: &Session,
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut ToolOutputViewerState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let title_prefix = " Transcript Review ";
    let status = format!(" {}", state.status_label());
    let mode_label = format!(" [{}] ", state.mode.label());
    let mode_x = area
        .x
        .saturating_add(1)
        .saturating_add(UnicodeWidthStr::width(title_prefix) as u16)
        .saturating_add(UnicodeWidthStr::width(status.as_str()) as u16);
    let mode_width = UnicodeWidthStr::width(mode_label.as_str()) as u16;
    state.title_mode_hit_region = (mode_width > 0 && mode_x < area.right())
        .then(|| Rect::new(mode_x, area.y, mode_width.min(area.right().saturating_sub(mode_x)), 1));
    let close_label = if session.core.transcript_review_close_button_visible() {
        " [close] "
    } else {
        ""
    };
    let close_x = mode_x.saturating_add(mode_width);
    let close_width = UnicodeWidthStr::width(close_label) as u16;
    state.title_close_hit_region = (close_width > 0 && close_x < area.right())
        .then(|| Rect::new(close_x, area.y, close_width.min(area.right().saturating_sub(close_x)), 1));
    state.set_viewer_area(area);

    let mode_style = session.core.header_secondary_style().add_modifier(Modifier::BOLD).add_modifier(
        if state.hovered_mode_control {
            Modifier::REVERSED | Modifier::UNDERLINED
        } else {
            Modifier::empty()
        },
    );
    let close_style = session.core.header_secondary_style().add_modifier(Modifier::BOLD).add_modifier(
        if state.hovered_close_control {
            Modifier::REVERSED | Modifier::UNDERLINED
        } else {
            Modifier::empty()
        },
    );
    let title = Line::from(vec![
        Span::styled(title_prefix, session.core.section_title_style().add_modifier(Modifier::BOLD)),
        Span::styled(status, session.core.header_secondary_style()),
        Span::styled(mode_label, mode_style),
        Span::styled(close_label, close_style),
    ]);
    let block = Block::default().borders(Borders::ALL).title(title);
    frame.render_widget(Clear, area);
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let show_search = state.search_active();
    let show_footer =
        session.core.transcript_review_shortcut_guide_visible() && inner.height >= if show_search { 3 } else { 2 };
    let mut constraints = Vec::with_capacity(3);
    constraints.push(Constraint::Min(1));
    if show_search {
        constraints.push(Constraint::Length(2));
    }
    if show_footer {
        constraints.push(Constraint::Length(1));
    }
    let chunks = Layout::vertical(constraints).split(inner);
    let content_height = chunks[0].height;
    state.content_area = chunks[0];
    let lines = state.visible_lines(usize::from(content_height));
    frame.render_widget(Paragraph::new(lines).style(session.core.styles.default_style()), chunks[0]);

    if show_search && chunks.len() > 1 {
        let input_styles = input_styles_from_theme(&session.core.theme);
        let input_widget = Input::new("Search")
            .placeholder("type to search...")
            .prompt("/")
            .styles(input_styles);

        let mut input_state = InputState::new();
        let query = state.search_query().to_string();
        let cursor_steps = query.chars().count();
        input_state.set_value(query);
        input_state.set_focused(true);
        for _ in 0..cursor_steps {
            input_state.move_right();
        }

        frame.render_stateful_widget(&input_widget, chunks[1], &mut input_state);
    }

    if show_footer {
        if let Some(hint) = transcript_review_shortcut_hint(session, show_search) {
            let footer_index = chunks.len().saturating_sub(1);
            frame.render_widget(
                Paragraph::new(Line::styled(hint, session.core.styles.default_style().dim())),
                chunks[footer_index],
            );
        }
    }
}

fn transcript_review_shortcut_hint(session: &Session, searching: bool) -> Option<String> {
    if !session.core.transcript_review_shortcut_guide_visible() {
        return None;
    }

    if searching {
        // While the search input owns keys, `q`/scroll hints don't apply:
        // typing goes to the query, Esc cancels, Enter finds.
        return Some("Esc cancel · Enter find".to_string());
    }

    let mut hints = Vec::with_capacity(5);
    if let Some(binding) = session.core.primary_binding_label(Action::OpenTranscriptReview) {
        hints.push(format!("{binding} open/close"));
    }
    if let Some(binding) = session.core.primary_binding_label(Action::ToggleTranscriptRenderMode) {
        hints.push(format!("{binding} rich/raw"));
    }
    hints.extend(["q/Esc close", "/ search", "↑/↓ scroll"].map(str::to_string));
    Some(hints.join(" · "))
}

pub(crate) fn viewer_content_width(area: Rect) -> u16 {
    area.width.saturating_sub(2).min(ui::TUI_MAX_VIEWPORT_WIDTH)
}

pub(crate) fn viewer_content_height(session: &Session, state: &ToolOutputViewerState, area: Rect) -> u16 {
    let inner_height = area.height.saturating_sub(2);
    let show_search = state.search_active();
    let show_footer =
        session.core.transcript_review_shortcut_guide_visible() && inner_height >= if show_search { 3 } else { 2 };
    let reserved_rows = usize::from(show_search) * 2 + usize::from(show_footer);
    inner_height.saturating_sub(reserved_rows as u16).max(1)
}

#[cfg(test)]
mod tests;
