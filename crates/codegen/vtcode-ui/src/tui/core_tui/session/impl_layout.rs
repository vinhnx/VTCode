use super::*;
use crate::tui::ui::tui::widgets::LayoutMode;

impl Session {
    pub(crate) fn resolved_layout_mode(&self, area: Rect) -> LayoutMode {
        match self.appearance.layout_mode {
            config::LayoutModeOverride::Auto => LayoutMode::from_area(area),
            config::LayoutModeOverride::Compact => LayoutMode::Compact,
            config::LayoutModeOverride::Standard => LayoutMode::Standard,
            config::LayoutModeOverride::Wide => LayoutMode::Wide,
        }
    }

    pub(crate) fn apply_view_rows(&mut self, rows: u16) {
        let resolved = rows.max(2);
        if self.view_rows != resolved {
            self.view_rows = resolved;
            self.invalidate_scroll_metrics();
        }
        self.recalculate_transcript_rows();
        self.enforce_scroll_bounds();
    }

    pub(crate) fn apply_transcript_rows(&mut self, rows: u16) {
        let resolved = rows.max(1);
        if self.transcript_rows != resolved {
            let anchor = self.transcript_scroll_anchor();
            self.transcript_rows = resolved;
            self.invalidate_scroll_metrics();
            self.restore_transcript_scroll_anchor(anchor);
        }
    }

    pub(crate) fn apply_transcript_width(&mut self, width: u16) {
        if self.transcript_width != width {
            let anchor = self.transcript_scroll_anchor();
            self.transcript_width = width;
            self.invalidate_scroll_metrics();
            self.restore_transcript_scroll_anchor(anchor);
            // The hidden-header line is built against `transcript_width`: the
            // right-aligned mode/model summary only renders when width > 0.
            // On the first frame `transcript_width` is still 0, so the line is
            // cached without the summary. Invalidate that cache and request a
            // redraw so the next frame recomputes it with the real width,
            // instead of waiting for an unrelated keypress to flush it.
            self.header_lines_cache = None;
            self.header_block_title_cache = None;
            self.mark_visual_dirty();
        }
    }

    pub(crate) fn force_view_rows(&mut self, rows: u16) {
        self.apply_view_rows(rows);
    }

    pub(crate) fn recalculate_transcript_rows(&mut self) {
        // Calculate reserved rows: header + input + borders (2)
        let header_rows = self.header_rows.max(ui::INLINE_HEADER_HEIGHT);
        let reserved = (header_rows + self.input_height).saturating_add(2);
        let available = self.view_rows.saturating_sub(reserved).max(1);
        self.apply_transcript_rows(available);
    }
}
