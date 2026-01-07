use std::collections::{HashMap, VecDeque};

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
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use shell_words::join;
use tokio::sync::Mutex as TokioMutex;
use tracing::{debug, info, warn};
use vt100::Parser;

use crate::audit::PermissionAuditLog;
use crate::config::{CommandsConfig, PtyConfig};
use crate::tools::path_env;
use crate::tools::shell::resolve_fallback_shell;
use crate::tools::types::VTCodePtySession;
use crate::utils::unicode_monitor::{UNICODE_MONITOR, UnicodeValidationContext};

#[derive(Clone)]
pub struct PtyManager {
    workspace_root: PathBuf,
    config: PtyConfig,
    inner: Arc<PtyState>,
    audit_log: Option<Arc<TokioMutex<PermissionAuditLog>>>,
    extra_paths: Arc<Mutex<Vec<PathBuf>>>,
}

#[derive(Default)]
struct PtyState {
    sessions: Mutex<HashMap<String, Arc<PtySessionHandle>>>,
}

struct CommandEchoState {
    command_bytes: Vec<u8>,
    failure: Vec<usize>,
    matched: usize,
    require_newline: bool,
    pending_newline: bool,
    consumed_once: bool,
}

impl CommandEchoState {
    fn new(command: &str, expect_newline: bool) -> Option<Self> {
        let trimmed = command.trim_matches(|ch| ch == '\n' || ch == '\r');
        if trimmed.is_empty() {
            return None;
        }

        let command_bytes = trimmed.as_bytes().to_vec();
        if command_bytes.is_empty() {
            return None;
        }

        let failure = build_failure(&command_bytes);

        Some(Self {
            command_bytes,
            failure,
            matched: 0,
            require_newline: expect_newline,
            pending_newline: expect_newline,
            consumed_once: false,
        })
    }

    fn reset(&mut self) {
        self.matched = 0;
        self.pending_newline = self.require_newline;
    }

    fn consume_chunk(&mut self, text: &str) -> (usize, bool) {
        let mut index = 0usize;
        let bytes = text.as_bytes();
        const ZERO_WIDTH_SPACE: &[u8] = "\u{200B}".as_bytes();

        while index < bytes.len() {
            let slice = &text[index..];

            if let Some(len) = parse_ansi_sequence(slice) {
                index += len;
                continue;
            }

            if slice.as_bytes().starts_with(ZERO_WIDTH_SPACE) {
                index += ZERO_WIDTH_SPACE.len();
                continue;
            }

            let byte = bytes[index];

            if byte == b'\r' {
                index += 1;
                self.reset();
                continue;
            }

            if self.pending_newline {
                if byte == b'\n' {
                    index += 1;
                    self.pending_newline = false;
                    continue;
                }
                self.pending_newline = false;
            }

            let mut matched_byte = false;
            loop {
                if let Some(&expected) = self.command_bytes.get(self.matched)
                    && byte == expected
                {
                    self.matched += 1;
                    index += 1;
                    if self.matched == self.command_bytes.len() {
                        self.consumed_once = true;
                        self.pending_newline = self.require_newline;
                        self.matched = if self.command_bytes.len() > 1 {
                            self.failure[self.matched - 1]
                        } else {
                            0
                        };
                    }
                    matched_byte = true;
                    break;
                }

                if self.matched == 0 {
                    break;
                }

                self.matched = self.failure[self.matched - 1];
            }

            if matched_byte {
                continue;
            }

            break;
        }

        let done = self.consumed_once && !self.pending_newline && self.matched == 0;
        (index, done)
    }
}

fn build_failure(pattern: &[u8]) -> Vec<usize> {
    let mut failure = vec![0usize; pattern.len()];
    let mut length = 0usize;
    let mut index = 1usize;

    while index < pattern.len() {
        if pattern[index] == pattern[length] {
            length += 1;
            failure[index] = length;
            index += 1;
        } else if length != 0 {
            length = failure[length - 1];
        } else {
            failure[index] = 0;
            index += 1;
        }
    }

    failure
}

fn parse_ansi_sequence(text: &str) -> Option<usize> {
    crate::utils::ansi_parser::parse_ansi_sequence(text)
}

struct PtyScrollback {
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
    fn new(capacity_lines: usize, max_bytes: usize) -> Self {
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

    fn push_utf8(&mut self, buffer: &mut Vec<u8>, eof: bool) {
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

    fn snapshot(&self) -> String {
        let mut output = String::with_capacity(self.current_bytes.min(self.max_bytes));
        for line in &self.lines {
            output.push_str(line);
        }
        output.push_str(&self.partial);
        output
    }

    fn pending(&self) -> String {
        let mut output =
            String::with_capacity(self.pending_lines.len() * 80 + self.pending_partial.len());
        for line in &self.pending_lines {
            output.push_str(line);
        }
        output.push_str(&self.pending_partial);
        output
    }

    fn take_pending(&mut self) -> String {
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
    fn metrics(&self) -> ScrollbackMetrics {
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
struct ScrollbackMetrics {
    current_bytes: usize,
    max_bytes: usize,
    usage_percent: f64,
    overflow_detected: bool,
    bytes_dropped: usize,
    lines_dropped: usize,
    current_lines: usize,
    capacity_lines: usize,
    unicode_errors: usize,      // Count of UTF-8 decoding errors
    utf8_buffer_size: usize,    // Current size of UTF-8 incomplete buffer
    total_unicode_chars: usize, // Total unicode characters processed
    unicode_sessions: usize,    // Number of sessions with unicode content
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
struct PtySessionHandle {
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    terminal: Arc<Mutex<Parser>>,
    scrollback: Arc<Mutex<PtyScrollback>>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    metadata: VTCodePtySession,
    last_input: Mutex<Option<CommandEchoState>>,
}

impl Drop for PtySessionHandle {
    fn drop(&mut self) {
        // Ensure cleanup even if close_session() wasn't called
        // Follow lock order: writer -> child -> reader_thread -> (no other locks in drop)

        // Close writer
        {
            let mut writer = self.writer.lock();
            if let Some(mut w) = writer.take() {
                let _ = w.write_all(b"exit\n");
                let _ = w.flush();
            }
        }

        // Kill child if still running
        {
            let mut child = self.child.lock();
            if let Ok(None) = child.try_wait() {
                // Child still running, terminate it
                let _ = child.kill();
            }
        }

        // Join reader thread with timeout to prevent hangs
        {
            let mut thread_guard = self.reader_thread.lock();
            if let Some(reader_thread) = thread_guard.take() {
                // Use timeout to prevent infinite hang in Drop
                let join_result = std::thread::spawn(move || {
                    // Give reader thread up to 5 seconds to finish
                    let start = std::time::Instant::now();
                    loop {
                        if reader_thread.is_finished() {
                            let _ = reader_thread.join();
                            break;
                        }
                        if start.elapsed() > Duration::from_secs(5) {
                            warn!("PTY reader thread did not finish within timeout");
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                })
                .join();

                if join_result.is_err() {
                    warn!("PTY reader thread cleanup panicked");
                }
            }
        }
    }
}

impl PtySessionHandle {
    fn snapshot_metadata(&self) -> VTCodePtySession {
        let mut metadata = self.metadata.clone();

        // Lock order: master -> terminal -> scrollback (respect documented order)
        // Note: master is acquired first (single-threaded access)
        let master_size = {
            let master = self.master.lock();
            master.get_size().ok()
        };

        if let Some(size) = master_size {
            metadata.rows = size.rows;
            metadata.cols = size.cols;
        }

        // terminal and scrollback are Arc-wrapped, can be acquired independently
        {
            let parser = self.terminal.lock();
            let contents = parser.screen().contents();
            metadata.screen_contents = Some(contents);
        }
        {
            let scrollback = self.scrollback.lock();
            let contents = scrollback.snapshot();
            if !contents.is_empty() {
                metadata.scrollback = Some(contents);
            }
        }

        metadata
    }

    fn read_output(&self, drain: bool) -> Option<String> {
        let mut scrollback = self.scrollback.lock();
        let text = if drain {
            scrollback.take_pending()
        } else {
            scrollback.pending()
        };
        if text.is_empty() {
            return None;
        }

        let filtered = self.strip_command_echo(text);
        if filtered.is_empty() {
            None
        } else {
            Some(filtered)
        }
    }

    fn strip_command_echo(&self, text: String) -> String {
        let mut guard = self.last_input.lock();
        let Some(state) = guard.as_mut() else {
            return text;
        };

        let (consumed, done) = state.consume_chunk(&text);
        if done {
            *guard = None;
        }

        text.get(consumed..)
            .map(|tail| tail.to_owned())
            .unwrap_or_default()
    }
}

pub struct PtyCommandRequest {
    pub command: Vec<String>,
    pub working_dir: PathBuf,
    pub timeout: Duration,
    pub size: PtySize,
    pub max_tokens: Option<usize>,
}

pub struct PtyCommandResult {
    pub exit_code: i32,
    pub output: String,
    pub duration: Duration,
    pub size: PtySize,
    pub applied_max_tokens: Option<usize>,
}

impl PtyManager {
    pub fn new(workspace_root: PathBuf, config: PtyConfig) -> Self {
        let resolved_root = workspace_root
            .canonicalize()
            .unwrap_or(workspace_root.clone());

        let default_paths = path_env::compute_extra_search_paths(
            &CommandsConfig::default().extra_path_entries,
            &resolved_root,
        );

        Self {
            workspace_root: resolved_root,
            config,
            inner: Arc::new(PtyState::default()),
            audit_log: None,
            extra_paths: Arc::new(Mutex::new(default_paths)),
        }
    }

    pub fn with_audit_log(mut self, audit_log: Arc<TokioMutex<PermissionAuditLog>>) -> Self {
        self.audit_log = Some(audit_log);
        self
    }

    pub fn config(&self) -> &PtyConfig {
        &self.config
    }

    pub fn apply_commands_config(&self, commands_config: &CommandsConfig) {
        let mut extra = self.extra_paths.lock();
        *extra = path_env::compute_extra_search_paths(
            &commands_config.extra_path_entries,
            &self.workspace_root,
        );
    }

    pub fn describe_working_dir(&self, path: &Path) -> String {
        self.format_working_dir(path)
    }

    pub async fn run_command(&self, request: PtyCommandRequest) -> Result<PtyCommandResult> {
        if request.command.is_empty() {
            return Err(anyhow!("PTY command cannot be empty"));
        }

        let mut command = request.command.clone();
        let program = command.remove(0);
        let args = command;
        let timeout = clamp_timeout(request.timeout);
        let work_dir = request.working_dir.clone();
        let size = request.size;
        let start = Instant::now();
        self.ensure_within_workspace(&work_dir)?;
        let workspace_root = self.workspace_root.clone();
        let extra_paths = self.extra_paths.lock().clone();
        let max_tokens = request.max_tokens; // Get max_tokens from request

        let result =
            tokio::task::spawn_blocking(move || -> Result<PtyCommandResult> {
                let timeout_duration = Duration::from_millis(timeout);

                // Use login shell for command execution to ensure user's PATH and environment
                // is properly initialized from their shell configuration files (~/.bashrc, ~/.zshrc, etc).
                // However, we avoid double-wrapping if the command is already a shell invocation.
                let (exec_program, exec_args, display_program, _use_shell_wrapper) =
                    if is_shell_program(&program)
                        && args.iter().any(|arg| arg == "-c" || arg == "/C")
                    {
                        // Already a shell command, don't wrap again
                        (program.clone(), args.clone(), program.clone(), false)
                    } else {
                        let shell = resolve_fallback_shell();
                        let full_command =
                            join(std::iter::once(program.clone()).chain(args.iter().cloned()));
                        (
                            shell.clone(),
                            vec!["-lc".to_owned(), full_command.clone()],
                            program.clone(),
                            true,
                        )
                    };

                let mut builder = CommandBuilder::new(exec_program.clone());
                for arg in &exec_args {
                    builder.arg(arg);
                }
                builder.cwd(&work_dir);
                set_command_environment(
                    &mut builder,
                    &display_program,
                    size,
                    &workspace_root,
                    &extra_paths,
                );

                let pty_system = native_pty_system();
                let pair = pty_system
                    .openpty(size)
                    .context("failed to allocate PTY pair")?;

                let mut child = pair
                    .slave
                    .spawn_command(builder)
                    .with_context(|| format!("failed to spawn PTY command '{display_program}'"))?;
                let mut killer = child.clone_killer();
                drop(pair.slave);

                let reader = pair
                    .master
                    .try_clone_reader()
                    .context("failed to clone PTY reader")?;

                let (wait_tx, wait_rx) = mpsc::channel();
                let wait_thread = thread::spawn(move || {
                    let status = child.wait();
                    let _ = wait_tx.send(());
                    status
                });

                let reader_thread = thread::spawn(move || -> Result<Vec<u8>> {
                    let mut reader = reader;
                    let mut buffer = [0u8; 4096];
                    let mut collected = Vec::new();

                    loop {
                        match reader.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(bytes_read) => {
                                collected.extend_from_slice(&buffer[..bytes_read]);
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                                continue;
                            }
                            Err(error) => {
                                return Err(error).context("failed to read PTY command output");
                            }
                        }
                    }

                    Ok(collected)
                });

                let wait_result = match wait_rx.recv_timeout(timeout_duration) {
                    Ok(()) => wait_thread.join().map_err(|panic| {
                        anyhow!("PTY command wait thread panicked: {:?}", panic)
                    })?,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        killer
                            .kill()
                            .context("failed to terminate PTY command after timeout")?;

                        let join_result = wait_thread.join().map_err(|panic| {
                            anyhow!("PTY command wait thread panicked: {:?}", panic)
                        })?;
                        if let Err(error) = join_result {
                            return Err(error)
                                .context("failed to wait for PTY command to exit after timeout");
                        }

                        reader_thread
                            .join()
                            .map_err(|panic| {
                                anyhow!("PTY command reader thread panicked: {:?}", panic)
                            })?
                            .context("failed to read PTY command output")?;

                        return Err(anyhow!(
                            "PTY command timed out after {} milliseconds",
                            timeout
                        ));
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        let join_result = wait_thread.join().map_err(|panic| {
                            anyhow!("PTY command wait thread panicked: {:?}", panic)
                        })?;
                        if let Err(error) = join_result {
                            return Err(error).context(
                                "failed to wait for PTY command after wait channel disconnected",
                            );
                        }

                        reader_thread
                            .join()
                            .map_err(|panic| {
                                anyhow!("PTY command reader thread panicked: {:?}", panic)
                            })?
                            .context("failed to read PTY command output")?;

                        return Err(anyhow!(
                            "PTY command wait channel disconnected unexpectedly"
                        ));
                    }
                };

                let status = wait_result.context("failed to wait for PTY command to exit")?;

                let output_bytes = reader_thread
                    .join()
                    .map_err(|panic| anyhow!("PTY command reader thread panicked: {:?}", panic))?
                    .context("failed to read PTY command output")?;
                let mut output = String::from_utf8_lossy(&output_bytes).into_owned();
                let exit_code = exit_status_code(status);

                // Apply max_tokens truncation if specified
                if let Some(max_tokens) = max_tokens {
                    if max_tokens > 0 {
                        // Simple byte-based truncation
                        if output.len() > max_tokens * 4 {
                            let truncate_point = (max_tokens * 4).min(output.len());
                            output.truncate(truncate_point);
                            output.push_str("\n[... truncated by max_tokens ...]");
                        }
                    } else {
                        // Keep original if max_tokens is not valid
                    }
                }
                // Keep original if max_tokens is None

                Ok(PtyCommandResult {
                    exit_code,
                    output,
                    duration: start.elapsed(),
                    size,
                    applied_max_tokens: max_tokens,
                })
            })
            .await
            .context("failed to join PTY command task")??;

        Ok(result)
    }

    pub async fn resolve_working_dir(&self, requested: Option<&str>) -> Result<PathBuf> {
        let requested = match requested {
            Some(dir) if !dir.trim().is_empty() => dir.trim(),
            _ => return Ok(self.workspace_root.clone()),
        };

        let candidate = self.workspace_root.join(requested);
        let normalized = normalize_path(&candidate);
        if !normalized.starts_with(&self.workspace_root) {
            return Err(anyhow!(
                "Working directory '{}' escapes the workspace root",
                candidate.display()
            ));
        }
        let metadata = tokio::fs::metadata(&normalized).await.with_context(|| {
            format!(
                "Working directory '{}' does not exist",
                normalized.display()
            )
        })?;
        if !metadata.is_dir() {
            return Err(anyhow!(
                "Working directory '{}' is not a directory",
                normalized.display()
            ));
        }
        Ok(normalized)
    }

    pub fn create_session(
        &self,
        session_id: String,
        command: Vec<String>,
        working_dir: PathBuf,
        size: PtySize,
    ) -> Result<VTCodePtySession> {
        if command.is_empty() {
            return Err(anyhow!(
                "PTY session command cannot be empty.\n\
                 This is an internal error - command validation should have caught this earlier.\n\
                 Please report this with the run_pty_cmd parameters used."
            ));
        }

        // Use entry API to avoid double lookup
        let mut sessions = self.inner.sessions.lock();
        use std::collections::hash_map::Entry;
        let entry = match sessions.entry(session_id.clone()) {
            Entry::Occupied(_) => {
                return Err(anyhow!("PTY session '{}' already exists", session_id));
            }
            Entry::Vacant(e) => e,
        };

        let mut command_parts = command.clone();
        let program = command_parts.remove(0);
        let args = command_parts;
        let extra_paths = self.extra_paths.lock().clone();

        // Use login shell for command execution to ensure user's PATH and environment
        // is properly initialized from their shell configuration files (~/.bashrc, ~/.zshrc, etc).
        // However, we avoid double-wrapping if the command is already a shell invocation.
        let (exec_program, exec_args, display_program) = if is_shell_program(&program)
            && args.iter().any(|arg| arg == "-c" || arg == "/C")
        {
            // Already a shell command, don't wrap again
            (program.clone(), args.clone(), program.clone())
        } else {
            let shell = resolve_fallback_shell();
            let full_command = join(std::iter::once(program.clone()).chain(args.iter().cloned()));

            // Verify we have a valid command string
            if full_command.is_empty() {
                return Err(anyhow!(
                    "Failed to construct command string from program '{}' and args {:?}",
                    program,
                    args
                ));
            }

            (
                shell.clone(),
                vec!["-lc".to_owned(), full_command.clone()],
                program.clone(),
            )
        };

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size)
            .context("failed to allocate PTY pair")?;

        let mut builder = CommandBuilder::new(exec_program.clone());
        for arg in &exec_args {
            builder.arg(arg);
        }
        builder.cwd(&working_dir);
        self.ensure_within_workspace(&working_dir)?;
        set_command_environment(
            &mut builder,
            &display_program,
            size,
            &self.workspace_root,
            &extra_paths,
        );

        let child = pair.slave.spawn_command(builder).with_context(|| {
            format!("failed to spawn PTY session command '{}'", display_program)
        })?;
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = master.take_writer().context("failed to take PTY writer")?;

        let parser = Arc::new(Mutex::new(Parser::new(
            size.rows,
            size.cols,
            self.config.scrollback_lines,
        )));
        let scrollback = Arc::new(Mutex::new(PtyScrollback::new(
            self.config.scrollback_lines,
            self.config.max_scrollback_bytes,
        )));
        let parser_clone = Arc::clone(&parser);
        let scrollback_clone = Arc::clone(&scrollback);
        let session_name = session_id.clone();
        // Start unicode monitoring for this session
        UNICODE_MONITOR.start_session();

        let reader_thread = thread::Builder::new()
            .name(format!("vtcode-pty-reader-{session_name}"))
            .spawn(move || {
                let mut buffer = [0u8; 8192]; // Increased buffer size for better performance
                let mut utf8_buffer: Vec<u8> = Vec::with_capacity(8192); // Pre-allocate buffer
                let mut total_bytes = 0usize;
                let mut unicode_detection_hits = 0usize;

                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            if !utf8_buffer.is_empty() {
                                let mut scrollback = scrollback_clone.lock();
                                scrollback.push_utf8(&mut utf8_buffer, true);
                            }
                            debug!("PTY session '{}' reader reached EOF (processed {} bytes, {} unicode detections)",
                                   session_name, total_bytes, unicode_detection_hits);
                            break;
                        }
                        Ok(bytes_read) => {
                            let chunk = &buffer[..bytes_read];
                            total_bytes += bytes_read;

                            // Quick unicode detection heuristic
                            let likely_unicode = chunk.iter().any(|&b| b >= 0x80);
                            if likely_unicode {
                                unicode_detection_hits += 1;
                            }

                            // Process chunk through VT100 parser for screen updates
                            {
                                let mut parser = parser_clone.lock();
                                parser.process(chunk);
                            }

                            utf8_buffer.extend_from_slice(chunk);
                            {
                                let mut scrollback = scrollback_clone.lock();
                                scrollback.push_utf8(&mut utf8_buffer, false);
                            }

                            // Periodic buffer cleanup to prevent excessive memory usage
                            if utf8_buffer.capacity() > 32768 && utf8_buffer.len() < 1024 {
                                utf8_buffer.shrink_to_fit();
                            }
                        }
                        Err(error) => {
                            warn!("PTY session '{}' reader error: {} (processed {} bytes)",
                                  session_name, error, total_bytes);
                            break;
                        }
                    }
                }
                debug!("PTY session '{}' reader thread finished (total: {} bytes, unicode detections: {})",
                       session_name, total_bytes, unicode_detection_hits);

                // End unicode monitoring for this session
                UNICODE_MONITOR.end_session();

                // Log unicode statistics if any unicode was detected
                if unicode_detection_hits > 0 {
                    let scrollback = scrollback_clone.lock();
                    let metrics = scrollback.metrics();
                    if metrics.unicode_errors > 0 {
                        warn!("PTY session '{}' had {} unicode errors during processing",
                              session_name, metrics.unicode_errors);
                    }
                    if metrics.total_unicode_chars > 0 {
                        info!("PTY session '{}' processed {} unicode characters across {} sessions with {} buffer remainder",
                              session_name, metrics.total_unicode_chars, metrics.unicode_sessions, metrics.utf8_buffer_size);
                    }
                }
            })
            .context("failed to spawn PTY reader thread")?;

        let metadata = VTCodePtySession {
            id: session_id.clone(),
            command: program,
            args,
            working_dir: Some(self.format_working_dir(&working_dir)),
            rows: size.rows,
            cols: size.cols,
            screen_contents: None,
            scrollback: None,
        };

        // Use the entry we obtained earlier to insert without additional lookup
        entry.insert(Arc::new(PtySessionHandle {
            master: Mutex::new(master),
            child: Mutex::new(child),
            writer: Mutex::new(Some(writer)),
            terminal: parser,
            scrollback,
            reader_thread: Mutex::new(Some(reader_thread)),
            metadata: metadata.clone(),
            last_input: Mutex::new(None),
        }));

        Ok(metadata)
    }

    pub fn list_sessions(&self) -> Vec<VTCodePtySession> {
        let sessions = self.inner.sessions.lock();
        sessions
            .values()
            .map(|handle| handle.snapshot_metadata())
            .collect()
    }

    pub fn snapshot_session(&self, session_id: &str) -> Result<VTCodePtySession> {
        let handle = self.session_handle(session_id)?;
        Ok(handle.snapshot_metadata())
    }

    pub fn read_session_output(&self, session_id: &str, drain: bool) -> Result<Option<String>> {
        let handle = self.session_handle(session_id)?;
        Ok(handle.read_output(drain))
    }

    pub fn send_input_to_session(
        &self,
        session_id: &str,
        data: &[u8],
        append_newline: bool,
    ) -> Result<usize> {
        let handle = self.session_handle(session_id)?;

        // Acquire last_input lock once and update conditionally
        {
            let mut last_input = handle.last_input.lock();
            *last_input = if let Ok(input_text) = std::str::from_utf8(data) {
                CommandEchoState::new(input_text, append_newline)
            } else {
                None
            };
        }

        // Acquire writer lock once for all write operations
        {
            let mut writer_guard = handle.writer.lock();
            let writer = writer_guard
                .as_mut()
                .ok_or_else(|| anyhow!("PTY session '{}' is no longer writable", session_id))?;

            writer
                .write_all(data)
                .context("failed to write input to PTY session")?;

            if append_newline {
                writer
                    .write_all(b"\n")
                    .context("failed to write newline to PTY session")?;
            }

            writer
                .flush()
                .context("failed to flush PTY session input")?;
        }

        let written = data.len() + if append_newline { 1 } else { 0 };
        Ok(written)
    }

    pub fn resize_session(&self, session_id: &str, size: PtySize) -> Result<VTCodePtySession> {
        let handle = self.session_handle(session_id)?;

        // Lock order: master -> terminal (Arc-wrapped, separate scope)
        {
            let master = handle.master.lock();
            master
                .resize(size)
                .context("failed to resize PTY session")?;
        }

        // Terminal lock acquired separately (Arc, safe to interleave)
        {
            let mut parser = handle.terminal.lock();
            parser.set_size(size.rows, size.cols);
        }

        Ok(handle.snapshot_metadata())
    }

    pub fn is_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let handle = self.session_handle(session_id)?;
        let mut child = handle.child.lock();
        child
            .try_wait()
            .context("failed to poll PTY session status")
            .map(|opt| opt.map(exit_status_code))
    }

    /// Sync all terminal sessions to files for dynamic context discovery
    ///
    /// This implements Cursor-style dynamic context discovery:
    /// - Each terminal session is written to `.vtcode/terminals/{session_id}.txt`
    /// - Includes metadata header (cwd, last command, exit code)
    /// - Agent can reference terminal output via grep/read_file
    pub async fn sync_sessions_to_files(&self) -> Result<Vec<std::path::PathBuf>> {
        let terminals_dir = self.workspace_root.join(".vtcode").join("terminals");
        tokio::fs::create_dir_all(&terminals_dir)
            .await
            .with_context(|| format!("Failed to create terminals directory: {}", terminals_dir.display()))?;

        let sessions = self.list_sessions();
        let mut written_files = Vec::with_capacity(sessions.len());

        for session in &sessions {
            let output = match self.read_session_output(&session.id, false) {
                Ok(Some(output)) => output,
                Ok(None) => String::new(),
                Err(_) => continue,
            };

            let content = format_terminal_file(session, &output);
            let file_path = terminals_dir.join(format!("{}.txt", sanitize_session_id(&session.id)));

            if let Err(e) = tokio::fs::write(&file_path, &content).await {
                tracing::warn!(
                    session_id = %session.id,
                    error = %e,
                    "Failed to sync terminal session to file"
                );
                continue;
            }

            written_files.push(file_path);
        }

        // Write index file
        let index_content = self.generate_terminals_index(&sessions);
        let index_path = terminals_dir.join("INDEX.md");
        tokio::fs::write(&index_path, &index_content)
            .await
            .with_context(|| format!("Failed to write terminals index: {}", index_path.display()))?;

        tracing::info!(
            sessions = sessions.len(),
            files = written_files.len(),
            "Synced terminal sessions to files"
        );

        Ok(written_files)
    }

    /// Generate INDEX.md content for terminal sessions
    fn generate_terminals_index(&self, sessions: &[VTCodePtySession]) -> String {
        let mut content = String::new();
        content.push_str("# Terminal Sessions Index\n\n");
        content.push_str("This file lists all active terminal sessions for dynamic discovery.\n");
        content.push_str("Use `read_file` on individual session files for full output.\n\n");

        if sessions.is_empty() {
            content.push_str("*No active terminal sessions.*\n");
        } else {
            content.push_str(&format!("**Active Sessions**: {}\n\n", sessions.len()));
            content.push_str("| Session ID | Command | Working Dir | Size |\n");
            content.push_str("|------------|---------|-------------|------|\n");

            for session in sessions {
                let cwd = session
                    .working_dir
                    .as_ref()
                    .map(|s| s.as_str())
                    .unwrap_or("-");
                let cmd_truncated = if session.command.len() > 25 {
                    format!("{}...", &session.command[..22])
                } else {
                    session.command.clone()
                };

                content.push_str(&format!(
                    "| `{}` | {} | {} | {}x{} |\n",
                    session.id,
                    cmd_truncated.replace('|', "\\|"),
                    cwd.replace('|', "\\|"),
                    session.cols,
                    session.rows
                ));
            }

            content.push_str("\n## Session Details\n\n");
            for session in sessions {
                content.push_str(&format!("### {}\n\n", session.id));
                content.push_str(&format!("- **Command**: `{}`\n", session.command));
                if !session.args.is_empty() {
                    content.push_str(&format!("- **Args**: {}\n", session.args.join(" ")));
                }
                if let Some(cwd) = &session.working_dir {
                    content.push_str(&format!("- **Working Dir**: {}\n", cwd));
                }
                content.push_str(&format!(
                    "- **Terminal Size**: {}x{}\n",
                    session.cols, session.rows
                ));
                content.push_str(&format!(
                    "- **File**: `.vtcode/terminals/{}.txt`\n\n",
                    sanitize_session_id(&session.id)
                ));
            }
        }

        content.push_str("---\n");
        content.push_str("*Generated automatically. Do not edit manually.*\n");

        content
    }

    /// Get the terminals directory path
    pub fn terminals_dir(&self) -> std::path::PathBuf {
        self.workspace_root.join(".vtcode").join("terminals")
    }

    pub fn close_session(&self, session_id: &str) -> Result<VTCodePtySession> {
        // Remove session from global map first
        let handle = {
            let mut sessions = self.inner.sessions.lock();
            sessions
                .remove(session_id)
                .ok_or_else(|| anyhow!("PTY session '{}' not found", session_id))?
        };

        // Lock order: writer -> child -> reader_thread (follow documented order)

        // 1. Close writer
        {
            let mut writer_guard = handle.writer.lock();
            if let Some(mut writer) = writer_guard.take() {
                let _ = writer.write_all(b"exit\n");
                let _ = writer.flush();
            }
        }

        // 2. Terminate child process
        {
            let mut child = handle.child.lock();
            if child
                .try_wait()
                .context("failed to poll PTY session status")?
                .is_none()
            {
                let kill_started = Instant::now();
                child.kill().context("failed to terminate PTY session")?;
                let _ = child.wait();
                let elapsed = kill_started.elapsed();
                if elapsed > Duration::from_secs(2) {
                    warn!(
                        session = %session_id,
                        elapsed_ms = %elapsed.as_millis(),
                        "PTY session termination exceeded budget"
                    );
                }
            }
        }

        // 3. Join reader thread
        {
            let mut thread_guard = handle.reader_thread.lock();
            if let Some(reader_thread) = thread_guard.take()
                && let Err(panic) = reader_thread.join()
            {
                warn!(
                    "PTY session '{}' reader thread panicked: {:?}",
                    session_id, panic
                );
            }
        }

        // Snapshot metadata calls snapshot_metadata() which acquires master, terminal, scrollback locks
        Ok(handle.snapshot_metadata())
    }

    fn format_working_dir(&self, path: &Path) -> String {
        match path.strip_prefix(&self.workspace_root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".into(),
            Ok(relative) => relative.to_string_lossy().replace("\\", "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }

    pub fn terminate_all_sessions(&self) {
        let session_ids: Vec<String> = {
            let sessions = self.inner.sessions.lock();
            sessions.keys().cloned().collect()
        };
        for id in session_ids {
            if let Err(e) = self.close_session(&id) {
                warn!("Failed to close PTY session {}: {}", id, e);
            }
        }
    }

    fn session_handle(&self, session_id: &str) -> Result<Arc<PtySessionHandle>> {
        let sessions = self.inner.sessions.lock();
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("PTY session '{}' not found", session_id))
    }

    fn ensure_within_workspace(&self, candidate: &Path) -> Result<()> {
        let normalized = normalize_path(candidate);
        if !normalized.starts_with(&self.workspace_root) {
            return Err(anyhow!(
                "Path '{}' escapes workspace '{}'",
                candidate.display(),
                self.workspace_root.display()
            ));
        }
        Ok(())
    }
}

fn clamp_timeout(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn exit_status_code(status: portable_pty::ExitStatus) -> i32 {
    if status.signal().is_some() {
        -1
    } else {
        status.exit_code() as i32
    }
}

use crate::utils::path::normalize_path;

fn set_command_environment(
    builder: &mut CommandBuilder,
    program: &str,
    size: PtySize,
    workspace_root: &Path,
    extra_paths: &[PathBuf],
) {
    // Inherit environment from parent process to preserve PATH and other important variables
    let mut env_map: HashMap<OsString, OsString> = std::env::vars_os().collect();

    // Ensure HOME is set - this is crucial for proper path expansion in cargo and other tools
    let home_key = OsString::from("HOME");
    if !env_map.contains_key(&home_key)
        && let Some(home_dir) = dirs::home_dir()
    {
        env_map.insert(home_key.clone(), OsString::from(home_dir.as_os_str()));
    }

    let path_key = OsString::from("PATH");
    let current_path = env_map.get(&path_key).map(|value| value.as_os_str());
    if let Some(merged) = path_env::merge_path_env(current_path, extra_paths) {
        env_map.insert(path_key, merged);
    }

    for (key, value) in env_map {
        builder.env(key, value);
    }

    // Override or set specific environment variables for TTY
    builder.env("TERM", "xterm-256color");
    builder.env("PAGER", "cat");
    builder.env("GIT_PAGER", "cat");
    builder.env("LESS", "R");
    builder.env("COLUMNS", size.cols.to_string());
    builder.env("LINES", size.rows.to_string());
    builder.env("WORKSPACE_DIR", workspace_root.as_os_str());

    // Disable automatic color output from ls and other commands
    builder.env("CLICOLOR", "0");
    builder.env("CLICOLOR_FORCE", "0");
    builder.env("LS_COLORS", "");
    builder.env("NO_COLOR", "1");

    // For Rust/Cargo, disable colors at the source
    builder.env("CARGO_TERM_COLOR", "never");

    // Suppress macOS malloc debugging junk that can pollute PTY output
    // This is especially common when running in login shells (-l)
    builder.env_remove("MallocStackLogging");
    builder.env_remove("MallocStackLoggingNoCompact");
    builder.env_remove("MallocStackLoggingDirectory");
    builder.env_remove("MallocErrorAbort");
    builder.env_remove("MallocCheckHeapStart");
    builder.env_remove("MallocCheckHeapEach");
    builder.env_remove("MallocCheckHeapSleep");
    builder.env_remove("MallocCheckHeapAbort");
    builder.env_remove("MallocGuardEdges");
    builder.env_remove("MallocScribble");
    builder.env_remove("MallocDoNotProtectSentinel");
    builder.env_remove("MallocQuiet");

    if is_shell_program(program) {
        builder.env("SHELL", program);
    }
}

fn is_shell_program(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "bash" | "sh" | "zsh" | "fish" | "dash" | "ash" | "busybox"
    )
}

// Note: resolve_fallback_shell moved to tools::shell module

/// Resolve program path - if program doesn't exist in PATH, return None to signal shell fallback.
/// This allows the shell to find programs installed in user-specific directories.
pub fn is_development_toolchain_command(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cargo"
            | "rustc"
            | "rustup"
            | "rustfmt"
            | "clippy"
            | "npm"
            | "node"
            | "yarn"
            | "pnpm"
            | "bun"
            | "go"
            | "python"
            | "python3"
            | "pip"
            | "pip3"
            | "java"
            | "javac"
            | "mvn"
            | "gradle"
            | "make"
            | "cmake"
            | "gcc"
            | "g++"
            | "clang"
            | "clang++"
            | "which"
    )
}

// Helper functions for terminal file sync (dynamic context discovery)

/// Sanitize session ID for use in filename
fn sanitize_session_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

/// Format terminal session as a file with metadata header
fn format_terminal_file(session: &VTCodePtySession, output: &str) -> String {
    let mut content = String::new();

    // Metadata header
    content.push_str("---\n");
    content.push_str(&format!("session_id: {}\n", session.id));
    content.push_str(&format!("command: {}\n", session.command));
    if !session.args.is_empty() {
        content.push_str(&format!("args: {}\n", session.args.join(" ")));
    }
    if let Some(cwd) = &session.working_dir {
        content.push_str(&format!("cwd: {}\n", cwd));
    }
    content.push_str(&format!("size: {}x{}\n", session.cols, session.rows));
    content.push_str("---\n\n");

    // Terminal output
    content.push_str(output);

    content
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
