//! Viewer scrolling and scroll clamping.

use super::*;

impl ToolOutputViewerState {
    pub(crate) fn line_count(&self) -> usize {
        self.total_lines.max(1)
    }
    pub(crate) fn scroll_line_up(&mut self, height: u16) {
        self.scroll_by(-1, height);
    }
    pub(crate) fn scroll_line_down(&mut self, height: u16) {
        self.scroll_by(1, height);
    }
    pub(crate) fn scroll_half_page_up(&mut self, height: u16) {
        let half = (Self::page_step(height).max(1) / 2) as isize;
        self.scroll_by(-half, height);
    }
    pub(crate) fn scroll_half_page_down(&mut self, height: u16) {
        let half = (Self::page_step(height).max(1) / 2) as isize;
        self.scroll_by(half, height);
    }
    pub(crate) fn scroll_full_page_up(&mut self, height: u16) {
        self.scroll_by(-(Self::page_step(height) as isize), height);
    }
    pub(crate) fn scroll_full_page_down(&mut self, height: u16) {
        self.scroll_by(Self::page_step(height) as isize, height);
    }
    pub(crate) fn scroll_to_top(&mut self) {
        self.scroll_top = 0;
    }
    pub(crate) fn scroll_to_bottom(&mut self, height: u16) {
        self.scroll_top = self.max_scroll(height);
    }
    pub(crate) fn clamp_scroll(&mut self, height: u16) {
        self.scroll_top = self.scroll_top.min(self.max_scroll(height));
    }
    pub(crate) fn max_scroll(&self, height: u16) -> usize {
        self.total_lines.saturating_sub(usize::from(height.max(1)))
    }
    pub(crate) fn is_at_bottom(&self, height: u16) -> bool {
        self.scroll_top >= self.max_scroll(height)
    }
    pub(crate) fn scroll_by(&mut self, delta: isize, height: u16) {
        if delta < 0 {
            self.scroll_top = self.scroll_top.saturating_sub(delta.unsigned_abs());
            self.clamp_scroll(height);
        } else {
            self.scroll_top = self.scroll_top.saturating_add(delta as usize).min(self.max_scroll(height));
        }
    }
    pub(crate) fn page_step(height: u16) -> usize {
        usize::from(height.max(2)).saturating_sub(1)
    }
}
