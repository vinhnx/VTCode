use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Clear, Paragraph, Widget},
};

use crate::config::constants::ui;
use crate::ui::tui::session::terminal_capabilities;
use crate::ui::tui::session::{
    Session, render::apply_transcript_rows, render::apply_transcript_width,
};

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
            return;
        }
        self.session.set_transcript_area(Some(inner));

        // Clamp effective dimensions to prevent pathological CPU usage with huge terminals
        // See: https://github.com/anthropics/claude-code/issues/21567
        let effective_height = inner.height.min(ui::TUI_MAX_VIEWPORT_HEIGHT);
        let effective_width = inner.width.min(ui::TUI_MAX_VIEWPORT_WIDTH);

        apply_transcript_rows(self.session, effective_height);

        let content_width = effective_width;
        if content_width == 0 {
            return;
        }
        apply_transcript_width(self.session, content_width);

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

        // Check if we need to mutate the lines (fill empty space or add queue overlay)
        let fill_count = viewport_rows.saturating_sub(cached_lines.len());
        let needs_mutation = fill_count > 0 || !self.session.queued_inputs.is_empty();

        // Build the lines Vec for Paragraph (which takes ownership)
        // Note: Clone is unavoidable because the cache holds a reference to the Arc
        let visible_lines = if needs_mutation {
            // Need to mutate, so clone and modify
            let mut lines = cached_lines.to_vec();
            if fill_count > 0 {
                let target_len = lines.len() + fill_count;
                lines.resize_with(target_len, ratatui::text::Line::default);
            }
            self.session.overlay_queue_lines(&mut lines, content_width);
            lines
        } else {
            // No mutation needed, just clone for Paragraph
            cached_lines.to_vec()
        };

        // Only clear if content actually changed, not on viewport-only scroll
        // This is a significant optimization: avoids expensive Clear operation on most scrolls
        if self.session.transcript_content_changed {
            Clear.render(scroll_area, buf);
            self.session.transcript_content_changed = false;
        }
        apply_full_width_line_backgrounds(buf, scroll_area, &visible_lines);
        let paragraph = Paragraph::new(visible_lines).style(self.session.styles.default_style());
        paragraph.render(scroll_area, buf);
    }
}

fn line_background(line: &ratatui::text::Line<'_>) -> Option<Color> {
    line.spans.iter().find_map(|span| span.style.bg)
}

fn apply_full_width_line_backgrounds(
    buf: &mut Buffer,
    area: Rect,
    lines: &[ratatui::text::Line<'_>],
) {
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
