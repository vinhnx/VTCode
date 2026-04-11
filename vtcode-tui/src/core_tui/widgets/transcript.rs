use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget},
};

use crate::config::constants::ui;
use crate::ui::tui::session::terminal_capabilities;
use crate::ui::tui::session::{Session, TranscriptLine, spinner_frame_for_phase};
use vtcode_config::constants::tools;

/// Widget for rendering the transcript area with conversation history
///
/// This widget handles:
/// - Scroll viewport management
/// - Content caching and optimization
/// - Text wrapping and overflow
/// - Queue overlay rendering
///
/// # Example
/// ```ignore
/// TranscriptWidget::new(session)
///     .show_scrollbar(true)
///     .custom_style(style)
///     .render(area, buf);
/// ```
pub struct TranscriptWidget<'a> {
    session: &'a mut Session,
    show_scrollbar: bool,
    custom_style: Option<Style>,
}

impl<'a> TranscriptWidget<'a> {
    /// Create a new TranscriptWidget with required parameters
    pub fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            show_scrollbar: false,
            custom_style: None,
        }
    }

    /// Enable or disable scrollbar rendering
    #[must_use]
    pub fn show_scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    /// Set a custom style for the transcript
    #[must_use]
    pub fn custom_style(mut self, style: Style) -> Self {
        self.custom_style = Some(style);
        self
    }
}

impl<'a> Widget for TranscriptWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            self.session.set_transcript_area(None);
            self.session.clear_transcript_file_link_targets();
            return;
        }

        let block = Block::new()
            .border_type(terminal_capabilities::get_border_type())
            .style(self.session.styles.default_style())
            .border_style(self.session.styles.border_style());

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            self.session.set_transcript_area(None);
            self.session.clear_transcript_file_link_targets();
            return;
        }
        self.session.set_transcript_area(Some(inner));

        // Clamp effective dimensions to prevent pathological CPU usage with huge terminals
        // See: https://github.com/anthropics/claude-code/issues/21567
        let effective_height = inner.height.min(ui::TUI_MAX_VIEWPORT_HEIGHT);
        let effective_width = inner.width.min(ui::TUI_MAX_VIEWPORT_WIDTH);

        self.session.apply_transcript_rows(effective_height);

        let content_width = effective_width;
        if content_width == 0 {
            self.session.clear_transcript_file_link_targets();
            return;
        }
        self.session.apply_transcript_width(content_width);

        let viewport_rows = effective_height as usize;
        let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
        let effective_padding = padding.min(viewport_rows.saturating_sub(1));
        let total_rows = self.session.total_transcript_rows(content_width) + effective_padding;
        let (top_offset, _clamped_total_rows) = self
            .session
            .prepare_transcript_scroll(total_rows, viewport_rows);
        let vertical_offset = top_offset.min(self.session.scroll_manager.max_offset());
        self.session.transcript_view_top = vertical_offset;

        let visible_start = vertical_offset;
        let scroll_area = inner;

        // Use cached visible lines to avoid rebuilding on every frame
        let cached_lines = self.session.collect_transcript_window_cached(
            content_width,
            visible_start,
            viewport_rows,
        );

        // Check if we need to mutate the lines (fill empty space or add overlays)
        let fill_count = viewport_rows.saturating_sub(cached_lines.len());
        let needs_mutation = fill_count > 0 || !self.session.queued_inputs.is_empty();

        let mut visible_lines = if needs_mutation {
            // Need to mutate, so clone and modify
            let mut lines = cached_lines.to_vec();
            if fill_count > 0 {
                let target_len = lines.len() + fill_count;
                lines.resize_with(target_len, TranscriptLine::default);
            }
            self.session.overlay_queue_lines(&mut lines, content_width);
            self.session
                .decorate_visible_cached_transcript_links(lines, scroll_area)
        } else {
            self.session
                .decorate_borrowed_cached_transcript_links(cached_lines.as_slice(), scroll_area)
        };
        apply_active_file_operation_spinner(self.session, &mut visible_lines);

        // Only clear if content actually changed, not on viewport-only scroll
        // This is a significant optimization: avoids expensive Clear operation on most scrolls
        if self.session.transcript_clear_required {
            Clear.render(scroll_area, buf);
            self.session.transcript_clear_required = false;
        }
        apply_full_width_line_backgrounds(buf, scroll_area, &visible_lines);
        let paragraph = Paragraph::new(visible_lines).style(self.session.styles.default_style());
        paragraph.render(scroll_area, buf);
    }
}

const FILE_OPERATION_STATUS_TOOLS: &[&str] = &[
    tools::WRITE_FILE,
    tools::CREATE_FILE,
    tools::EDIT_FILE,
    tools::APPLY_PATCH,
    tools::SEARCH_REPLACE,
    tools::DELETE_FILE,
    tools::UNIFIED_FILE,
];

const FILE_OPERATION_INDICATORS: &[&str] = &[
    "❋ Writing ",
    "❋ Editing ",
    "❋ Applying patch to ",
    "❋ Search/replace in ",
    "❋ Deleting ",
];

fn apply_active_file_operation_spinner(session: &Session, lines: &mut [Line<'static>]) {
    let Some(frame) = active_file_operation_spinner_frame(session) else {
        return;
    };

    for line in lines.iter_mut().rev() {
        if is_file_operation_indicator_line(line) && replace_indicator_icon(line, frame) {
            break;
        }
    }
}

fn active_file_operation_spinner_frame(session: &Session) -> Option<&'static str> {
    if !session.appearance.should_animate_progress_status() {
        return None;
    }

    let left = session.input_status_left.as_deref()?.to_ascii_lowercase();
    let is_active_file_tool = FILE_OPERATION_STATUS_TOOLS
        .iter()
        .any(|tool_name| left.contains(&format!("running tool: {tool_name}")));

    is_active_file_tool.then(|| spinner_frame_for_phase(session.shimmer_state.phase()))
}

fn is_file_operation_indicator_line(line: &Line<'_>) -> bool {
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    FILE_OPERATION_INDICATORS
        .iter()
        .any(|pattern| text.contains(pattern))
}

fn replace_indicator_icon(line: &mut Line<'static>, frame: &str) -> bool {
    let mut replaced = false;
    let mut new_spans = Vec::with_capacity(line.spans.len() + 2);

    for span in std::mem::take(&mut line.spans) {
        if replaced {
            new_spans.push(span);
            continue;
        }

        let style = span.style;
        let text = span.content.into_owned();
        let Some(icon_index) = text.find('❋') else {
            new_spans.push(Span::styled(text, style));
            continue;
        };

        let icon_end = icon_index + '❋'.len_utf8();
        if icon_index > 0 {
            new_spans.push(Span::styled(text[..icon_index].to_string(), style));
        }
        new_spans.push(Span::styled(frame.to_string(), style));
        if icon_end < text.len() {
            new_spans.push(Span::styled(text[icon_end..].to_string(), style));
        }
        replaced = true;
    }

    line.spans = new_spans;
    replaced
}

fn line_background(line: &Line<'_>) -> Option<Color> {
    line.spans.iter().find_map(|span| span.style.bg)
}

fn apply_full_width_line_backgrounds(buf: &mut Buffer, area: Rect, lines: &[Line<'_>]) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let max_rows = usize::from(area.height).min(lines.len());
    for (row, line) in lines.iter().take(max_rows).enumerate() {
        if let Some(bg) = line_background(line) {
            let row_rect = Rect::new(area.x, area.y + row as u16, area.width, 1);
            buf.set_style(row_rect, Style::default().bg(bg));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_tui::types::{InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme};
    use std::sync::Arc;

    fn segment(text: &str) -> InlineSegment {
        InlineSegment {
            text: text.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }
    }

    fn row_text(buf: &Buffer, area: Rect, row: u16) -> String {
        (area.left()..area.right())
            .map(|x| buf[(x, row)].symbol())
            .collect::<String>()
    }

    #[test]
    fn scroll_metric_invalidation_does_not_request_transcript_clear() {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.transcript_clear_required = false;

        session.invalidate_scroll_metrics();

        assert!(!session.transcript_clear_required);
    }

    #[test]
    fn render_clears_stale_wrapped_rows_when_requested() {
        let area = Rect::new(0, 0, 14, 6);
        let inner = Rect::new(1, 1, 12, 4);
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(
            InlineMessageKind::Agent,
            vec![segment("this line wraps across several rows")],
        );

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        let revision = session.next_revision();
        session.lines[0].segments = vec![segment("short")];
        session.lines[0].revision = revision;
        session.mark_line_dirty(0);
        session.invalidate_transcript_cache();
        for row in inner.y + 1..inner.bottom() {
            for x in inner.left()..inner.right() {
                buf[(x, row)].set_symbol("X");
            }
        }

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        assert!(
            (inner.y + 1..inner.bottom()).all(|row| row_text(&buf, inner, row).trim().is_empty())
        );
    }

    #[test]
    fn render_preserves_queue_overlay_lines() {
        let area = Rect::new(0, 0, 20, 6);
        let inner = Rect::new(1, 1, 18, 4);
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("alpha")]);
        session.push_queued_input("queued follow-up".to_string());

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        let bottom_row = row_text(&buf, inner, inner.bottom() - 1);
        assert!(bottom_row.contains("queued"));
    }

    #[test]
    fn render_clears_stale_queue_overlay_rows_when_queue_is_removed() {
        let area = Rect::new(0, 0, 20, 6);
        let inner = Rect::new(1, 1, 18, 4);
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("alpha")]);
        session.push_queued_input("queued follow-up".to_string());

        TranscriptWidget::new(&mut session).render(area, &mut buf);
        assert!(row_text(&buf, inner, inner.bottom() - 1).contains("queued"));

        let _ = session.pop_latest_queued_input();

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        assert!(row_text(&buf, inner, inner.bottom() - 1).trim().is_empty());
    }

    #[test]
    fn resize_larger_keeps_existing_transcript_lines_visible() {
        let small_area = Rect::new(0, 0, 20, 4);
        let large_area = Rect::new(0, 0, 20, 10);
        let small_inner = Rect::new(1, 1, 18, 2);
        let large_inner = Rect::new(1, 1, 18, 8);
        let mut small_buf = Buffer::empty(small_area);
        let mut large_buf = Buffer::empty(large_area);
        let mut session = Session::new(InlineTheme::default(), None, 12);

        for index in 0..6 {
            session.push_line(
                InlineMessageKind::Agent,
                vec![segment(&format!("line {index}"))],
            );
        }

        TranscriptWidget::new(&mut session).render(small_area, &mut small_buf);
        let small_rendered: Vec<String> = (small_inner.y..small_inner.bottom())
            .map(|row| row_text(&small_buf, small_inner, row).trim().to_string())
            .filter(|row| !row.is_empty())
            .collect();
        TranscriptWidget::new(&mut session).render(large_area, &mut large_buf);

        let rendered: Vec<String> = (large_inner.y..large_inner.bottom())
            .map(|row| row_text(&large_buf, large_inner, row).trim().to_string())
            .filter(|row| !row.is_empty())
            .collect();

        assert!(rendered.len() > small_rendered.len());
        assert!(rendered.iter().any(|row| row == "line 1"));
        assert!(rendered.iter().any(|row| row == "line 5"));
    }

    #[test]
    fn width_resize_keeps_transcript_visible() {
        let wide_area = Rect::new(0, 0, 28, 8);
        let narrow_area = Rect::new(0, 0, 16, 8);
        let narrow_inner = Rect::new(1, 1, 14, 6);
        let mut wide_buf = Buffer::empty(wide_area);
        let mut narrow_buf = Buffer::empty(narrow_area);
        let mut session = Session::new(InlineTheme::default(), None, 12);

        for index in 0..4 {
            session.push_line(
                InlineMessageKind::Agent,
                vec![segment(&format!("line {index}"))],
            );
        }

        TranscriptWidget::new(&mut session).render(wide_area, &mut wide_buf);
        TranscriptWidget::new(&mut session).render(narrow_area, &mut narrow_buf);

        let rendered: Vec<String> = (narrow_inner.y..narrow_inner.bottom())
            .map(|row| row_text(&narrow_buf, narrow_inner, row).trim().to_string())
            .filter(|row| !row.is_empty())
            .collect();

        assert!(!rendered.is_empty());
        assert!(rendered.iter().any(|row| row == "line 1"));
        assert!(rendered.iter().any(|row| row == "line 3"));
    }
}
