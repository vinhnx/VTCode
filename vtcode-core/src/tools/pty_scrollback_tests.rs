#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_enforces_byte_limit() {
        let mut scrollback = PtyScrollback::new(10_000, 1024); // 1KB limit

        // Fill with 2KB of data (100 lines x 22 bytes each = 2.2KB)
        for i in 0..100 {
            scrollback.push_text(&format!("line-{:04}-data-here\n", i)); // ~22 bytes per line
        }

        // Should have stopped accepting data after hitting limit
        assert!(scrollback.has_overflow());
        assert!(scrollback.current_size_bytes() <= 1024);
        
        // Snapshot should contain overflow warning
        let snapshot = scrollback.snapshot();
        assert!(snapshot.contains("Output size limit exceeded"));
    }

    #[test]
    fn scrollback_circular_buffer_drops_oldest() {
        let mut scrollback = PtyScrollback::new(3, 10_000); // 3 lines, 10KB max

        scrollback.push_text("line1\n");
        scrollback.push_text("line2\n");
        scrollback.push_text("line3\n");
        scrollback.push_text("line4\n"); // Should drop line1

        let snapshot = scrollback.snapshot();
        assert!(!snapshot.contains("line1"), "line1 should be dropped");
        assert!(snapshot.contains("line4"), "line4 should be present");
        assert_eq!(scrollback.lines.len(), 3);
    }

    #[test]
    fn scrollback_tracks_bytes_correctly() {
        let mut scrollback = PtyScrollback::new(100, 10_000);

        scrollback.push_text("hello\n"); // 6 bytes
        assert_eq!(scrollback.current_size_bytes(), 6);

        scrollback.push_text("world\n"); // 6 bytes
        assert_eq!(scrollback.current_size_bytes(), 12);
    }

    #[test]
    fn scrollback_drops_oldest_when_line_limit_exceeded() {
        let mut scrollback = PtyScrollback::new(2, 10_000); // Only 2 lines allowed

        scrollback.push_text("first\n");  // 6 bytes
        scrollback.push_text("second\n"); // 7 bytes
        scrollback.push_text("third\n");  // 6 bytes - should drop "first"

        assert_eq!(scrollback.lines.len(), 2);
        // Should only have second + third = 13 bytes
        assert_eq!(scrollback.current_size_bytes(), 13);
        
        let snapshot = scrollback.snapshot();
        assert!(!snapshot.contains("first"));
        assert!(snapshot.contains("second"));
        assert!(snapshot.contains("third"));
    }

    #[test]
    fn scrollback_no_overflow_under_limit() {
        let mut scrollback = PtyScrollback::new(1000, 10_000); // 10KB limit

        // Push 5KB of data
        for i in 0..100 {
            scrollback.push_text(&format!("line-{:04}-xxxxxxxxxx\n", i)); // ~50 bytes each
        }

        assert!(!scrollback.has_overflow());
        assert!(scrollback.current_size_bytes() < 10_000);
    }

    #[test]
    fn scrollback_pending_operations() {
        let mut scrollback = PtyScrollback::new(100, 10_000);

        scrollback.push_text("line1\n");
        scrollback.push_text("line2\n");

        // Pending should show both lines
        let pending = scrollback.pending();
        assert!(pending.contains("line1"));
        assert!(pending.contains("line2"));

        // Take pending should return and clear
        let taken = scrollback.take_pending();
        assert!(taken.contains("line1"));
        assert!(taken.contains("line2"));

        // After taking, pending should be empty
        let empty = scrollback.pending();
        assert!(empty.is_empty());
    }
}
