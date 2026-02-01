//! Diff preview utilities for file operations.

use crate::config::constants::diff;
use crate::utils::diff::{DiffOptions, compute_diff_with_theme};
use serde_json::{Value, json};
use std::time::Instant;

/// Create a diff preview response when content exceeds the size limit.
pub fn diff_preview_size_skip() -> Value {
    json!({
        "skipped": true,
        "reason": "content_exceeds_preview_limit",
        "max_bytes": diff::MAX_PREVIEW_BYTES
    })
}

/// Create a diff preview response when inline diffs are suppressed due to too many changes.
pub fn diff_preview_suppressed(additions: usize, deletions: usize, line_count: usize) -> Value {
    json!({
        "skipped": true,
        "suppressed": true,
        "reason": "too_many_changes",
        "message": diff::SUPPRESSION_MESSAGE,
        "summary": {
            "additions": additions,
            "deletions": deletions,
            "total_lines": line_count
        }
    })
}

/// Create a diff preview response when an error prevents diff generation.
pub fn diff_preview_error_skip(reason: &str, detail: Option<&str>) -> Value {
    match detail {
        Some(value) => json!({
            "skipped": true,
            "reason": reason,
            "detail": value
        }),
        None => json!({
            "skipped": true,
            "reason": reason
        }),
    }
}

/// Build a unified diff preview between before and after content.
pub fn build_diff_preview(path: &str, before: Option<&str>, after: &str) -> Value {
    let started = Instant::now();
    let previous = before.unwrap_or("");
    let old_label = format!("a/{path}");
    let new_label = format!("b/{path}");

    let diff_bundle = compute_diff_with_theme(
        previous,
        after,
        DiffOptions {
            context_lines: diff::CONTEXT_RADIUS,
            old_label: Some(old_label.as_str()),
            new_label: Some(new_label.as_str()),
            missing_newline_hint: true,
        },
    );

    if diff_bundle.formatted.trim().is_empty() {
        tracing::debug!(
            target: "vtcode.tools.diff",
            path,
            before_bytes = previous.len(),
            after_bytes = after.len(),
            additions = 0,
            deletions = 0,
            line_count = 0,
            truncated = false,
            suppressed = false,
            elapsed_ms = started.elapsed().as_millis(),
            "diff preview generated"
        );

        return json!({
            "content": "",
            "truncated": false,
            "omitted_line_count": 0,
            "skipped": false,
            "is_empty": true
        });
    }

    let line_count = diff_bundle.formatted.lines().count();
    let (additions, deletions) =
        diff_bundle
            .formatted
            .lines()
            .fold((0usize, 0usize), |(add, del), line| {
                match line.chars().next() {
                    Some('+') => (add + 1, del),
                    Some('-') => (add, del + 1),
                    _ => (add, del),
                }
            });
    let total_changes = additions + deletions;

    if total_changes > diff::MAX_SINGLE_FILE_CHANGES {
        tracing::debug!(
            target: "vtcode.tools.diff",
            path,
            before_bytes = previous.len(),
            after_bytes = after.len(),
            additions,
            deletions,
            line_count,
            truncated = false,
            suppressed = true,
            elapsed_ms = started.elapsed().as_millis(),
            "diff preview suppressed (too many changes)"
        );

        return diff_preview_suppressed(additions, deletions, line_count);
    }

    if line_count > diff::MAX_PREVIEW_LINES {
        let lines: Vec<&str> = diff_bundle.formatted.lines().collect();
        let head_count = diff::HEAD_LINE_COUNT.min(lines.len());
        let tail_count = diff::TAIL_LINE_COUNT.min(lines.len().saturating_sub(head_count));
        let omitted = lines.len().saturating_sub(head_count + tail_count);

        let mut condensed = Vec::with_capacity(head_count + tail_count + 1);
        condensed.extend(lines[..head_count].iter().copied());
        if omitted > 0 {
            condensed.push("");
        }
        if tail_count > 0 {
            let tail_start = lines.len().saturating_sub(tail_count);
            condensed.extend(lines[tail_start..].iter().copied());
        }

        let diff_output = if omitted > 0 {
            let mut result = condensed[..head_count].join("\n");
            result.push_str(&format!("\n... {omitted} lines omitted ...\n"));
            result.push_str(&condensed[head_count + 1..].join("\n"));
            result
        } else {
            condensed.join("\n")
        };

        let elapsed = started.elapsed().as_millis();

        tracing::debug!(
            target: "vtcode.tools.diff",
            path,
            before_bytes = previous.len(),
            after_bytes = after.len(),
            additions,
            deletions,
            line_count,
            omitted_lines = omitted,
            truncated = true,
            suppressed = false,
            elapsed_ms = elapsed,
            "diff preview generated"
        );

        json!({
            "content": diff_output,
            "truncated": true,
            "omitted_line_count": omitted,
            "skipped": false
        })
    } else {
        let elapsed = started.elapsed().as_millis();

        tracing::debug!(
            target: "vtcode.tools.diff",
            path,
            before_bytes = previous.len(),
            after_bytes = after.len(),
            additions,
            deletions,
            line_count,
            truncated = false,
            suppressed = false,
            elapsed_ms = elapsed,
            "diff preview generated"
        );

        json!({
            "content": diff_bundle.formatted,
            "truncated": false,
            "omitted_line_count": 0,
            "skipped": false
        })
    }
}
