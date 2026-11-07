//! Pipe-session line monitor: substring filter with bounded match buffering.
//!
//! Taps decoded pipe output line by line, keeps only lines matching a
//! subscriber filter, and exposes a bounded digest through peek/drain
//! semantics that mirror [`PipeOutputBuffer`]. Tool responses drain windows
//! per read; the runloop drains leftovers for idle wake. Every buffer here
//! is capped, including the unterminated-line remainder.

use super::*;
use crate::types::CompactStr;
use vtcode_commons::sanitizer::{redact_secrets, sanitize_terminal_text};

/// Default cap on buffered matched lines per drain window.
pub(crate) const MONITOR_DEFAULT_MAX_LINES: usize = 50;
/// Default cap on buffered matched bytes per drain window.
pub(crate) const MONITOR_DEFAULT_MAX_BYTES: usize = 4_096;
/// Hard ceiling for `max_lines`; keeps one subscription from owning memory.
const MONITOR_MAX_LINES_HARD_CAP: usize = 500;
/// Hard ceiling for `max_bytes`; keeps one digest inside a tool result.
const MONITOR_MAX_BYTES_HARD_CAP: usize = 16_384;
/// Floor for `max_bytes` so a digest can always hold one short line.
const MONITOR_MIN_BYTES: usize = 256;
/// Longest accepted filter in characters; filters are keywords, not documents.
const MONITOR_FILTER_MAX_CHARS: usize = 256;
/// Longest retained line in characters; matches the completion-field budget.
const MONITOR_LINE_MAX_CHARS: usize = 512;
/// Most sessions drained per idle-wake tick; leftovers stay buffered.
pub(crate) const MONITOR_WAKE_MAX_SESSIONS: usize = 8;
/// Longest unterminated tail kept verbatim; past this the head force-flushes
/// as a line so newline-less output cannot grow memory without bound.
const MONITOR_REMAINDER_MAX_BYTES: usize = 64 * 1024;

/// Validated subscription: which lines to keep and how much to buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MonitorSpec {
    filter: CompactStr,
    max_lines: usize,
    max_bytes: usize,
}

impl MonitorSpec {
    /// Parse untrusted subscription input into a validated spec.
    ///
    /// The filter must be non-empty after trimming and fit
    /// `MONITOR_FILTER_MAX_CHARS`. Caps fall back to defaults when omitted
    /// and clamp into `[1, HARD_CAP]` / `[MIN, HARD_CAP]`.
    ///
    /// # Errors
    ///
    /// Returns an error when the filter is empty or too long.
    pub(crate) fn new(filter: &str, max_lines: Option<usize>, max_bytes: Option<usize>) -> Result<Self> {
        let trimmed = filter.trim();
        if trimmed.is_empty() {
            bail!("monitor filter must not be empty");
        }
        if trimmed.chars().count() > MONITOR_FILTER_MAX_CHARS {
            bail!("monitor filter exceeds {MONITOR_FILTER_MAX_CHARS} characters");
        }
        Ok(Self {
            filter: CompactStr::new(trimmed),
            max_lines: max_lines
                .unwrap_or(MONITOR_DEFAULT_MAX_LINES)
                .clamp(1, MONITOR_MAX_LINES_HARD_CAP),
            max_bytes: max_bytes
                .unwrap_or(MONITOR_DEFAULT_MAX_BYTES)
                .clamp(MONITOR_MIN_BYTES, MONITOR_MAX_BYTES_HARD_CAP),
        })
    }

    /// Case-sensitive substring match against one decoded line.
    #[must_use]
    pub(crate) fn matches(&self, line: &str) -> bool {
        line.contains(self.filter.as_str())
    }
}

/// Bounded view of one drain window: matched lines plus drop accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MonitorDigest {
    /// Redacted, terminal-sanitized matched lines in arrival order.
    pub(crate) lines: Vec<String>,
    /// Matched lines dropped by the window caps.
    pub(crate) dropped_lines: u64,
    /// Bytes of dropped matched lines.
    pub(crate) dropped_bytes: u64,
    /// Matched lines seen in this window, accepted plus dropped.
    pub(crate) total_seen: u64,
}

impl MonitorDigest {
    /// Render lines with a drop-count tail so truncation stays visible.
    #[must_use]
    pub(crate) fn render(&self) -> String {
        let mut rendered = self.lines.join("\n");
        if self.dropped_lines > 0 {
            if !rendered.is_empty() {
                rendered.push('\n');
            }
            rendered.push_str(&format!(
                "[monitor digest truncated: {} lines, {} bytes dropped]",
                self.dropped_lines, self.dropped_bytes
            ));
        }
        rendered
    }
}

/// Mutable per-session monitor state: spec, pending window, line remainder.
#[derive(Debug, Clone)]
pub(crate) struct MonitorState {
    spec: MonitorSpec,
    pending: Vec<String>,
    pending_bytes: usize,
    remainder: String,
    dropped_lines: u64,
    dropped_bytes: u64,
    total_seen: u64,
}

impl MonitorState {
    pub(crate) fn new(spec: MonitorSpec) -> Self {
        Self {
            spec,
            pending: Vec::new(),
            pending_bytes: 0,
            remainder: String::new(),
            dropped_lines: 0,
            dropped_bytes: 0,
            total_seen: 0,
        }
    }

    /// Feed decoded text; splits on line breaks, so boundaries stay char-safe.
    pub(crate) fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.remainder.push_str(text);
        self.split_complete_lines();
        self.force_flush_oversize_remainder();
    }

    /// Split off every newline-terminated line, keeping the tail.
    ///
    /// Both `\n` and `\r` end a line so carriage-return progress output
    /// cannot pile up behind one giant remainder.
    fn split_complete_lines(&mut self) {
        if !self.remainder.contains('\n') && !self.remainder.contains('\r') {
            return;
        }
        let combined = std::mem::take(&mut self.remainder);
        let mut lines: Vec<&str> = combined.split(['\n', '\r']).collect();
        // `split` always yields a trailing segment after the last break; it
        // is empty exactly when the input ends with a break.
        let trailing = lines.pop().unwrap_or_default();
        if !combined.ends_with(['\n', '\r']) {
            self.remainder.push_str(trailing);
        }
        for line in lines {
            self.push_line(line);
        }
    }

    /// Force-flush an overgrown tail so memory stays bounded.
    ///
    /// Keeps a filter-length overlap so a match spanning the cut is still
    /// found exactly once, in the next line. The overlap is excluded from
    /// the flushed head, so fully overlapped matches never double-report.
    fn force_flush_oversize_remainder(&mut self) {
        if self.remainder.len() <= MONITOR_REMAINDER_MAX_BYTES {
            return;
        }
        let overlap_chars = self.spec.filter.as_str().chars().count().max(1);
        let total_chars = self.remainder.chars().count();
        let cut_chars = total_chars.saturating_sub(overlap_chars);
        let cut_byte = self
            .remainder
            .char_indices()
            .nth(cut_chars)
            .map(|(index, _)| index)
            .unwrap_or(self.remainder.len());
        let tail = self.remainder.split_off(cut_byte);
        let head = std::mem::replace(&mut self.remainder, tail);
        self.push_line(&head);
    }

    /// Flush a final unterminated line at end of stream.
    pub(crate) fn flush(&mut self) {
        if self.remainder.is_empty() {
            return;
        }
        let last = std::mem::take(&mut self.remainder);
        self.push_line(&last);
    }

    fn push_line(&mut self, line: &str) {
        if !self.spec.matches(line) {
            return;
        }
        self.total_seen = self.total_seen.saturating_add(1);
        let redacted = redact_secrets(line.to_owned());
        let sanitized = sanitize_terminal_text(&redacted, MONITOR_LINE_MAX_CHARS);
        let line_bytes = sanitized.len();
        if self.pending.len() >= self.spec.max_lines
            || self.pending_bytes.saturating_add(line_bytes) > self.spec.max_bytes
        {
            self.dropped_lines = self.dropped_lines.saturating_add(1);
            self.dropped_bytes = self.dropped_bytes.saturating_add(line_bytes as u64);
            return;
        }
        self.pending_bytes = self.pending_bytes.saturating_add(line_bytes);
        self.pending.push(sanitized);
    }

    fn snapshot(&self) -> MonitorDigest {
        MonitorDigest {
            lines: self.pending.clone(),
            dropped_lines: self.dropped_lines,
            dropped_bytes: self.dropped_bytes,
            total_seen: self.total_seen,
        }
    }

    /// Non-consuming view; `None` when no matched line arrived this window.
    pub(crate) fn peek_digest(&self) -> Option<MonitorDigest> {
        if self.total_seen == 0 {
            None
        } else {
            Some(self.snapshot())
        }
    }

    /// Consuming view; resets the window counts with the lines.
    pub(crate) fn drain_digest(&mut self) -> Option<MonitorDigest> {
        if self.total_seen == 0 {
            return None;
        }
        let digest = self.snapshot();
        self.pending.clear();
        self.pending_bytes = 0;
        self.dropped_lines = 0;
        self.dropped_bytes = 0;
        self.total_seen = 0;
        Some(digest)
    }
}

/// One drained monitor window for idle wake, rendered by the runloop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorWakeNote {
    /// Session that produced the matches.
    pub session_id: String,
    /// Program the session runs, for note headers.
    pub command: String,
    /// Rendered matched lines with any drop-count tail.
    pub text: String,
    /// Matched lines dropped by the window caps.
    pub dropped_lines: u64,
    /// Bytes of dropped matched lines.
    pub dropped_bytes: u64,
    /// Matched lines seen in this window, accepted plus dropped.
    pub total_seen: u64,
}

impl PipeSessionManager {
    /// Attach or replace the line filter; resets any buffered window.
    pub(crate) async fn set_monitor_filter(&self, session_id: &str, spec: MonitorSpec) -> Result<()> {
        let record = self.session_record(session_id).await?;
        *record.monitor.lock() = Some(MonitorState::new(spec));
        Ok(())
    }

    /// Peek or drain the bounded match digest; `None` when nothing matched.
    pub(crate) async fn read_monitor_digest(&self, session_id: &str, drain: bool) -> Result<Option<MonitorDigest>> {
        let record = self.session_record(session_id).await?;
        let mut guard = record.monitor.lock();
        let Some(state) = guard.as_mut() else {
            return Ok(None);
        };
        if drain {
            Ok(state.drain_digest())
        } else {
            Ok(state.peek_digest())
        }
    }
}
