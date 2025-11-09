/// Scroll state management for transcript views
///
/// Handles viewport scrolling, metrics caching, and bounds enforcement
/// for transcript content that may exceed the visible area.

use std::cmp::min;

/// Manages scrolling state for a transcript or list view
#[derive(Clone, Debug)]
pub struct ScrollManager {
    /// Current scroll offset from top
    offset: usize,
    /// Cached maximum scroll offset
    max_offset: usize,
    /// Cached total number of lines in content
    total_rows: usize,
    /// Current viewport height
    viewport_rows: u16,
    /// Whether metrics cache is valid
    metrics_dirty: bool,
}

impl ScrollManager {
    /// Creates a new scroll manager with given viewport height
    pub fn new(viewport_rows: u16) -> Self {
        Self {
            offset: 0,
            max_offset: 0,
            total_rows: 0,
            viewport_rows: viewport_rows.max(1),
            metrics_dirty: true,
        }
    }

    /// Returns current scroll offset
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Sets scroll offset (clamped to valid range)
    pub fn set_offset(&mut self, offset: usize) {
        self.offset = min(offset, self.max_offset);
    }

    /// Returns maximum scroll offset
    pub fn max_offset(&self) -> usize {
        self.max_offset
    }

    /// Returns current viewport height
    pub fn viewport_rows(&self) -> u16 {
        self.viewport_rows
    }

    /// Updates viewport height and invalidates metrics
    pub fn set_viewport_rows(&mut self, rows: u16) {
        let rows = rows.max(1);
        if self.viewport_rows != rows {
            self.viewport_rows = rows;
            self.metrics_dirty = true;
        }
    }

    /// Returns total rows in content
    pub fn total_rows(&self) -> usize {
        self.total_rows
    }

    /// Updates total rows and max offset, returns if changed
    pub fn set_total_rows(&mut self, total: usize) -> bool {
        if self.total_rows != total {
            self.total_rows = total;
            self.update_max_offset();
            self.metrics_dirty = false;
            true
        } else {
            false
        }
    }

    /// Invalidates metrics cache (e.g., due to theme/width changes)
    pub fn invalidate_metrics(&mut self) {
        self.metrics_dirty = true;
    }

    /// Returns whether metrics cache is valid
    pub fn metrics_valid(&self) -> bool {
        !self.metrics_dirty
    }

    /// Scrolls up by a number of lines
    pub fn scroll_up(&mut self, lines: usize) {
        self.offset = self.offset.saturating_sub(lines);
    }

    /// Scrolls down by a number of lines
    pub fn scroll_down(&mut self, lines: usize) {
        self.offset = min(self.offset + lines, self.max_offset);
    }

    /// Scrolls up by one page
    pub fn scroll_page_up(&mut self) {
        self.scroll_up(self.viewport_rows.saturating_sub(1) as usize);
    }

    /// Scrolls down by one page
    pub fn scroll_page_down(&mut self) {
        self.scroll_down(self.viewport_rows.saturating_sub(1) as usize);
    }

    /// Scrolls to the top
    pub fn scroll_to_top(&mut self) {
        self.offset = 0;
    }

    /// Scrolls to the bottom
    pub fn scroll_to_bottom(&mut self) {
        self.offset = self.max_offset;
    }

    /// Checks if scrolled to the top
    pub fn at_top(&self) -> bool {
        self.offset == 0
    }

    /// Checks if scrolled to the bottom
    pub fn at_bottom(&self) -> bool {
        self.offset >= self.max_offset
    }

    /// Returns scroll progress as percentage (0-100)
    pub fn progress_percent(&self) -> u8 {
        if self.max_offset == 0 {
            100
        } else {
            ((self.offset as f32 / self.max_offset as f32) * 100.0) as u8
        }
    }

    /// Computes the maximum scroll offset given total rows and viewport
    fn update_max_offset(&mut self) {
        let viewport = self.viewport_rows as usize;
        self.max_offset = self.total_rows.saturating_sub(viewport).max(0);
    }

    /// Enforces scroll bounds after external changes
    pub fn clamp_offset(&mut self) {
        if self.offset > self.max_offset {
            self.offset = self.max_offset;
        }
    }

    /// Returns the visible range (start, end)
    pub fn visible_range(&self) -> (usize, usize) {
        let start = self.offset;
        let end = min(self.offset + self.viewport_rows as usize, self.total_rows);
        (start, end)
    }

    /// Returns number of visible lines
    pub fn visible_count(&self) -> usize {
        let (start, end) = self.visible_range();
        end - start
    }
}

impl Default for ScrollManager {
    fn default() -> Self {
        Self::new(10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_scroll_manager() {
        let manager = ScrollManager::new(10);
        assert_eq!(manager.offset(), 0);
        assert_eq!(manager.viewport_rows(), 10);
        assert!(manager.at_top());
    }

    #[test]
    fn scroll_down() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(100);
        manager.scroll_down(5);
        assert_eq!(manager.offset(), 5);
    }

    #[test]
    fn scroll_down_clamped_at_max() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(20);
        manager.scroll_down(100);
        assert_eq!(manager.offset(), manager.max_offset());
        assert!(manager.at_bottom());
    }

    #[test]
    fn scroll_up() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(100);
        manager.set_offset(50);
        manager.scroll_up(20);
        assert_eq!(manager.offset(), 30);
    }

    #[test]
    fn scroll_up_clamped_at_zero() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(100);
        manager.set_offset(50);
        manager.scroll_up(100);
        assert_eq!(manager.offset(), 0);
        assert!(manager.at_top());
    }

    #[test]
    fn page_navigation() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(100);

        manager.scroll_page_down();
        assert_eq!(manager.offset(), 9); // viewport - 1

        manager.scroll_page_up();
        assert_eq!(manager.offset(), 0);
    }

    #[test]
    fn visible_range() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(100);
        manager.set_offset(20);

        let (start, end) = manager.visible_range();
        assert_eq!(start, 20);
        assert_eq!(end, 30);
        assert_eq!(manager.visible_count(), 10);
    }

    #[test]
    fn progress_calculation() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(100);

        assert_eq!(manager.progress_percent(), 0);

        manager.set_offset(50);
        assert_eq!(manager.progress_percent(), 50);

        manager.scroll_to_bottom();
        assert_eq!(manager.progress_percent(), 100);
    }

    #[test]
    fn metrics_invalidation() {
        let mut manager = ScrollManager::new(10);
        assert!(manager.metrics_dirty);

        manager.set_total_rows(50);
        assert!(!manager.metrics_dirty);

        manager.invalidate_metrics();
        assert!(manager.metrics_dirty);
    }

    #[test]
    fn viewport_change() {
        let mut manager = ScrollManager::new(10);
        manager.set_total_rows(100);
        manager.set_offset(50);

        manager.set_viewport_rows(20);
        assert!(manager.metrics_dirty);
        assert_eq!(manager.viewport_rows(), 20);
    }
}
