//! Single-file line presentation: truncation, condensing, and range assembly.
//!
//! Pure line-shaping helpers shared by the slice and indentation readers.
//! Moved out of the handler root so range assembly (`range_result_from_lines`)
//! and output condensing live beside the pre-existing `batch` submodule.
//! No behavior change: truncation limits, condense thresholds, and the
//! diff-content exemption are verbatim.

use std::collections::VecDeque;

use vtcode_commons::diff_paths::looks_like_diff_content;

use super::{BATCH_CONDENSED_THRESHOLD, LineRecord, MAX_LINE_LENGTH, RangeResult};

pub(super) fn range_result_from_lines(offset: usize, mut collected: Vec<String>, line_truncated: bool) -> RangeResult {
    let original_len = collected.len();
    let is_paginated = offset > 1;
    let (condensed, omitted) = if is_paginated {
        (false, 0)
    } else {
        condense_for_batch(&mut collected)
    };

    RangeResult {
        offset,
        lines_read: original_len,
        condensed,
        omitted_lines: (omitted > 0).then_some(omitted),
        line_truncated,
        content: collected.join("\n"),
    }
}

pub(super) fn format_line(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    if decoded.len() > MAX_LINE_LENGTH {
        take_bytes_at_char_boundary(&decoded, MAX_LINE_LENGTH).to_string()
    } else {
        decoded.into_owned()
    }
}

fn take_bytes_at_char_boundary(s: &str, limit: usize) -> &str {
    if limit >= s.len() {
        return s;
    }
    let mut i = limit;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    &s[..i]
}

pub(super) fn trim_empty_lines(out: &mut VecDeque<&LineRecord>) {
    while matches!(out.front(), Some(line) if line.raw.trim().is_empty()) {
        out.pop_front();
    }
    while matches!(out.back(), Some(line) if line.raw.trim().is_empty()) {
        out.pop_back();
    }
}

pub(super) fn condense_collected_lines(lines: &mut Vec<String>) {
    if looks_like_diff_lines(lines) {
        return;
    }
    const CONDENSED_THRESHOLD: usize = 200;
    const HEAD_LINES: usize = 80;
    const TAIL_LINES: usize = 40;

    // If under threshold, return as-is
    if lines.len() <= CONDENSED_THRESHOLD {
        return;
    }

    // Build condensed output: head + omission indicator + tail
    let head_count = HEAD_LINES.min(lines.len());
    let tail_count = TAIL_LINES.min(lines.len() - head_count);
    let omitted_count = lines.len() - head_count - tail_count;
    let total_lines = lines.len();

    // Take head lines
    let mut condensed: Vec<String> = lines[..head_count].to_vec();

    // Add omission indicator with actionable guidance
    condensed.push(format!(
        "… [+{} lines omitted ({} total). To read omitted ranges: file_operation offset={} limit={}]",
        omitted_count,
        total_lines,
        head_count + 1,
        omitted_count
    ));

    // Add tail lines
    let tail_start = lines.len() - tail_count;
    condensed.extend_from_slice(&lines[tail_start..]);

    // Replace original with condensed
    *lines = condensed;
}

/// Condense lines for batch mode with stricter threshold.
/// Returns (was_condensed, omitted_count).
pub(super) fn condense_for_batch(lines: &mut Vec<String>) -> (bool, usize) {
    if looks_like_diff_lines(lines) {
        return (false, 0);
    }
    const HEAD_LINES: usize = 15;
    const TAIL_LINES: usize = 5;

    if lines.len() <= BATCH_CONDENSED_THRESHOLD {
        return (false, 0);
    }

    let head_count = HEAD_LINES.min(lines.len());
    let tail_count = TAIL_LINES.min(lines.len() - head_count);
    let omitted_count = lines.len() - head_count - tail_count;

    let mut condensed: Vec<String> = lines[..head_count].to_vec();
    condensed.push(format!(
        "… [+{} lines omitted ({} total). To read omitted ranges: file_operation offset={} limit={}]",
        omitted_count,
        lines.len(),
        head_count + 1,
        omitted_count
    ));

    let tail_start = lines.len() - tail_count;
    condensed.extend_from_slice(&lines[tail_start..]);

    *lines = condensed;
    (true, omitted_count)
}

fn looks_like_diff_lines(lines: &[String]) -> bool {
    let joined = lines.join("\n");
    looks_like_diff_content(&joined)
}
