use super::*;

impl Session {
    pub fn scroll_offset(&self) -> usize {
        self.scroll_manager.offset()
    }

    #[allow(dead_code)]
    pub(super) fn scroll_to_top(&mut self) {
        self.mark_scrolling();
        self.scroll_manager.scroll_to_top();
        self.mark_dirty();
    }

    #[allow(dead_code)]
    pub(super) fn scroll_to_bottom(&mut self) {
        self.mark_scrolling();
        self.scroll_manager.scroll_to_bottom();
        self.mark_dirty();
    }
}
