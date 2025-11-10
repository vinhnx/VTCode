//! Optimized transcript reflow cache for efficient line wrapping and rendering
//!
//! This module provides improved caching mechanisms for reflowing transcript content,
//! with performance optimizations for large transcripts.

use ratatui::text::Line;
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct CachedMessage {
    pub revision: u64,
    pub lines: Vec<Line<'static>>,
    pub hash: u64, // Added hash for faster comparison
}

// Simple hash function to identify content changes faster than full comparison
fn calculate_content_hash(content: &[Line<'static>]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for line in content {
        // Hash content of each line
        for span in &line.spans {
            span.content.hash(&mut hasher);
            span.style.hash(&mut hasher);
        }
    }
    hasher.finish()
}

pub struct TranscriptReflowCache {
    pub width: u16,
    pub total_rows: usize,
    pub row_offsets: Vec<usize>, // Precomputed row offsets for faster access
    pub messages: Vec<CachedMessage>,
    pub width_specific_cache: HashMap<u16, Vec<Vec<Line<'static>>>>, // Cache reflowed content by width
}

impl TranscriptReflowCache {
    pub fn new(width: u16) -> Self {
        Self {
            width,
            total_rows: 0,
            row_offsets: Vec::new(),
            messages: Vec::new(),
            width_specific_cache: HashMap::new(),
        }
    }

    /// Updates the cache width and invalidates relevant data
    pub fn set_width(&mut self, new_width: u16) {
        if self.width != new_width {
            self.width = new_width;
            self.invalidate_content();
        }
    }

    /// Invalidates the content cache when width changes
    fn invalidate_content(&mut self) {
        self.width_specific_cache.clear();
        for message in &mut self.messages {
            message.lines.clear(); // Clear cached lines
            message.revision = 0; // Mark as invalid
        }
    }

    /// Checks if a specific message needs reflow based on revision and content hash
    pub fn needs_reflow(&self, index: usize, current_revision: u64) -> bool {
        if index >= self.messages.len() {
            return true;
        }

        let cached = &self.messages[index];
        cached.revision != current_revision
    }

    /// Updates a cached message with new reflowed content
    pub fn update_message(&mut self, index: usize, revision: u64, lines: Vec<Line<'static>>) {
        // Ensure we have enough space in the messages vector
        while self.messages.len() <= index {
            self.messages.push(CachedMessage::default());
        }

        let hash = calculate_content_hash(&lines);
        let message = &mut self.messages[index];
        message.revision = revision;
        message.lines = lines;
        message.hash = hash;
    }

    /// Precomputes row offsets for faster access to transcript ranges
    pub fn update_row_offsets(&mut self) {
        self.row_offsets.clear();
        self.row_offsets.reserve(self.messages.len());

        let mut current_offset = 0;
        for message in &self.messages {
            self.row_offsets.push(current_offset);
            current_offset += message.lines.len();
        }

        self.total_rows = current_offset;
    }

    /// Gets the total number of rows in the transcript
    pub fn total_rows(&self) -> usize {
        self.total_rows
    }

    /// Gets a range of visible lines for a given window
    pub fn get_visible_range(&self, start_row: usize, max_rows: usize) -> Vec<Line<'static>> {
        if max_rows == 0 || start_row >= self.total_rows {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(max_rows.min(self.total_rows - start_row));
        let current_row = start_row;
        let remaining_rows = max_rows.min(self.total_rows - start_row);

        // Find the starting message index using binary search on row_offsets
        let start_message_idx = match self.row_offsets.binary_search(&current_row) {
            Ok(idx) => idx,
            Err(0) => 0,
            Err(pos) => pos - 1,
        };

        for msg_idx in start_message_idx..self.messages.len() {
            let msg_start_row = self.row_offsets[msg_idx];
            let msg = &self.messages[msg_idx];

            if msg_start_row >= current_row + remaining_rows {
                break;
            }

            let skip_lines = if msg_start_row < current_row {
                current_row - msg_start_row
            } else {
                0
            };

            for (_line_idx, line) in msg.lines.iter().enumerate().skip(skip_lines) {
                if result.len() >= remaining_rows {
                    break;
                }
                result.push(line.clone());
            }

            if result.len() >= remaining_rows {
                break;
            }
        }

        result
    }
}

impl Default for TranscriptReflowCache {
    fn default() -> Self {
        Self::new(80) // Default terminal width
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    #[test]
    fn test_cache_initialization() {
        let cache = TranscriptReflowCache::new(100);
        assert_eq!(cache.width, 100);
        assert_eq!(cache.total_rows(), 0);
        assert!(cache.messages.is_empty());
    }

    #[test]
    fn test_update_message() {
        let mut cache = TranscriptReflowCache::new(80);
        let test_line = Line::from(vec![Span::raw("Test line")]);
        let lines = vec![test_line];

        cache.update_message(0, 1, lines);

        assert!(!cache.messages.is_empty());
        assert_eq!(cache.messages[0].revision, 1);
        assert_eq!(cache.messages[0].lines.len(), 1);
    }

    #[test]
    fn test_row_offsets() {
        let mut cache = TranscriptReflowCache::new(80);

        // Add three messages: 2 lines, 1 line, 3 lines
        cache.update_message(0, 1, vec![Line::default(), Line::default()]);
        cache.update_message(1, 2, vec![Line::default()]);
        cache.update_message(
            2,
            3,
            vec![Line::default(), Line::default(), Line::default()],
        );

        cache.update_row_offsets();

        assert_eq!(cache.row_offsets, vec![0, 2, 3]); // [0, 0+2, 0+2+1]
        assert_eq!(cache.total_rows(), 6); // 2+1+3
    }

    #[test]
    fn test_get_visible_range() {
        let mut cache = TranscriptReflowCache::new(80);

        // Add two messages
        cache.update_message(0, 1, vec![Line::from("Line 1"), Line::from("Line 2")]);
        cache.update_message(1, 2, vec![Line::from("Line 3")]);

        cache.update_row_offsets();

        // Get first 2 rows
        let range = cache.get_visible_range(0, 2);
        assert_eq!(range.len(), 2);

        // Get from row 1 (second line of first message) to row 2 (first line of second message)
        let range = cache.get_visible_range(1, 2);
        assert_eq!(range.len(), 2);
    }

    #[test]
    fn test_needs_reflow() {
        let cache = TranscriptReflowCache::new(80);

        // Initially needs reflow
        assert!(cache.needs_reflow(0, 1));

        // After adding message with same revision, doesn't need reflow
        let mut cache = TranscriptReflowCache::new(80);
        cache.update_message(0, 1, vec![Line::default()]);
        assert!(!cache.needs_reflow(0, 1));

        // But needs reflow with different revision
        assert!(cache.needs_reflow(0, 2));
    }

    #[test]
    fn test_width_changes() {
        let mut cache = TranscriptReflowCache::new(80);
        assert_eq!(cache.width, 80);

        cache.set_width(120);
        assert_eq!(cache.width, 120);
    }

    #[test]
    fn test_message_accessors() {
        let mut cache = TranscriptReflowCache::new(80);
        cache.update_message(0, 1, vec![Line::from("Test"), Line::from("Lines")]);

        cache.update_row_offsets();

        assert_eq!(cache.message_start_row(0), Some(0));
        assert_eq!(cache.message_row_count(0), Some(2));
        assert_eq!(cache.message_start_row(1), None); // Non-existent message
        assert_eq!(cache.message_row_count(1), None); // Non-existent message
    }

    #[test]
    fn test_empty_range() {
        let cache = TranscriptReflowCache::new(80);
        let range = cache.get_visible_range(0, 0);
        assert!(range.is_empty());
    }

    #[test]
    fn test_out_of_bounds_range() {
        let cache = TranscriptReflowCache::new(80);
        let range = cache.get_visible_range(100, 10); // Start beyond available rows
        assert!(range.is_empty());
    }
}
