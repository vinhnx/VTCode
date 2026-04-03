use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};

use super::Session;
use crate::config::constants::ui;
use crate::core_tui::session::TranscriptLine;

#[derive(Clone, Debug, Default)]
struct TranscriptReviewSearchState {
    active: bool,
    pending_query: String,
    query: String,
    matches: Vec<usize>,
    current_match: Option<usize>,
    restore_scroll_top: usize,
    restore_query: String,
    restore_match: Option<usize>,
}

#[derive(Clone, Debug, Default)]
struct CachedReviewMessage {
    revision: u64,
    lines: Vec<String>,
    lowered_lines: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TranscriptReviewState {
    width: u16,
    source_revision: u64,
    messages: Vec<CachedReviewMessage>,
    row_offsets: Vec<usize>,
    total_lines: usize,
    cached_export_text: Option<String>,
    scroll_top: usize,
    search: TranscriptReviewSearchState,
}

impl TranscriptReviewState {
    pub(crate) fn open(session: &Session, width: u16, height: u16) -> Self {
        let mut state = Self::default();
        state.refresh(session, width, height);
        state.scroll_to_bottom(height);
        state
    }

    pub(crate) fn refresh(&mut self, session: &Session, width: u16, height: u16) {
        let width = width.max(1);
        let revision = session.current_transcript_revision();
        if self.width == width && self.source_revision == revision {
            self.clamp_scroll(height);
            return;
        }

        let was_at_bottom = self.is_at_bottom(height);
        self.refresh_messages(session, width);
        self.width = width;
        self.source_revision = revision;
        self.recompute_matches();

        if was_at_bottom {
            self.scroll_to_bottom(height);
        } else {
            self.clamp_scroll(height);
        }
    }

    pub(crate) fn line_count(&self) -> usize {
        self.total_lines.max(1)
    }

    pub(crate) fn export_text(&mut self) -> String {
        if let Some(text) = &self.cached_export_text {
            return text.clone();
        }

        let mut export = String::new();
        let mut wrote_line = false;
        for message in &self.messages {
            for line in &message.lines {
                if wrote_line {
                    export.push('\n');
                }
                export.push_str(line);
                wrote_line = true;
            }
        }

        self.cached_export_text = Some(export.clone());
        export
    }

    pub(crate) fn visible_lines(&self, height: usize) -> Vec<Line<'static>> {
        let height = height.max(1);
        let end = self.scroll_top.saturating_add(height).min(self.total_lines);
        let current_match_line = self.current_match_line();
        let mut visible = Vec::with_capacity(height);

        for row in self.scroll_top..end {
            let style = if current_match_line == Some(row) {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            let line = self
                .line_text_at(row)
                .map_or_else(String::new, Clone::clone);
            visible.push(Line::styled(line, style));
        }

        while visible.len() < height {
            visible.push(Line::default());
        }

        visible
    }

    pub(crate) fn scroll_line_up(&mut self, height: u16) {
        self.scroll_top = self.scroll_top.saturating_sub(1);
        self.clamp_scroll(height);
    }

    pub(crate) fn scroll_line_down(&mut self, height: u16) {
        self.scroll_top = self
            .scroll_top
            .saturating_add(1)
            .min(self.max_scroll(height));
    }

    pub(crate) fn scroll_half_page_up(&mut self, height: u16) {
        self.scroll_top = self
            .scroll_top
            .saturating_sub(Self::page_step(height).max(1) / 2);
        self.clamp_scroll(height);
    }

    pub(crate) fn scroll_half_page_down(&mut self, height: u16) {
        self.scroll_top = self
            .scroll_top
            .saturating_add(Self::page_step(height).max(1) / 2)
            .min(self.max_scroll(height));
    }

    pub(crate) fn scroll_full_page_up(&mut self, height: u16) {
        self.scroll_top = self.scroll_top.saturating_sub(Self::page_step(height));
        self.clamp_scroll(height);
    }

    pub(crate) fn scroll_full_page_down(&mut self, height: u16) {
        self.scroll_top = self
            .scroll_top
            .saturating_add(Self::page_step(height))
            .min(self.max_scroll(height));
    }

    pub(crate) fn scroll_to_top(&mut self) {
        self.scroll_top = 0;
    }

    pub(crate) fn scroll_to_bottom(&mut self, height: u16) {
        self.scroll_top = self.max_scroll(height);
    }

    pub(crate) fn start_search(&mut self) {
        if self.search.active {
            return;
        }
        self.search.active = true;
        self.search.pending_query = self.search.query.clone();
        self.search.restore_scroll_top = self.scroll_top;
        self.search.restore_query = self.search.query.clone();
        self.search.restore_match = self.search.current_match;
    }

    pub(crate) fn search_active(&self) -> bool {
        self.search.active
    }

    pub(crate) fn search_query(&self) -> &str {
        if self.search.active {
            &self.search.pending_query
        } else {
            &self.search.query
        }
    }

    pub(crate) fn insert_search_text(&mut self, text: &str) {
        self.search.pending_query.push_str(text);
    }

    pub(crate) fn backspace_search(&mut self) {
        self.search.pending_query.pop();
    }

    pub(crate) fn cancel_search(&mut self) {
        self.search.active = false;
        self.scroll_top = self.search.restore_scroll_top;
        self.search.query = self.search.restore_query.clone();
        self.search.current_match = self.search.restore_match;
        self.search.pending_query.clear();
        self.recompute_matches();
    }

    pub(crate) fn commit_search(&mut self, height: u16) {
        self.search.active = false;
        self.search.query = self.search.pending_query.clone();
        self.search.pending_query.clear();
        self.recompute_matches();
        if !self.search.matches.is_empty() {
            self.search.current_match = Some(0);
            self.jump_to_current_match(height);
        } else {
            self.search.current_match = None;
        }
    }

    pub(crate) fn jump_next_match(&mut self, height: u16) {
        if self.search.matches.is_empty() {
            return;
        }
        let next = match self.search.current_match {
            Some(current) => (current + 1) % self.search.matches.len(),
            None => 0,
        };
        self.search.current_match = Some(next);
        self.jump_to_current_match(height);
    }

    pub(crate) fn jump_previous_match(&mut self, height: u16) {
        if self.search.matches.is_empty() {
            return;
        }
        let next = match self.search.current_match {
            Some(0) | None => self.search.matches.len().saturating_sub(1),
            Some(current) => current.saturating_sub(1),
        };
        self.search.current_match = Some(next);
        self.jump_to_current_match(height);
    }

    pub(crate) fn status_label(&self) -> String {
        let total = self.line_count();
        let line = (self.scroll_top + 1).min(total);
        let match_status = if self.search.query.is_empty() {
            "search off".to_string()
        } else if self.search.matches.is_empty() {
            format!("search '{}' (0 matches)", self.search.query)
        } else {
            let current = self.search.current_match.unwrap_or(0) + 1;
            format!(
                "search '{}' ({}/{})",
                self.search.query,
                current,
                self.search.matches.len()
            )
        };
        format!("line {line}/{total} • {match_status}")
    }

    fn refresh_messages(&mut self, session: &Session, width: u16) {
        let session_lines = &session.core.lines;
        let previous_len = self.messages.len();
        let current_len = session_lines.len();
        let width_changed = self.width != width;

        if current_len < previous_len {
            self.messages.truncate(current_len);
            self.cached_export_text = None;
        }
        while self.messages.len() < current_len {
            self.messages.push(CachedReviewMessage::default());
        }

        let first_dirty = if width_changed {
            0
        } else if current_len > previous_len {
            previous_len
        } else {
            session_lines
                .iter()
                .zip(self.messages.iter())
                .position(|(line, cached)| cached.revision != line.revision)
                .unwrap_or(current_len)
        };

        for (index, line) in session_lines.iter().enumerate().skip(first_dirty) {
            if width_changed || self.messages[index].revision != line.revision {
                self.messages[index] = CachedReviewMessage {
                    revision: line.revision,
                    lines: collect_review_message_lines(session, index, width),
                    lowered_lines: None,
                };
                self.cached_export_text = None;
            }
        }

        self.update_row_offsets_from(first_dirty.min(current_len));
    }

    fn update_row_offsets_from(&mut self, start_index: usize) {
        if start_index == 0 {
            self.row_offsets.clear();
            self.row_offsets.reserve(self.messages.len());
        } else {
            self.row_offsets.truncate(start_index);
        }

        let mut current_offset = self
            .row_offsets
            .last()
            .map(|offset| offset + self.messages[self.row_offsets.len() - 1].lines.len())
            .unwrap_or(0);

        for message in self.messages.iter().skip(self.row_offsets.len()) {
            self.row_offsets.push(current_offset);
            current_offset += message.lines.len();
        }

        self.total_lines = current_offset;
    }

    fn line_text_at(&self, row: usize) -> Option<&String> {
        if row >= self.total_lines {
            return None;
        }

        let message_index = match self.row_offsets.binary_search(&row) {
            Ok(index) => index,
            Err(0) => 0,
            Err(index) => index - 1,
        };
        let message = self.messages.get(message_index)?;
        let local_index = row.saturating_sub(self.row_offsets[message_index]);
        message.lines.get(local_index)
    }

    fn current_match_line(&self) -> Option<usize> {
        self.search
            .current_match
            .and_then(|index| self.search.matches.get(index).copied())
    }

    fn jump_to_current_match(&mut self, height: u16) {
        let Some(line) = self.current_match_line() else {
            return;
        };
        self.scroll_top = line.min(self.max_scroll(height));
    }

    fn recompute_matches(&mut self) {
        self.search.matches.clear();
        if self.search.query.is_empty() {
            self.search.current_match = None;
            return;
        }

        let needle = self.search.query.to_ascii_lowercase();
        let mut row_index = 0usize;
        for message in &mut self.messages {
            let lowered_lines = message.lowered_lines.get_or_insert_with(|| {
                message
                    .lines
                    .iter()
                    .map(|line| line.to_ascii_lowercase())
                    .collect()
            });
            for line in lowered_lines {
                if line.contains(&needle) {
                    self.search.matches.push(row_index);
                }
                row_index += 1;
            }
        }

        if let Some(current) = self.search.current_match
            && current < self.search.matches.len()
        {
            return;
        }

        self.search.current_match = (!self.search.matches.is_empty()).then_some(0);
    }

    fn clamp_scroll(&mut self, height: u16) {
        self.scroll_top = self.scroll_top.min(self.max_scroll(height));
    }

    fn max_scroll(&self, height: u16) -> usize {
        self.total_lines.saturating_sub(usize::from(height.max(1)))
    }

    fn is_at_bottom(&self, height: u16) -> bool {
        self.scroll_top >= self.max_scroll(height)
    }

    fn page_step(height: u16) -> usize {
        usize::from(height.max(2)).saturating_sub(1)
    }
}

fn collect_review_message_lines(session: &Session, index: usize, width: u16) -> Vec<String> {
    let mut lines: Vec<String> = session
        .reflow_message_lines_for_review(index, width)
        .into_iter()
        .map(transcript_line_text)
        .collect();

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn transcript_line_text(line: TranscriptLine) -> String {
    line.line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

pub(crate) fn render_transcript_review(
    session: &Session,
    frame: &mut Frame<'_>,
    area: Rect,
    state: &TranscriptReviewState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let title = Line::from(vec![
        Span::styled(
            " Transcript Review ",
            session
                .core
                .section_title_style()
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(state.status_label(), session.core.header_secondary_style()),
    ]);
    let block = Block::default().borders(Borders::ALL).title(title);
    frame.render_widget(Clear, area);
    frame.render_widget(block.clone(), area);
    let inner = block.inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let show_search = state.search_active();
    let chunks = if show_search {
        Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).split(inner)
    } else {
        Layout::vertical([Constraint::Min(1)]).split(inner)
    };
    let content_height = chunks[0].height;
    let lines = state.visible_lines(usize::from(content_height));
    frame.render_widget(
        Paragraph::new(lines).style(session.core.styles.default_style()),
        chunks[0],
    );

    if show_search && chunks.len() > 1 {
        let search = Paragraph::new(Line::from(vec![
            Span::styled("/", session.core.header_secondary_style()),
            Span::raw(state.search_query().to_string()),
        ]))
        .block(Block::default().borders(Borders::TOP).title(Span::styled(
            "Search",
            session.core.header_secondary_style(),
        )));
        frame.render_widget(search, chunks[1]);
    }
}

pub(crate) fn review_content_width(area: Rect) -> u16 {
    area.width.saturating_sub(2).min(ui::TUI_MAX_VIEWPORT_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_tui::app::session::AppSession;
    use crate::core_tui::types::{InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme};
    use std::sync::Arc;

    fn test_session() -> AppSession {
        AppSession::new(InlineTheme::default(), None, 24)
    }

    fn text_segment(text: impl Into<String>) -> InlineSegment {
        InlineSegment {
            text: text.into(),
            style: Arc::new(InlineTextStyle::default()),
        }
    }

    #[test]
    fn refresh_appends_without_rebuilding_unchanged_messages() {
        let mut session = test_session();
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("alpha")]);
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("beta")]);

        let mut review = TranscriptReviewState::open(&session, 40, 10);
        let original_first = review.messages[0].revision;

        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("gamma")]);
        review.refresh(&session, 40, 10);

        assert_eq!(review.messages[0].revision, original_first);
        assert_eq!(review.messages.len(), 3);
        assert_eq!(review.line_count(), 3);
    }

    #[test]
    fn refresh_rebuilds_from_first_dirty_message() {
        let mut session = test_session();
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("alpha")]);
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("beta")]);
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("gamma")]);

        let mut review = TranscriptReviewState::open(&session, 40, 10);
        let old_revisions: Vec<u64> = review
            .messages
            .iter()
            .map(|message| message.revision)
            .collect();

        let revision = session.core.next_revision();
        session.core.lines[1].segments = vec![text_segment("beta updated")];
        session.core.lines[1].revision = revision;
        session.core.mark_line_dirty(1);
        session.core.invalidate_transcript_cache();
        review.refresh(&session, 40, 10);

        assert_eq!(review.messages[0].revision, old_revisions[0]);
        assert_ne!(review.messages[1].revision, old_revisions[1]);
        assert_eq!(review.line_count(), 3);
    }

    #[test]
    fn search_uses_cached_lowercase_lines() {
        let mut session = test_session();
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("Alpha")]);
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("beta alpha")]);

        let mut review = TranscriptReviewState::open(&session, 40, 10);
        review.search.query = "alpha".to_string();
        review.recompute_matches();
        let lowered = review.messages[0]
            .lowered_lines
            .as_ref()
            .expect("lowered lines cached")[0]
            .clone();

        review.jump_next_match(10);
        review.recompute_matches();

        assert!(lowered.contains("alpha"));
        assert_eq!(review.search.matches, vec![0, 1]);
    }

    #[test]
    fn export_text_is_cached_until_refresh_changes_content() {
        let mut session = test_session();
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("alpha")]);

        let mut review = TranscriptReviewState::open(&session, 40, 10);
        let exported = review.export_text();
        assert!(exported.contains("alpha"));
        assert_eq!(
            review.cached_export_text.as_deref(),
            Some(exported.as_str())
        );

        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment("beta")]);
        review.refresh(&session, 40, 10);

        assert_eq!(review.cached_export_text, None);
        let refreshed = review.export_text();
        assert!(refreshed.contains("alpha"));
        assert!(refreshed.contains("beta"));
    }
}
