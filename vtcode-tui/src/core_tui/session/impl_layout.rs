use super::*;
use crate::ui::tui::widgets::LayoutMode;

impl Session {
    pub(crate) fn resolved_layout_mode(&self, area: Rect) -> LayoutMode {
        match self.appearance.layout_mode {
            crate::ui::tui::session::config::LayoutModeOverride::Auto => {
                LayoutMode::from_area(area)
            }
            crate::ui::tui::session::config::LayoutModeOverride::Compact => LayoutMode::Compact,
            crate::ui::tui::session::config::LayoutModeOverride::Standard => LayoutMode::Standard,
            crate::ui::tui::session::config::LayoutModeOverride::Wide => LayoutMode::Wide,
        }
    }

    pub fn apply_view_rows(&mut self, rows: u16) {
        let resolved = rows.max(2);
        if self.view_rows != resolved {
            self.view_rows = resolved;
            self.invalidate_scroll_metrics();
        }
        self.recalculate_transcript_rows();
        self.enforce_scroll_bounds();
    }

    #[allow(dead_code)]
    pub(crate) fn force_view_rows(&mut self, rows: u16) {
        self.apply_view_rows(rows);
    }

    pub(super) fn recalculate_transcript_rows(&mut self) {
        // Calculate reserved rows: header + input + borders (2)
        let header_rows = self.header_rows.max(ui::INLINE_HEADER_HEIGHT);
        let reserved = (header_rows + self.input_height).saturating_add(2);
        let available = self.view_rows.saturating_sub(reserved).max(1);

        if self.transcript_rows != available {
            self.transcript_rows = available;
            self.invalidate_scroll_metrics();
        }
    }
}
