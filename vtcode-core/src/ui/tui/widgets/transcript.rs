use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Clear, Paragraph, Widget, Wrap},
};

use crate::config::constants::ui;
use crate::ui::tui::session::{Session, render::apply_transcript_rows, render::apply_transcript_width};
use crate::ui::tui::session::terminal_capabilities;

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
            return;
        }

        let block = Block::new()
            .border_type(terminal_capabilities::get_border_type())
            .style(self.session.styles.default_style())
            .border_style(self.session.styles.border_style());

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        apply_transcript_rows(self.session, inner.height);

        let content_width = inner.width;
        if content_width == 0 {
            return;
        }
        apply_transcript_width(self.session, content_width);

        let viewport_rows = inner.height as usize;
        let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
        let effective_padding = padding.min(viewport_rows.saturating_sub(1));
        let total_rows = self.session.total_transcript_rows(content_width) + effective_padding;
        let (top_offset, _clamped_total_rows) =
            self.session.prepare_transcript_scroll(total_rows, viewport_rows);
        let vertical_offset = top_offset.min(self.session.scroll_manager.max_offset());
        self.session.transcript_view_top = vertical_offset;

        let visible_start = vertical_offset;
        let scroll_area = inner;

        // Use cached visible lines to avoid re-cloning on viewport-only scrolls
        let cached_lines = self.session.collect_transcript_window_cached(
            content_width,
            visible_start,
            viewport_rows,
        );

        // Only clone if we need to mutate (fill or overlay)
        let fill_count = viewport_rows.saturating_sub(cached_lines.len());
        let visible_lines = if fill_count > 0 || !self.session.queued_inputs.is_empty() {
            // Need to mutate, so clone from Arc
            let mut lines = (*cached_lines).clone();
            if fill_count > 0 {
                let target_len = lines.len() + fill_count;
                lines.resize_with(target_len, ratatui::text::Line::default);
            }
            self.session.overlay_queue_lines(&mut lines, content_width);
            lines
        } else {
            // No mutation needed, use Arc directly
            (*cached_lines).clone()
        };

        let paragraph = Paragraph::new(visible_lines)
            .style(self.session.styles.default_style())
            .wrap(Wrap { trim: true });

        // Only clear if content actually changed, not on viewport-only scroll
        // This is a significant optimization: avoids expensive Clear operation on most scrolls
        if self.session.transcript_content_changed {
            Clear.render(scroll_area, buf);
            self.session.transcript_content_changed = false;
        }
        paragraph.render(scroll_area, buf);
    }
}
