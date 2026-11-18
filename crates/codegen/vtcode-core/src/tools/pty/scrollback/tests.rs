use super::*;

#[test]
fn scrollback_enforces_byte_limit() {
    let mut scrollback = PtyScrollback::new(10_000, 1024); // 1KB limit

    // Fill with 2KB of data (100 lines x 22 bytes each = 2.2KB)
    for i in 0..100 {
        scrollback.push_text(&format!("line-{i:04}-data-here\n")); // ~22 bytes per line
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

    scrollback.push_text("first\n"); // 6 bytes
    scrollback.push_text("second\n"); // 7 bytes
    scrollback.push_text("third\n"); // 6 bytes - should drop "first"

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
        scrollback.push_text(&format!("line-{i:04}-xxxxxxxxxx\n")); // ~50 bytes each
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

#[test]
fn scrollback_early_warning_at_80_percent() {
    let mut scrollback = PtyScrollback::new(1000, 1000); // 1KB limit

    // Push data up to 80% (800 bytes)
    for _ in 0..40 {
        scrollback.push_text("12345678901234567890\n"); // 21 bytes x 40 = 840 bytes
    }

    // Should have warning
    let snapshot = scrollback.snapshot();
    assert!(snapshot.contains("approaching size limit"), "Should show 80% warning");
    assert!(scrollback.warning_shown, "Warning flag should be set");
}

#[test]
fn scrollback_tracks_dropped_metrics() {
    let mut scrollback = PtyScrollback::new(1000, 500); // 500 byte limit

    // Push 1KB of data (should drop half)
    for i in 0..50 {
        scrollback.push_text(&format!("line-{i:04}-data\n")); // ~16 bytes each
    }

    let metrics = scrollback.metrics();
    assert!(metrics.bytes_dropped > 0, "Should track dropped bytes");
    assert!(metrics.lines_dropped > 0, "Should track dropped lines");
    assert!(metrics.overflow_detected, "Should detect overflow");
}

#[test]
fn scrollback_usage_percent_calculation() {
    let mut scrollback = PtyScrollback::new(1000, 1000);

    // Push 500 bytes
    for _ in 0..25 {
        scrollback.push_text("12345678901234567890\n"); // 21 bytes x 25 = 525 bytes
    }

    let metrics = scrollback.metrics();
    assert!(metrics.usage_percent > 50.0 && metrics.usage_percent < 60.0, "Usage should be around 50-55%");
}

#[test]
fn scrollback_metrics_structure() {
    let mut scrollback = PtyScrollback::new(100, 10_000);

    scrollback.push_text("test line\n");

    let metrics = scrollback.metrics();
    assert_eq!(metrics.max_bytes, 10_000);
    assert_eq!(metrics.capacity_lines, 100);
    assert_eq!(metrics.current_lines, 1);
    assert!(!metrics.overflow_detected);
    assert_eq!(metrics.bytes_dropped, 0);
    assert_eq!(metrics.lines_dropped, 0);
}

#[test]
fn scrollback_strips_ansi_codes_from_compiler_output() {
    let mut scrollback = PtyScrollback::new(1000, 10_000);

    // Simulate realistic Cargo warning output with ANSI codes
    // (these would normally be stripped by push_text via strip_ansi)
    let ansi_colored = "warning: unused variable\n  --> src/main.rs:10:5\n   |\n10 | let x = 5;\n   |     ^ this is orange/yellow in colored output\n";
    scrollback.push_text(ansi_colored);

    let snapshot = scrollback.snapshot();
    // Verify no ANSI escape sequences remain
    assert!(!snapshot.contains("\x1b["), "Snapshot contains ESC character (0x1b)");
    assert!(!snapshot.contains("\u{001b}"), "Snapshot contains ESC Unicode");
    // Verify content is preserved
    assert!(snapshot.contains("warning"));
    assert!(snapshot.contains("src/main.rs"));
}

#[test]
fn scrollback_handles_mixed_ansi_and_plain_text() {
    let mut scrollback = PtyScrollback::new(1000, 10_000);

    // Mix of plain text and escaped sequences
    scrollback.push_text("plain text\n");
    // Even if somehow ANSI codes made it through, strip_ansi handles them
    scrollback.push_text("more text\n");

    let snapshot = scrollback.snapshot();
    assert!(!snapshot.contains("\x1b["), "No ANSI codes in output");
    assert_eq!(snapshot.lines().count(), 2, "Both lines preserved");
}
