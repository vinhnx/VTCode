use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::styles::{GitStyles, LsStyles, select_line_style};

/// Constants for line and content limits
const MAX_DIFF_LINES: usize = 500;
const MAX_DIFF_LINE_LENGTH: usize = 200;

/// Safely truncate text to a maximum character length, respecting UTF-8 boundaries
pub(super) fn truncate_text_safe(text: &str, max_chars: usize) -> &str {
    if text.len() <= max_chars {
        return text;
    }
    text.char_indices()
        .nth(max_chars)
        .map(|(i, _)| &text[..i])
        .unwrap_or(text)
}

/// Helper to extract optional string from JSON value
fn get_string<'a>(val: &'a Value, key: &str) -> Option<&'a str> {
    val.get(key).and_then(|v| v.as_str())
}

/// Helper to extract optional boolean from JSON value
fn get_bool(val: &Value, key: &str) -> bool {
    val.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Helper to extract optional u64 from JSON value
fn get_u64(val: &Value, key: &str) -> Option<u64> {
    val.get(key).and_then(|v| v.as_u64())
}

pub(crate) fn render_write_file_preview(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    if let Some(encoding) = get_string(payload, "encoding") {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    if get_bool(payload, "created") {
        renderer.line(MessageStyle::Response, "  File created")?;
    }

    let diff_value = match payload.get("diff_preview") {
        Some(value) => value,
        None => return Ok(()),
    };

    if get_bool(diff_value, "skipped") {
        let reason = get_string(diff_value, "reason").unwrap_or("diff preview skipped");
        renderer.line(
            MessageStyle::Info,
            &format!("  diff preview skipped: {reason}"),
        )?;

        if let Some(detail) = get_string(diff_value, "detail") {
            renderer.line(MessageStyle::Info, &format!("  detail: {detail}"))?;
        }

        if let Some(max_bytes) = get_u64(diff_value, "max_bytes") {
            renderer.line(
                MessageStyle::Info,
                &format!("  preview limit: {max_bytes} bytes"),
            )?;
        }
        return Ok(());
    }

    let diff_content = get_string(diff_value, "content").unwrap_or("");

    if diff_content.is_empty() && get_bool(diff_value, "is_empty") {
        renderer.line(MessageStyle::Info, "  No diff changes to display.")?;
    }

    if !diff_content.is_empty() {
        renderer.line(MessageStyle::Info, "[diff]")?;
        // Use higher limit for diffs since they're already filtered by token limit in render_stream_section
        // Diffs are usually sparse (many unchanged lines) so line-based preview is reasonable here
        render_diff_content(renderer, diff_content, git_styles, ls_styles)?;
    }

    if get_bool(diff_value, "truncated") {
        if let Some(omitted) = get_u64(diff_value, "omitted_line_count") {
            renderer.line(
                MessageStyle::Info,
                &format!("  ... diff truncated ({omitted} lines omitted)"),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "  ... diff truncated")?;
        }
    }

    Ok(())
}

pub(crate) fn render_list_dir_output(
    renderer: &mut AnsiRenderer,
    val: &Value,
    _ls_styles: &LsStyles,
) -> Result<()> {
    if let Some(path) = get_string(val, "path") {
        renderer.line(MessageStyle::Info, &format!("  {}", path))?;
    }

    // Show pagination summary
    let count = get_u64(val, "count").unwrap_or(0);
    let total = get_u64(val, "total").unwrap_or(0);
    let page = get_u64(val, "page").unwrap_or(1);
    let has_more = get_bool(val, "has_more");

    if count > 0 || total > 0 {
        let summary = if total > count {
            // Multi-page results
            if has_more {
                format!(
                    "  Page {} of ~{} ({} items per page, {} total)",
                    page,
                    (total + count - 1) / count, // Estimate total pages
                    count,
                    total
                )
            } else {
                format!("  Page {} ({} items, {} total)", page, count, total)
            }
        } else {
            // Single page with all results
            format!("  {} items", count)
        };
        renderer.line(MessageStyle::Info, &summary)?;
    }

    // Render items
    if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            renderer.line(MessageStyle::Info, "  (empty directory)")?;
        } else {
            for item in items {
                if let Some(name) = get_string(item, "name") {
                    let item_type = get_string(item, "type").unwrap_or("file");
                    let size = get_u64(item, "size");

                    let display_name = if item_type == "directory" {
                        format!("{}/", name)
                    } else {
                        name.to_string()
                    };

                    let display = if let Some(size_bytes) = size {
                        format!("  {} ({})", display_name, format_size(size_bytes))
                    } else {
                        format!("  {}", display_name)
                    };

                    renderer.line(MessageStyle::Response, &display)?;
                }
            }
        }
    }

    // Show pagination instructions if more results available
    if has_more {
        renderer.line(
            MessageStyle::Info,
            "  Use page=N to view other pages (e.g., page=2, page=3)",
        )?;
    }

    // Show any additional guidance message from the tool
    if let Some(message) = get_string(val, "message").filter(|s| !s.is_empty()) {
        renderer.line(MessageStyle::Info, &format!("  {}", message))?;
    }

    Ok(())
}

pub(crate) fn render_read_file_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(encoding) = get_string(val, "encoding") {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    if let Some(size) = get_u64(val, "size") {
        renderer.line(
            MessageStyle::Info,
            &format!("  size: {}", format_size(size)),
        )?;
    }

    if let Some(start) = get_u64(val, "start_line")
        && let Some(end) = get_u64(val, "end_line")
    {
        renderer.line(MessageStyle::Info, &format!("  lines: {}-{}", start, end))?;
    }

    Ok(())
}

/// Render diff content lines with proper truncation and styling
fn render_diff_content(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let mut line_count = 0;
    let total_lines = diff_content.lines().count();

    for line in diff_content.lines() {
        if line_count >= MAX_DIFF_LINES {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "  ... ({} more lines truncated, view full diff in tool logs)",
                    total_lines - MAX_DIFF_LINES
                ),
            )?;
            break;
        }

        let truncated = truncate_text_safe(line, MAX_DIFF_LINE_LENGTH);
        let display = format!("  {truncated}");

        if let Some(style) =
            select_line_style(Some(tools::WRITE_FILE), truncated, git_styles, ls_styles)
        {
            renderer.line_with_style(style, &display)?;
        } else {
            renderer.line(MessageStyle::Response, &display)?;
        }
        line_count += 1;
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
