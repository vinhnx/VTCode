use super::*;

impl Session {
    pub fn scroll_offset(&self) -> usize {
        self.scroll_manager.offset()
    }

    #[allow(dead_code)]
    pub(crate) fn scroll_to_top(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        // Inverted model: max offset = top of content
        self.scroll_manager.scroll_to_bottom();
        self.user_scrolled = true;
        self.mark_dirty();
    }

    #[allow(dead_code)]
    pub(crate) fn scroll_to_bottom(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        // Inverted model: offset 0 = bottom of content
        self.scroll_manager.scroll_to_top();
        self.user_scrolled = false;
        self.mark_dirty();
    }
}
