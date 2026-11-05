//! Diff preview utilities for file operations.

use crate::config::constants::diff;
use serde_json::{Value, json};
use std::time::Instant;
use vtcode_diff::{
    DiffDisplayKind, DiffDisplayLine, DiffDocument, DiffHunk, DiffOptions, count_diff_changes,
    display_lines_from_unified_diff, format_unified_hunks,
};

/// Create a diff preview response when content exceeds the size limit.
pub fn diff_preview_size_skip() -> Value {
    json!({
        "skipped": true,
        "reason": "content_exceeds_preview_limit",
        "max_bytes": diff::MAX_PREVIEW_BYTES
    })
}

/// Stable machine-readable reason for suppressed diff previews.
///
/// Kept stable for `ThreadEvent`/API consumers. Renderers must show
/// [`diff_preview_user_message`] instead of this code.
pub const SUPPRESSED_PREVIEW_REASON: &str = "too_many_changes";

/// Create a diff preview response when inline diffs are suppressed due to too many changes.
pub fn diff_preview_suppressed(additions: usize, deletions: usize, line_count: usize) -> Value {
    json!({
        "skipped": true,
        "suppressed": true,
        "reason": SUPPRESSED_PREVIEW_REASON,
        "message": diff::SUPPRESSION_MESSAGE,
        "summary": {
            "additions": additions,
            "deletions": deletions,
            "total_lines": line_count
        }
    })
}

/// User-facing one-line summary for a skipped/suppressed diff entry.
///
/// Prefers the producer-supplied `message`, maps known `reason` codes to
/// friendly text, and humanizes unknown snake_case codes. The stable `reason`
/// code itself is never surfaced for known cases.
pub fn diff_preview_user_message(preview: &Value) -> String {
    if let Some(message) = preview
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return message.to_owned();
    }
    let reason = preview.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    match reason {
        "too_many_changes" => {
            let additions = preview
                .get("additions")
                .and_then(Value::as_u64)
                .or_else(|| preview.get("summary").and_then(|s| s.get("additions")).and_then(Value::as_u64));
            let deletions = preview
                .get("deletions")
                .and_then(Value::as_u64)
                .or_else(|| preview.get("summary").and_then(|s| s.get("deletions")).and_then(Value::as_u64));
            match (additions, deletions) {
                (Some(a), Some(d)) => {
                    format!("Large change — preview suppressed (+{a} -{d}); use `git diff` for the full view")
                }
                _ => "Large change — preview suppressed; use `git diff` for the full view".to_owned(),
            }
        }
        "content_exceeds_preview_limit" => "Preview skipped — file exceeds the preview size limit".to_owned(),
        "" => "preview skipped".to_owned(),
        other => other.replace('_', " "),
    }
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

/// Return the canonical bounded diff entries from a file-operation response.
///
/// `diff[]` is the shared response contract for multi-file operations. The
/// legacy `diff_preview` object is normalized into a one-entry list so older
/// write/create responses render and persist through the same path.
pub fn canonical_diff_previews(output: &Value) -> Vec<Value> {
    if let Some(diffs) = output.get("diff").and_then(Value::as_array) {
        return diffs.clone();
    }

    let Some(preview) = output.get("diff_preview") else {
        return Vec::new();
    };

    let mut entry = preview.clone();
    if let Some(fields) = entry.as_object_mut() {
        if !fields.contains_key("path")
            && let Some(path) = output.get("path").and_then(Value::as_str)
        {
            fields.insert("path".to_string(), Value::String(path.to_string()));
        }
        if !fields.contains_key("operation") {
            let operation = if output.get("created").and_then(Value::as_bool) == Some(true)
                || output.get("file_existed").and_then(Value::as_bool) == Some(false)
            {
                "created"
            } else {
                "updated"
            };
            fields.insert("operation".to_string(), Value::String(operation.to_string()));
        }
    }

    vec![entry]
}

/// Determine whether a canonical diff contract represents an effective
/// mutation. `None` means the response is not a file-operation response and
/// callers should use the tool's normal success semantics.
pub fn diff_output_has_effective_change(output: &Value) -> Option<bool> {
    if output.get("skipped").and_then(Value::as_bool) == Some(true)
        || output.get("conflict").and_then(Value::as_bool) == Some(true)
        || output.get("success").and_then(Value::as_bool) == Some(false)
    {
        return Some(false);
    }

    if output.get("diff").is_none() && output.get("diff_preview").is_none() {
        return None;
    }

    Some(canonical_diff_previews(output).iter().any(diff_preview_has_effective_change))
}

fn diff_preview_has_effective_change(preview: &Value) -> bool {
    if preview.get("is_empty").and_then(Value::as_bool) == Some(true) {
        return matches!(preview.get("operation").and_then(Value::as_str), Some("created" | "deleted"));
    }

    if preview
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| !content.is_empty())
    {
        return true;
    }

    // A skipped preview still represents a completed mutation when the
    // operation itself succeeded; only an explicit `is_empty` preview is a
    // no-op. This covers bounded/suppressed previews without over-counting
    // writes that were skipped or conflicted at the response level.
    preview.get("skipped").and_then(Value::as_bool) == Some(true)
        || preview
            .get("summary")
            .and_then(|summary| summary.get("additions"))
            .and_then(Value::as_u64)
            .is_some_and(|additions| additions > 0)
        || preview
            .get("summary")
            .and_then(|summary| summary.get("deletions"))
            .and_then(Value::as_u64)
            .is_some_and(|deletions| deletions > 0)
}

/// Build a unified diff preview between before and after content.
pub fn build_diff_preview(path: &str, before: Option<&str>, after: &str) -> Value {
    let started = Instant::now();
    let previous = before.unwrap_or("");
    let old_label = format!("a/{path}");
    let new_label = format!("b/{path}");

    let options = DiffOptions {
        context_lines: diff::CONTEXT_RADIUS,
        old_label: Some(old_label.as_str()),
        new_label: Some(new_label.as_str()),
        missing_newline_hint: true,
        ..DiffOptions::default()
    };
    // Unified formatting normalizes `\r\n` to `\n` with no marker, so the
    // recorded bytes cannot reproduce the original exactly when a `\r` was
    // present. Exact-reversal consumers refuse such recordings.
    let normalized_terminators = previous.contains('\r') || after.contains('\r');
    let document = DiffDocument::between(previous, after, options.clone());
    // Tool responses carry a plain unified diff. The terminal/UI renderers
    // apply colors, gutters, and syntax highlighting after parsing the diff;
    // embedding ANSI here would hide hunk markers and line prefixes from that
    // parser and make apply_patch previews fall back to raw text.
    let formatted = format_unified_hunks(&document.hunks, &options);

    if formatted.trim().is_empty() {
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
            "is_empty": true,
            "normalized_terminators": normalized_terminators,
            "additions": 0,
            "deletions": 0
        });
    }

    let line_count = formatted.lines().count();
    let counts = count_diff_changes(&document.hunks);
    let additions = counts.additions;
    let deletions = counts.deletions;
    if line_count > diff::MAX_PREVIEW_LINES {
        let lines: Vec<&str> = formatted.lines().collect();
        let head_count = diff::HEAD_LINE_COUNT.min(lines.len());
        let base_tail_count = diff::TAIL_LINE_COUNT.min(lines.len().saturating_sub(head_count));
        let full_display = (document.hunks.len() > 1).then(|| display_lines_from_unified_diff(&formatted));
        let mut tail_count = base_tail_count;
        let mut tail_hunk_header = full_display.as_ref().and_then(|display| {
            bounded_tail_hunk_header(display, &document.hunks, lines.len().saturating_sub(tail_count))
        });
        if tail_hunk_header.is_some() && tail_count > 0 {
            tail_count = tail_count.saturating_sub(1);
            tail_hunk_header = full_display.as_ref().and_then(|display| {
                bounded_tail_hunk_header(display, &document.hunks, lines.len().saturating_sub(tail_count))
            });
            if tail_hunk_header.is_none() {
                tail_count = base_tail_count;
            }
        }
        let omitted = lines.len().saturating_sub(head_count + tail_count);

        let diff_output = if omitted > 0 {
            let mut result = lines[..head_count].join("\n");
            result.push_str(&format!("\n... {omitted} lines omitted ...\n"));
            if let Some(header) = tail_hunk_header {
                result.push_str(&header);
                result.push('\n');
            }
            result.push_str(&lines[lines.len().saturating_sub(tail_count)..].join("\n"));
            result
        } else {
            lines.join("\n")
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
            "skipped": false,
            "normalized_terminators": normalized_terminators,
            "additions": additions,
            "deletions": deletions
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
            "content": formatted,
            "truncated": false,
            "omitted_line_count": 0,
            "skipped": false,
            "normalized_terminators": normalized_terminators,
            "additions": additions,
            "deletions": deletions
        })
    }
}

fn bounded_tail_hunk_header(display: &[DiffDisplayLine], hunks: &[DiffHunk], tail_start: usize) -> Option<String> {
    let tail = display.get(tail_start..)?;
    let first_body_offset = tail.iter().position(DiffDisplayLine::is_diff)?;
    if tail[..first_body_offset]
        .iter()
        .any(|line| line.kind == DiffDisplayKind::HunkHeader)
    {
        return None;
    }

    let first_body_index = tail_start + first_body_offset;
    let hunk_header_index = display[..first_body_index]
        .iter()
        .rposition(|line| line.kind == DiffDisplayKind::HunkHeader)?;
    let hunk_index = display[..first_body_index]
        .iter()
        .filter(|line| line.kind == DiffDisplayKind::HunkHeader)
        .count()
        .checked_sub(1)?;
    let hunk = hunks.get(hunk_index)?;
    let prefix_line_count = display[hunk_header_index + 1..first_body_index]
        .iter()
        .filter(|line| line.is_diff())
        .count();
    let prefix = hunk.lines.get(..prefix_line_count)?;
    let mut old_start = hunk.old_start;
    let mut new_start = hunk.new_start;
    for line in prefix {
        if line.kind != vtcode_diff::DiffLineKind::Addition {
            old_start = old_start.saturating_add(1);
        }
        if line.kind != vtcode_diff::DiffLineKind::Deletion {
            new_start = new_start.saturating_add(1);
        }
    }

    let tail_lines = tail[first_body_offset..]
        .iter()
        .take_while(|line| line.kind != DiffDisplayKind::HunkHeader)
        .filter(|line| line.is_diff());
    let mut old_lines = 0usize;
    let mut new_lines = 0usize;
    for line in tail_lines {
        if line.kind != DiffDisplayKind::Addition {
            old_lines = old_lines.saturating_add(1);
        }
        if line.kind != DiffDisplayKind::Deletion {
            new_lines = new_lines.saturating_add(1);
        }
    }
    (old_lines > 0 || new_lines > 0).then(|| format!("@@ -{old_start},{old_lines} +{new_start},{new_lines} @@"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_commons::ansi::strip_ansi;

    use super::*;

    #[test]
    fn canonical_diff_previews_normalize_legacy_write_output() {
        let previews = canonical_diff_previews(&json!({
            "path": "README.md",
            "file_existed": true,
            "diff_preview": {"content": "diff", "skipped": false}
        }));

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0]["path"], "README.md");
        assert_eq!(previews[0]["operation"], "updated");
    }

    #[test]
    fn effective_change_distinguishes_noop_skipped_and_empty_file_operations() {
        assert_eq!(
            diff_output_has_effective_change(&json!({
                "success": true,
                "diff": [{"operation": "updated", "is_empty": true}]
            })),
            Some(false)
        );
        assert_eq!(
            diff_output_has_effective_change(&json!({"success": true, "skipped": true, "diff": []})),
            Some(false)
        );
        assert_eq!(
            diff_output_has_effective_change(&json!({
                "success": true,
                "diff": [{"operation": "created", "is_empty": true}]
            })),
            Some(true)
        );
    }

    #[test]
    fn suppressed_preview_user_message_prefers_message_and_maps_reason_codes() {
        let suppressed = diff_preview_suppressed(12, 3, 40);
        assert_eq!(suppressed["reason"], SUPPRESSED_PREVIEW_REASON);
        let rendered = diff_preview_user_message(&suppressed);
        assert_eq!(rendered, diff::SUPPRESSION_MESSAGE);
        assert!(!rendered.contains("too_many_changes"));

        let reason_only = json!({
            "skipped": true,
            "reason": SUPPRESSED_PREVIEW_REASON,
            "summary": {"additions": 12, "deletions": 3}
        });
        let fallback = diff_preview_user_message(&reason_only);
        assert!(fallback.contains("+12 -3"), "fallback should keep counts: {fallback}");
        assert!(!fallback.contains("too_many_changes"));

        let oversized = json!({"skipped": true, "reason": "content_exceeds_preview_limit"});
        assert_eq!(diff_preview_user_message(&oversized), "Preview skipped — file exceeds the preview size limit");

        let unknown = json!({"skipped": true, "reason": "custom_retry_later"});
        assert_eq!(diff_preview_user_message(&unknown), "custom retry later");
    }

    #[test]
    fn build_diff_preview_keeps_serialized_content_plain_for_ui_parsing() {
        let preview = build_diff_preview("README.md", Some("before\n"), "after\n");
        let content = preview
            .get("content")
            .and_then(Value::as_str)
            .expect("changed preview should contain diff content");

        assert_eq!(content, strip_ansi(content));
        assert!(content.contains("-before"));
        assert!(content.contains("+after"));
    }

    #[test]
    fn large_change_uses_bounded_head_tail_instead_of_suppression() {
        let before = (0..=diff::MAX_SINGLE_FILE_CHANGES)
            .map(|index| format!("old-{index}\n"))
            .collect::<String>();
        let after = (0..=diff::MAX_SINGLE_FILE_CHANGES)
            .map(|index| format!("new-{index}\n"))
            .collect::<String>();

        let preview = build_diff_preview("large.txt", Some(&before), &after);

        assert_eq!(preview["skipped"], false);
        assert_eq!(preview["truncated"], true);
        assert!(preview["omitted_line_count"].as_u64().is_some_and(|count| count > 0));
        let content = preview["content"].as_str().expect("bounded diff content");
        assert!(content.contains("lines omitted"));
        assert!(content.contains("old-0"));
        assert!(content.contains(&format!("new-{}", diff::MAX_SINGLE_FILE_CHANGES)));

        let display_lines = display_lines_from_unified_diff(content);
        let tail = display_lines
            .iter()
            .find(|line| line.text.starts_with("new-200"))
            .expect("tail line");
        assert_eq!(tail.new_line, Some(201));
    }

    #[test]
    fn large_multi_hunk_preview_keeps_tail_hunk_numbers() {
        let before = (0..600).map(|index| format!("old-{index}\n")).collect::<String>();
        let mut after_lines = (0..600).map(|index| format!("old-{index}\n")).collect::<Vec<_>>();
        for (index, line) in after_lines.iter_mut().enumerate().take(220).skip(100) {
            *line = format!("new-{index}\n");
        }
        for (index, line) in after_lines.iter_mut().enumerate().take(520).skip(400) {
            *line = format!("new-{index}\n");
        }

        let preview = build_diff_preview("large-multi.txt", Some(&before), &after_lines.concat());
        let content = preview["content"].as_str().expect("bounded diff content");
        let display_lines = display_lines_from_unified_diff(content);
        let tail = display_lines
            .iter()
            .find(|line| line.text.starts_with("new-519"))
            .expect("tail hunk line");
        assert_eq!(tail.new_line, Some(520));
    }

    #[test]
    fn build_preview_flags_normalized_terminators() {
        let clean = build_diff_preview("lf.txt", Some("a\nb\n"), "a\nB\n");
        assert_eq!(clean["normalized_terminators"], false);

        let crlf_before = build_diff_preview("crlf.txt", Some("a\r\nb\r\n"), "a\r\nB\r\n");
        assert_eq!(crlf_before["normalized_terminators"], true);

        // Converted endings still flag: the recorded old side no longer
        // reproduces the original bytes.
        let converted = build_diff_preview("conv.txt", Some("a\r\nb\r\n"), "a\nB\n");
        assert_eq!(converted["normalized_terminators"], true);

        let empty = build_diff_preview("empty.txt", Some(""), "");
        assert_eq!(empty["normalized_terminators"], false);
    }
}
