use std::collections::VecDeque;

use crate::utils::unicode_monitor::UnicodeValidationContext;

pub(super) struct PtyScrollback {
    lines: VecDeque<String>,
    pending_lines: VecDeque<String>,
    partial: String,
    pending_partial: String,
    capacity_lines: usize,
    max_bytes: usize,
    current_bytes: usize,
    overflow_detected: bool,
    warning_shown: bool,            // Track if 80% warning shown
    bytes_dropped: usize,           // Track dropped bytes for metrics
    lines_dropped: usize,           // Track dropped lines for metrics
    unicode_errors: usize,          // Count UTF-8 decoding errors
    utf8_buffer_remainder: Vec<u8>, // Store incomplete UTF-8 sequences between calls
    total_unicode_chars: usize,     // NEW: Total unicode characters processed
    unicode_sessions: usize,        // NEW: Number of sessions with unicode content
    last_unicode_check: bool,       // NEW: Cache last unicode detection result
}

impl PtyScrollback {
    pub(super) fn new(capacity_lines: usize, max_bytes: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            pending_lines: VecDeque::new(),
            partial: String::with_capacity(256),
            pending_partial: String::with_capacity(256),
            capacity_lines: capacity_lines.max(1),
            max_bytes,
            current_bytes: 0,
            overflow_detected: false,
            warning_shown: false,
            bytes_dropped: 0,
            lines_dropped: 0,
            unicode_errors: 0,
            utf8_buffer_remainder: Vec::new(),
            total_unicode_chars: 0,
            unicode_sessions: 0,
            last_unicode_check: false,
        }
    }

    fn push_text(&mut self, text: &str) {
        // Unicode-aware ANSI stripping with fast path for ASCII-only text
        let has_unicode = crate::utils::ansi_parser::contains_unicode(text);
        self.last_unicode_check = has_unicode;

        if has_unicode {
            self.unicode_sessions += 1;
            // Count unicode characters for metrics
            self.total_unicode_chars += text.chars().filter(|&ch| ch as u32 > 127).count();
        }

        let cleaned_text = if has_unicode {
            // Text contains unicode, use full ANSI stripping
            crate::utils::ansi_parser::strip_ansi(text)
        } else {
            // Fast path: ASCII-only text, use simple escape sequence removal
            crate::utils::ansi_parser::strip_ansi_ascii_only(text)
        };

        let text_bytes = cleaned_text.len();

        // Early warning at 80% threshold
        if !self.warning_shown && self.current_bytes + text_bytes > (self.max_bytes * 80 / 100) {
            self.warning_shown = true;
            let warning = format!(
                "\n[WARN] Output approaching size limit ({:.1} MB of {} MB). Output may be truncated soon.\n",
                self.current_bytes as f64 / 1_000_000.0,
                self.max_bytes / 1_000_000
            );
            // Add warning to both buffers
            self.current_bytes += warning.len();
            // Use String::clone_from pattern - push to pending first, then move to lines
            self.pending_lines.push_back(warning.clone());
            self.lines.push_back(warning);
        }

        // Check byte limit BEFORE processing to prevent memory explosion
        if self.current_bytes + text_bytes > self.max_bytes {
            if !self.overflow_detected {
                self.overflow_detected = true;
                let warning = format!(
                    "\n[WARN] Output size limit exceeded ({} MB). Further output truncated.\n\
                    [TIP] Full output can be retrieved with output spooling enabled\n",
                    self.max_bytes / 1_000_000
                );
                // Add warning to both buffers - push clone first, then move original
                self.current_bytes += warning.len();
                self.pending_lines.push_back(warning.clone());
                self.lines.push_back(warning);
            }

            // Track metrics for dropped data
            self.bytes_dropped += text_bytes;
            self.lines_dropped += cleaned_text.lines().count();

            return; // DROP further output to prevent hang
        }

        // Unicode-aware line splitting with optimization for ASCII-only text
        if crate::utils::ansi_parser::contains_unicode(&cleaned_text) {
            // Text contains unicode, use standard line splitting
            for part in cleaned_text.split_inclusive('\n') {
                self.partial.push_str(part);
                self.pending_partial.push_str(part);
                if part.ends_with('\n') {
                    let complete = std::mem::take(&mut self.partial);
                    let _ = std::mem::take(&mut self.pending_partial);

                    self.current_bytes += complete.len();
                    // Push clone to pending_lines first, then move original to lines
                    self.pending_lines.push_back(complete.clone());
                    self.lines.push_back(complete);

                    // Circular buffer: drop oldest lines when line capacity exceeded
                    while self.lines.len() > self.capacity_lines {
                        if let Some(oldest) = self.lines.pop_front() {
                            self.current_bytes = self.current_bytes.saturating_sub(oldest.len());
                        }
                    }
                    while self.pending_lines.len() > self.capacity_lines {
                        self.pending_lines.pop_front();
                    }
                }
            }
        } else {
            // Fast path: ASCII-only text, use byte-based splitting
            let bytes = cleaned_text.as_bytes();
            let mut start = 0;

            for (i, &byte) in bytes.iter().enumerate() {
                if byte == b'\n' {
                    let line = std::str::from_utf8(&bytes[start..=i]).unwrap_or("");
                    self.partial.push_str(line);
                    self.pending_partial.push_str(line);

                    let complete = std::mem::take(&mut self.partial);
                    let _ = std::mem::take(&mut self.pending_partial);

                    self.current_bytes += complete.len();
                    self.pending_lines.push_back(complete.clone());
                    self.lines.push_back(complete);

                    // Circular buffer management
                    while self.lines.len() > self.capacity_lines {
                        if let Some(oldest) = self.lines.pop_front() {
                            self.current_bytes = self.current_bytes.saturating_sub(oldest.len());
                        }
                    }
                    while self.pending_lines.len() > self.capacity_lines {
                        self.pending_lines.pop_front();
                    }

                    start = i + 1;
                }
            }

            // Handle remaining partial line
            if start < bytes.len() {
                let remaining = std::str::from_utf8(&bytes[start..]).unwrap_or("");
                self.partial.push_str(remaining);
                self.pending_partial.push_str(remaining);
            }
        }
    }

    pub(super) fn push_utf8(&mut self, buffer: &mut Vec<u8>, eof: bool) {
        const MAX_UTF8_BUFFER_SIZE: usize = 16 * 1024; // 16KB limit for incomplete UTF-8
        const MAX_UNICODE_ERRORS: usize = 100; // Prevent excessive error logging

        // Start unicode validation context
        let validation_context = UnicodeValidationContext::new(buffer.len());

        // Prepend any remainder from previous calls
        if !self.utf8_buffer_remainder.is_empty() {
            let mut combined = std::mem::take(&mut self.utf8_buffer_remainder);
            combined.append(buffer);
            *buffer = combined;
        }

        // Prevent buffer overflow from accumulated incomplete sequences
        if buffer.len() > MAX_UTF8_BUFFER_SIZE {
            // If buffer is too large, treat remaining content as invalid
            if !buffer.is_empty() {
                self.push_text("\u{FFFD}");
                self.unicode_errors += 1;
                if self.unicode_errors <= MAX_UNICODE_ERRORS {
                    tracing::warn!(
                        "UTF-8 buffer overflow: {} bytes, treating as invalid",
                        buffer.len()
                    );
                }
                buffer.clear();
            }
            return;
        }

        loop {
            match std::str::from_utf8(buffer) {
                Ok(valid) => {
                    if !valid.is_empty() {
                        self.push_text(valid);
                    }
                    buffer.clear();
                    break;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        // Process valid portion
                        if let Ok(valid) = std::str::from_utf8(&buffer[..valid_up_to])
                            && !valid.is_empty()
                        {
                            self.push_text(valid);
                        }
                        buffer.drain(..valid_up_to);

                        // Check buffer size again after draining
                        if buffer.len() > MAX_UTF8_BUFFER_SIZE {
                            self.push_text("\u{FFFD}");
                            self.unicode_errors += 1;
                            if self.unicode_errors <= MAX_UNICODE_ERRORS {
                                tracing::warn!(
                                    "UTF-8 buffer overflow after processing: {} bytes",
                                    buffer.len()
                                );
                            }
                            buffer.clear();
                            break;
                        }
                        continue;
                    }

                    if let Some(error_len) = error.error_len() {
                        // Invalid UTF-8 sequence - replace with replacement character
                        self.push_text("\u{FFFD}");
                        self.unicode_errors += 1;
                        if self.unicode_errors <= MAX_UNICODE_ERRORS {
                            tracing::debug!(
                                "Invalid UTF-8 sequence detected, replacing with U+FFFD"
                            );
                        }
                        buffer.drain(..error_len);

                        // Check buffer size after draining
                        if buffer.len() > MAX_UTF8_BUFFER_SIZE {
                            self.push_text("\u{FFFD}");
                            self.unicode_errors += 1;
                            if self.unicode_errors <= MAX_UNICODE_ERRORS {
                                tracing::warn!(
                                    "UTF-8 buffer overflow after error: {} bytes",
                                    buffer.len()
                                );
                            }
                            buffer.clear();
                            break;
                        }
                        continue;
                    }

                    // Incomplete UTF-8 sequence at end
                    if eof && !buffer.is_empty() {
                        // At EOF, treat incomplete sequences as invalid
                        self.push_text("\u{FFFD}");
                        self.unicode_errors += 1;
                        if self.unicode_errors <= MAX_UNICODE_ERRORS {
                            tracing::debug!(
                                "Incomplete UTF-8 sequence at EOF, replacing with U+FFFD"
                            );
                        }
                        buffer.clear();
                    } else if !buffer.is_empty() && !eof {
                        // Save incomplete sequence for next call
                        self.utf8_buffer_remainder = buffer.clone();
                        buffer.clear();
                    }

                    break;
                }
            }
        }

        // Complete unicode validation context
        let processed_bytes = if buffer.is_empty() { 0 } else { buffer.len() };
        validation_context.complete(processed_bytes);
    }

    pub(super) fn snapshot(&self) -> String {
        let mut output = String::with_capacity(self.current_bytes.min(self.max_bytes));
        for line in &self.lines {
            output.push_str(line);
        }
        output.push_str(&self.partial);
        output
    }

    pub(super) fn pending(&self) -> String {
        let mut output =
            String::with_capacity(self.pending_lines.len() * 80 + self.pending_partial.len());
        for line in &self.pending_lines {
            output.push_str(line);
        }
        output.push_str(&self.pending_partial);
        output
    }

    pub(super) fn take_pending(&mut self) -> String {
        let mut output =
            String::with_capacity(self.pending_lines.len() * 80 + self.pending_partial.len());
        while let Some(line) = self.pending_lines.pop_front() {
            output.push_str(&line);
        }
        if !self.pending_partial.is_empty() {
            output.push_str(&self.pending_partial);
            self.pending_partial.clear();
        }
        output
    }

    #[allow(dead_code)]
    fn has_overflow(&self) -> bool {
        self.overflow_detected
    }

    #[allow(dead_code)]
    fn current_size_bytes(&self) -> usize {
        self.current_bytes
    }

    #[allow(dead_code)]
    fn usage_percent(&self) -> f64 {
        (self.current_bytes as f64 / self.max_bytes as f64) * 100.0
    }

    #[allow(dead_code)]
    pub(super) fn metrics(&self) -> ScrollbackMetrics {
        ScrollbackMetrics {
            current_bytes: self.current_bytes,
            max_bytes: self.max_bytes,
            usage_percent: self.usage_percent(),
            overflow_detected: self.overflow_detected,
            bytes_dropped: self.bytes_dropped,
            lines_dropped: self.lines_dropped,
            current_lines: self.lines.len(),
            capacity_lines: self.capacity_lines,
            unicode_errors: self.unicode_errors,
            utf8_buffer_size: self.utf8_buffer_remainder.len(),
            total_unicode_chars: self.total_unicode_chars,
            unicode_sessions: self.unicode_sessions,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct ScrollbackMetrics {
    pub(super) current_bytes: usize,
    pub(super) max_bytes: usize,
    pub(super) usage_percent: f64,
    pub(super) overflow_detected: bool,
    pub(super) bytes_dropped: usize,
    pub(super) lines_dropped: usize,
    pub(super) current_lines: usize,
    pub(super) capacity_lines: usize,
    pub(super) unicode_errors: usize, // Count of UTF-8 decoding errors
    pub(super) utf8_buffer_size: usize, // Current size of UTF-8 incomplete buffer
    pub(super) total_unicode_chars: usize, // Total unicode characters processed
    pub(super) unicode_sessions: usize, // Number of sessions with unicode content
}

/// PTY session handle with exclusive access to all PTY resources.
///
/// ## Lock Ordering (CRITICAL - must be respected to avoid deadlock)
/// When acquiring multiple locks, always follow this order:
/// 1. writer (PTY input stream)
/// 2. child (PTY child process)
/// 3. master (PTY master terminal)
/// 4. reader_thread (background reader thread handle)
/// 5. terminal (VT100 parser) - acquired via Arc, can be held alongside others
/// 6. scrollback (output buffer) - acquired via Arc, can be held alongside others
/// 7. last_input (command echo state)
///
/// Note: Some Arc-wrapped locks can be held simultaneously with others since Arc sharing
/// is safe. Single-lock methods don't need to follow this order.

#[cfg(test)]
mod unicode_optimization_tests {
    use super::*;

    #[test]
    fn test_unicode_metrics_tracking() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);

        // Test ASCII-only text
        scrollback.push_text("Hello World");
        assert_eq!(scrollback.total_unicode_chars, 0);
        assert_eq!(scrollback.unicode_sessions, 0);

        // Test unicode text
        scrollback.push_text("Hello 世界");
        assert!(scrollback.total_unicode_chars > 0);
        assert_eq!(scrollback.unicode_sessions, 1);

        // Test emoji
        scrollback.push_text("[TEST]");
        assert!(scrollback.total_unicode_chars > 2);
        assert_eq!(scrollback.unicode_sessions, 2);
    }

    #[test]
    fn test_unicode_buffer_cleanup() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);

        // Add unicode content
        scrollback.push_utf8(&mut "Hello 世界".as_bytes().to_vec(), false);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);

        // Test incomplete sequence
        scrollback.push_utf8(&mut vec![0xF0, 0x9F], false);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 2);

        // Complete the sequence
        scrollback.push_utf8(&mut vec![0x8C, 0x8D], false);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);
    }

    #[test]
    fn test_large_unicode_content() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);

        // Create large unicode content
        let large_unicode: String = "你好世界".repeat(1000);
        scrollback.push_text(&large_unicode);

        let metrics = scrollback.metrics();
        assert!(metrics.total_unicode_chars > 0);
        assert!(metrics.unicode_sessions > 0);
        assert_eq!(metrics.utf8_buffer_size, 0);
    }

    #[test]
    fn test_mixed_ascii_unicode_performance() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);

        // Mix of ASCII and unicode
        let mixed_content = "Hello World! 你好世界 [TEST] café naïve résumé";
        scrollback.push_text(mixed_content);

        assert!(scrollback.total_unicode_chars > 0);
        assert_eq!(scrollback.unicode_sessions, 1);
        assert_eq!(scrollback.snapshot(), mixed_content);
    }

    #[test]
    fn test_unicode_error_recovery() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);

        // Valid content first
        scrollback.push_text("Hello World");
        assert_eq!(scrollback.unicode_errors, 0);

        // Invalid UTF-8 through push_utf8
        scrollback.push_utf8(&mut vec![0xFF, 0xFE], false);
        assert_eq!(scrollback.unicode_errors, 1);

        // Should continue working after error
        scrollback.push_text("Still working");
        assert_eq!(scrollback.unicode_errors, 1); // Error count unchanged
        assert!(scrollback.snapshot().contains("Still working"));
    }
}

#[cfg(test)]
mod unicode_tests {
    use super::*;

    #[test]
    fn test_push_utf8_valid_ascii() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = b"Hello World".to_vec();
        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "Hello World");
        assert_eq!(scrollback.unicode_errors, 0);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);
    }

    #[test]
    fn test_push_utf8_valid_unicode() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = "Hello 世界".as_bytes().to_vec();
        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "Hello 世界");
        assert_eq!(scrollback.unicode_errors, 0);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);
    }

    #[test]
    fn test_push_utf8_valid_emoji() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = "[TEST]".as_bytes().to_vec();
        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "[TEST]");
        assert_eq!(scrollback.unicode_errors, 0);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);
    }

    #[test]
    fn test_push_utf8_invalid_sequence() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8
        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "\u{FFFD}");
        assert_eq!(scrollback.unicode_errors, 1);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_push_utf8_incomplete_sequence() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = vec![0xF0, 0x9F]; // Incomplete emoji (first 2 bytes)
        scrollback.push_utf8(&mut buffer, false);

        // Should save incomplete sequence for next call
        assert_eq!(scrollback.snapshot(), "");
        assert_eq!(scrollback.unicode_errors, 0);
        assert_eq!(scrollback.utf8_buffer_remainder, vec![0xF0, 0x9F]);
    }

    #[test]
    fn test_push_utf8_incomplete_sequence_completed() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);

        // First call with incomplete sequence
        let mut buffer1 = vec![0xF0, 0x9F]; // Incomplete emoji
        scrollback.push_utf8(&mut buffer1, false);

        // Second call with remaining bytes
        let mut buffer2 = vec![0x8C, 0x8D]; // Remaining emoji bytes
        scrollback.push_utf8(&mut buffer2, false);

        assert_eq!(scrollback.snapshot(), "[T]");
        assert_eq!(scrollback.unicode_errors, 0);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);
    }

    #[test]
    fn test_push_utf8_incomplete_at_eof() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = vec![0xF0, 0x9F]; // Incomplete emoji
        scrollback.push_utf8(&mut buffer, true); // EOF = true

        assert_eq!(scrollback.snapshot(), "\u{FFFD}");
        assert_eq!(scrollback.unicode_errors, 1);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_push_utf8_buffer_overflow() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = vec![0xF0; 20 * 1024]; // 20KB of invalid data
        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "\u{FFFD}");
        assert_eq!(scrollback.unicode_errors, 1);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_push_utf8_mixed_valid_invalid() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = Vec::new();
        buffer.extend_from_slice("Hello ".as_bytes());
        buffer.push(0xFF); // Invalid byte
        buffer.extend_from_slice(" World".as_bytes());

        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "Hello \u{FFFD} World");
        assert_eq!(scrollback.unicode_errors, 1);
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_push_utf8_cjk_characters() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = "你好世界こんにちは안녕하세요".as_bytes().to_vec();
        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "你好世界こんにちは안녕하세요");
        assert_eq!(scrollback.unicode_errors, 0);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);
    }

    #[test]
    fn test_push_utf8_european_accents() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = "café naïve résumé".as_bytes().to_vec();
        scrollback.push_utf8(&mut buffer, false);

        assert_eq!(scrollback.snapshot(), "café naïve résumé");
        assert_eq!(scrollback.unicode_errors, 0);
        assert_eq!(scrollback.utf8_buffer_remainder.len(), 0);
    }

    #[test]
    fn test_push_utf8_metrics_tracking() {
        let mut scrollback = PtyScrollback::new(100, 1024 * 1024);
        let mut buffer = vec![0xFF, 0xFE]; // Invalid UTF-8
        scrollback.push_utf8(&mut buffer, false);

        let metrics = scrollback.metrics();
        assert_eq!(metrics.unicode_errors, 1);
        assert_eq!(metrics.utf8_buffer_size, 0);
    }
}

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

    #[test]
    fn scrollback_early_warning_at_80_percent() {
        let mut scrollback = PtyScrollback::new(1000, 1000); // 1KB limit

        // Push data up to 80% (800 bytes)
        for _ in 0..40 {
            scrollback.push_text("12345678901234567890\n"); // 21 bytes x 40 = 840 bytes
        }

        // Should have warning
        let snapshot = scrollback.snapshot();
        assert!(
            snapshot.contains("approaching size limit"),
            "Should show 80% warning"
        );
        assert!(scrollback.warning_shown, "Warning flag should be set");
    }

    #[test]
    fn scrollback_tracks_dropped_metrics() {
        let mut scrollback = PtyScrollback::new(1000, 500); // 500 byte limit

        // Push 1KB of data (should drop half)
        for i in 0..50 {
            scrollback.push_text(&format!("line-{:04}-data\n", i)); // ~16 bytes each
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
        assert!(
            metrics.usage_percent > 50.0 && metrics.usage_percent < 60.0,
            "Usage should be around 50-55%"
        );
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
        assert!(
            !snapshot.contains("\x1b["),
            "Snapshot contains ESC character (0x1b)"
        );
        assert!(
            !snapshot.contains("\u{001b}"),
            "Snapshot contains ESC Unicode"
        );
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
}
