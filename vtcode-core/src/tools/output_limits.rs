//! Centralized tool-output limits.
//!
//! §18.4.3 of *The Hitchhiker's Guide to Agentic AI* prescribes a deterministic
//! truncation pipeline. Historically, VT Code scattered the relevant constants
//! across `response_content.rs`, `error_handling.rs`, `summarizers/`, and
//! `tool_pipeline/`. This module collects them into one struct so the runloop,
//! summarizers, and the new [`crate::tools::output_pipeline`] can all read the
//! same knobs.
//!
//! Defaults track the constants the runloop used at the time this module was
//! introduced; nothing here should be made smaller without checking the existing
//! tests.

use serde::{Deserialize, Serialize};

/// All truncation / summarization knobs the runloop cares about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutputLimits {
    /// Maximum characters fed into the summarizer before it bails out.
    pub summary_max_input_chars: usize,
    /// Maximum tokens the summarizer is allowed to emit.
    pub summary_max_output_tokens: usize,
    /// Bytes to keep from the head of a `read` / `cat` result.
    pub read_head_bytes: usize,
    /// Bytes to keep from the tail of a `read` / `cat` result.
    pub read_tail_bytes: usize,
    /// Bytes to keep from the tail of an `exec` result.
    pub exec_tail_bytes: usize,
    /// Maximum number of lines retained from an `exec` result.
    pub exec_max_lines: usize,
    /// Cap on the size of an error message rendered to the model.
    pub error_message_max_chars: usize,
    /// Probe-side cap used by the auto-permission LLM probe.
    pub probe_max_tool_output_chars: usize,
    /// Threshold above which a tool result is spooled to disk instead of
    /// inlined into the prompt.
    pub spool_threshold_bytes: usize,
}

impl Default for ToolOutputLimits {
    fn default() -> Self {
        Self {
            summary_max_input_chars: 12_000,
            summary_max_output_tokens: 400,
            read_head_bytes: 6_000,
            read_tail_bytes: 4_000,
            exec_tail_bytes: 10_000,
            exec_max_lines: 120,
            error_message_max_chars: 420,
            probe_max_tool_output_chars: 2_400,
            spool_threshold_bytes: 50_000,
        }
    }
}

impl ToolOutputLimits {
    /// Apply a head/tail truncation to `content` while keeping the configured
    /// byte budgets.
    #[must_use]
    pub fn truncate_head_tail(&self, content: &str) -> Option<(String, TruncationReport)> {
        let total = content.len();
        if total <= self.read_head_bytes + self.read_tail_bytes {
            return None;
        }
        // Boundary-safe slicing on char boundaries.
        let head_end = floor_char_boundary(content, self.read_head_bytes);
        let tail_start = ceil_char_boundary_from_end(content, self.read_tail_bytes);
        let mut out = String::with_capacity(self.read_head_bytes + self.read_tail_bytes + 64);
        out.push_str(&content[..head_end]);
        out.push_str("\n\n[... ");
        out.push_str(&total.to_string());
        out.push_str(" bytes truncated ...]\n\n");
        out.push_str(&content[tail_start..]);
        let kept_bytes = out.len();
        Some((
            out,
            TruncationReport {
                original_bytes: total,
                kept_bytes,
                head_bytes: head_end,
                tail_bytes: total - tail_start,
                kind: TruncationKind::HeadTail,
            },
        ))
    }

    /// Apply a tail-only truncation, preserving the most recent lines first.
    #[must_use]
    pub fn truncate_tail_only(&self, content: &str) -> Option<(String, TruncationReport)> {
        if content.len() <= self.exec_tail_bytes {
            return None;
        }
        let tail_start = ceil_char_boundary_from_end(content, self.exec_tail_bytes);
        let mut out = String::with_capacity(self.exec_tail_bytes + 32);
        out.push_str("[... earlier output elided ...]\n");
        out.push_str(&content[tail_start..]);
        let kept_bytes = out.len();
        let tail_bytes = content.len() - tail_start;
        Some((
            out,
            TruncationReport {
                original_bytes: content.len(),
                kept_bytes,
                head_bytes: 0,
                tail_bytes,
                kind: TruncationKind::TailOnly,
            },
        ))
    }
}

/// Description of a truncation that was applied.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TruncationReport {
    /// Bytes in the original content.
    pub original_bytes: usize,
    /// Bytes in the post-truncation content (incl. any marker text).
    pub kept_bytes: usize,
    /// Bytes kept from the head (0 when tail-only).
    pub head_bytes: usize,
    /// Bytes kept from the tail.
    pub tail_bytes: usize,
    /// Which kind of truncation was applied.
    pub kind: TruncationKind,
}

/// Which truncation strategy was applied.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TruncationKind {
    /// Both head and tail preserved.
    HeadTail,
    /// Only the tail preserved.
    TailOnly,
    /// No truncation applied (output fits within budget).
    None,
}

fn floor_char_boundary(content: &str, max_bytes: usize) -> usize {
    if max_bytes >= content.len() {
        return content.len();
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn ceil_char_boundary_from_end(content: &str, max_bytes: usize) -> usize {
    if max_bytes >= content.len() {
        return 0;
    }
    let mut start = content.len().saturating_sub(max_bytes);
    while start < content.len() && !content.is_char_boundary(start) {
        start += 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_tail_truncation_preserves_total_marker() {
        let limits = ToolOutputLimits {
            read_head_bytes: 4,
            read_tail_bytes: 4,
            ..ToolOutputLimits::default()
        };
        let long = "abcdefghijklmnopqrstuvwxyz";
        let (out, report) = limits.truncate_head_tail(long).expect("should truncate");
        assert!(out.contains("bytes truncated"));
        assert_eq!(report.original_bytes, long.len());
        assert_eq!(report.kind, TruncationKind::HeadTail);
    }

    #[test]
    fn no_truncation_when_within_budget() {
        let limits = ToolOutputLimits::default();
        let short = "hello";
        assert!(limits.truncate_head_tail(short).is_none());
        assert!(limits.truncate_tail_only(short).is_none());
    }

    #[test]
    fn tail_only_truncation_prepends_marker() {
        let limits = ToolOutputLimits {
            exec_tail_bytes: 4,
            ..ToolOutputLimits::default()
        };
        let long = "abcdefghijklmnop";
        let (out, report) = limits.truncate_tail_only(long).expect("should truncate");
        assert!(out.starts_with("[... earlier output elided ...]"));
        assert_eq!(report.kind, TruncationKind::TailOnly);
        assert_eq!(report.tail_bytes, 4);
    }

    #[test]
    fn defaults_match_pre_split_constants() {
        let d = ToolOutputLimits::default();
        assert_eq!(d.summary_max_input_chars, 12_000);
        assert_eq!(d.summary_max_output_tokens, 400);
        assert_eq!(d.exec_max_lines, 120);
        assert_eq!(d.spool_threshold_bytes, 50_000);
    }
}
