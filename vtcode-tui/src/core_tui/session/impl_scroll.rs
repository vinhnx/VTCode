use super::*;

impl Session {
    pub fn scroll_offset(&self) -> usize {
        self.scroll_manager.offset()
    }

    #[allow(dead_code)]
    pub(crate) fn scroll_to_top(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        let previous_offset = self.scroll_manager.offset();
        // Inverted model: max offset = top of content
        self.scroll_manager.scroll_to_bottom();
        let offset_delta = self.scroll_manager.offset() as i64 - previous_offset as i64;
        self.mouse_selection.adjust_for_scroll(offset_delta as i32);
        self.user_scrolled = true;
        self.mark_dirty();
    }

    #[allow(dead_code)]
    pub(crate) fn scroll_to_bottom(&mut self) {
        self.mark_scrolling();
        self.ensure_scroll_metrics();
        let previous_offset = self.scroll_manager.offset();
        // Inverted model: offset 0 = bottom of content
        self.scroll_manager.scroll_to_top();
        let offset_delta = self.scroll_manager.offset() as i64 - previous_offset as i64;
        self.mouse_selection.adjust_for_scroll(offset_delta as i32);
        self.user_scrolled = false;
        self.mark_dirty();
    }
}
