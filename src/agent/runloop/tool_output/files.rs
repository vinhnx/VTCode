use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::styles::{GitStyles, LsStyles, select_line_style};
#[path = "files_diff.rs"]
mod files_diff;
pub(super) use files_diff::{format_condensed_edit_diff_lines, format_diff_content_lines};

/// Constants for line and content limits (compact display)
const MAX_DIFF_LINE_LENGTH: usize = 100; // Reduced for compact view
const MAX_DISPLAYED_FILES: usize = 100; // Limit displayed files to reduce clutter

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
    // Show basic metadata (compact format)
    if get_bool(payload, "created") {
        renderer.line(MessageStyle::ToolDetail, "File created")?;
    }

    if let Some(encoding) = get_string(payload, "encoding") {
        renderer.line(MessageStyle::ToolDetail, &format!("encoding: {}", encoding))?;
    }

    // Handle diff preview
    let diff_value = match payload.get("diff_preview") {
        Some(value) => value,
        None => return Ok(()),
    };

    if get_bool(diff_value, "skipped") {
        let reason = get_string(diff_value, "reason").unwrap_or("skipped");
        if let Some(detail) = get_string(diff_value, "detail") {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("diff: {} ({})", reason, detail),
            )?;
        } else {
            renderer.line(MessageStyle::ToolDetail, &format!("diff: {}", reason))?;
        }
        return Ok(());
    }

    let diff_content = get_string(diff_value, "content").unwrap_or("");

    if diff_content.is_empty() && get_bool(diff_value, "is_empty") {
        renderer.line(MessageStyle::ToolDetail, "(no changes)")?;
        return Ok(());
    }

    if !diff_content.is_empty() {
        renderer.line(MessageStyle::ToolDetail, "")?;
        render_edit_diff_preview(renderer, diff_content, git_styles, ls_styles)?;
    }

    if get_bool(diff_value, "truncated") {
        if let Some(omitted) = get_u64(diff_value, "omitted_line_count") {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("… +{} lines (use read_file for full view)", omitted),
            )?;
        } else {
            renderer.line(MessageStyle::ToolDetail, "… diff truncated")?;
        }
    }

    Ok(())
}

pub(crate) fn render_list_dir_output(
    renderer: &mut AnsiRenderer,
    val: &Value,
    _ls_styles: &LsStyles,
) -> Result<()> {
    // Get pagination info first
    let count = get_u64(val, "count").unwrap_or(0);
    let total = get_u64(val, "total").unwrap_or(0);
    let page = get_u64(val, "page").unwrap_or(1);
    let _has_more = get_bool(val, "has_more");
    let per_page = get_u64(val, "per_page").unwrap_or(20);

    // Show path - always display root directory for clarity
    if let Some(path) = get_string(val, "path") {
        let display_path = if path.is_empty() { "/" } else { path };
        renderer.line(
            MessageStyle::ToolDetail,
            &format!(
                "{}{}",
                display_path,
                if !path.is_empty() { "/" } else { "" }
            ),
        )?;
    }

    // Show summary - compact format
    if count > 0 || total > 0 {
        let start_idx = (page - 1) * per_page + 1;
        let _end_idx = start_idx + count - 1;

        // Simplified summary without pagination details that confuse the agent
        let summary = if total > count {
            format!("Showing {} of {} items", count, total)
        } else {
            format!("{} items total", count)
        };
        renderer.line(MessageStyle::ToolDetail, &summary)?;
    }

    // Render items grouped by type
    if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            renderer.line(MessageStyle::ToolDetail, "(empty)")?;
        } else {
            let mut directories = Vec::new();
            let mut files = Vec::new();

            // Group items by type
            for item in items.iter().take(MAX_DISPLAYED_FILES) {
                if let Some(name) = get_string(item, "name") {
                    let item_type = get_string(item, "type").unwrap_or("file");
                    let size = get_u64(item, "size");

                    if item_type == "directory" {
                        directories.push((name.to_string(), size));
                    } else {
                        files.push((name.to_string(), size));
                    }
                }
            }

            // Get sort order from the JSON value, defaulting to alphabetical by name
            let sort_order = get_string(val, "sort").unwrap_or("name");

            // Sort each group based on the specified sort order
            match sort_order {
                "size" => {
                    // Sort by size (largest first), with None sizes treated as 0
                    directories.sort_by(|a, b| b.1.unwrap_or(0).cmp(&a.1.unwrap_or(0)));
                    files.sort_by(|a, b| b.1.unwrap_or(0).cmp(&a.1.unwrap_or(0)));
                }
                "name" => {
                    // Sort alphabetically (case-insensitive for natural sorting)
                    directories.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                    files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                }
                "type" => {
                    // Sort by type/extension (files with extensions first, then by extension)
                    directories.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                    files.sort_by(|a, b| {
                        let ext_a = std::path::Path::new(&a.0)
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        let ext_b = std::path::Path::new(&b.0)
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default();

                        ext_a
                            .cmp(&ext_b)
                            .then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
                    });
                }
                _ => {
                    // Default to alphabetical sorting
                    directories.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                    files.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                }
            }

            // Calculate max name width for directories (with trailing /) and files
            let max_name_width = if !directories.is_empty() || !files.is_empty() {
                let dir_max_width = directories
                    .iter()
                    .map(|(name, _)| name.len() + 1) // +1 for trailing /
                    .max()
                    .unwrap_or(10)
                    .min(40);

                let file_max_width = files
                    .iter()
                    .map(|(name, _)| name.len())
                    .max()
                    .unwrap_or(10)
                    .min(40);

                dir_max_width.max(file_max_width)
            } else {
                10 // Default width if no items
            };

            // Render directories first with section header
            if !directories.is_empty() {
                renderer.line(MessageStyle::ToolDetail, "[Directories]")?;
                for (name, _size) in &directories {
                    let name_with_slash = format!("{}/", name);
                    let display = format!("{:<width$}", name_with_slash, width = max_name_width,);
                    renderer.line(MessageStyle::ToolDetail, &display)?;
                }

                // Add visual separation between directories and files
                if !files.is_empty() {
                    renderer.line(MessageStyle::ToolDetail, "")?; // Add blank line
                }
            }

            // Render files with section header
            if !files.is_empty() {
                renderer.line(MessageStyle::ToolDetail, "[Files]")?;
                for (name, _size) in &files {
                    // Simple file name display without size or emoji
                    let display = format!("{:<width$}", name, width = max_name_width,);
                    renderer.line(MessageStyle::ToolDetail, &display)?;
                }
            }

            let omitted = items.len().saturating_sub(MAX_DISPLAYED_FILES);
            if omitted > 0 {
                renderer.line(
                    MessageStyle::ToolDetail,
                    &format!("+ {} more items not shown", omitted),
                )?;
            }
        }
    }

    // Pagination navigation removed - agent should work with first page results
    // If more items exist, agent can call list_files again with specific page parameter

    Ok(())
}

pub(crate) fn render_read_file_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Batch read: show compact per-file summary
    if let Some(items) = val.get("items").and_then(Value::as_array) {
        let files_read = get_u64(val, "files_read").unwrap_or(items.len() as u64);
        let files_ok = get_u64(val, "files_succeeded").unwrap_or(0);
        let failed = files_read.saturating_sub(files_ok);

        let mut summary = format!(
            "{} file{} read",
            files_ok,
            if files_ok == 1 { "" } else { "s" }
        );
        if failed > 0 {
            summary.push_str(&format!(", {} failed", failed));
        }
        renderer.line(MessageStyle::ToolDetail, &summary)?;

        for item in items.iter().take(MAX_BATCH_DISPLAY_FILES) {
            if let Some(fp) = item.get("file_path").and_then(Value::as_str) {
                let short = shorten_path(fp, 60);
                if item.get("error").is_some() {
                    renderer.line(MessageStyle::ToolError, &format!("  ✗ {}", short))?;
                } else {
                    let lines_info = item
                        .get("ranges")
                        .and_then(Value::as_array)
                        .map(|ranges| {
                            let total_lines: u64 = ranges
                                .iter()
                                .filter_map(|r| r.get("lines_read").and_then(Value::as_u64))
                                .sum();
                            format!(" ({} lines)", total_lines)
                        })
                        .unwrap_or_default();
                    renderer.line(
                        MessageStyle::ToolDetail,
                        &format!("  ✓ {}{}", short, lines_info),
                    )?;
                }
            }
        }
        if items.len() > MAX_BATCH_DISPLAY_FILES {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("  … +{} more", items.len() - MAX_BATCH_DISPLAY_FILES),
            )?;
        }
        return Ok(());
    }

    // Single file read: show line range and content preview
    if let Some(start) = get_u64(val, "start_line")
        && let Some(end) = get_u64(val, "end_line")
    {
        let count = end.saturating_sub(start) + 1;
        renderer.line(
            MessageStyle::ToolDetail,
            &format!("lines {}-{} ({} lines)", start, end, count),
        )?;
    }

    if let Some(content) = get_string(val, "content") {
        if looks_like_diff_content(content) {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            renderer.line(MessageStyle::ToolDetail, "")?;
            render_diff_content(renderer, content, &git_styles, &ls_styles)?;
        } else {
            render_content_preview(renderer, content)?;
        }
    }

    Ok(())
}

const MAX_BATCH_DISPLAY_FILES: usize = 10;
const MAX_PREVIEW_LINES: usize = 4;
const MAX_PREVIEW_LINE_LEN: usize = 80;

fn shorten_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    if let Some(name) = std::path::Path::new(path).file_name() {
        let name_str = name.to_string_lossy();
        if let Some(parent) = std::path::Path::new(path).parent() {
            let parent_str = parent.to_string_lossy();
            let budget = max_len.saturating_sub(name_str.len() + 4);
            if budget > 0 && parent_str.len() > budget {
                return format!("…{}/{}", &parent_str[parent_str.len() - budget..], name_str);
            }
        }
        return name_str.to_string();
    }
    truncate_text_safe(path, max_len).to_string()
}

fn strip_line_number(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(pos) = trimmed.find(':') {
        let prefix = &trimmed[..pos];
        if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
            let rest = &trimmed[pos + 1..];
            return rest.strip_prefix(' ').unwrap_or(rest);
        }
    }
    line
}

fn render_content_preview(renderer: &mut AnsiRenderer, content: &str) -> Result<()> {
    let meaningful: Vec<&str> = content
        .lines()
        .map(strip_line_number)
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(MAX_PREVIEW_LINES)
        .collect();

    if meaningful.is_empty() {
        return Ok(());
    }

    renderer.line(MessageStyle::ToolDetail, "")?;
    for line in &meaningful {
        let display = truncate_text_safe(line, MAX_PREVIEW_LINE_LEN);
        renderer.line(MessageStyle::ToolDetail, &format!("  {}", display))?;
    }

    Ok(())
}

/// Render diff content lines with proper truncation and styling (compact format)
fn render_diff_content(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let lines_to_render = format_diff_content_lines(diff_content);

    for line in lines_to_render {
        let truncated = truncate_text_safe(&line, MAX_DIFF_LINE_LENGTH);
        if let Some(style) =
            select_line_style(Some(tools::WRITE_FILE), truncated, git_styles, ls_styles)
        {
            renderer.line_with_override_style(MessageStyle::ToolDetail, style, truncated)?;
        } else {
            renderer.line(MessageStyle::ToolDetail, truncated)?;
        }
    }

    Ok(())
}

fn render_edit_diff_preview(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let lines_to_render = format_condensed_edit_diff_lines(diff_content);

    for line in lines_to_render {
        let truncated = truncate_text_safe(&line, MAX_DIFF_LINE_LENGTH);
        if let Some(style) =
            select_line_style(Some(tools::WRITE_FILE), truncated, git_styles, ls_styles)
        {
            renderer.line_with_override_style(MessageStyle::ToolDetail, style, truncated)?;
        } else {
            renderer.line(MessageStyle::ToolDetail, truncated)?;
        }
    }

    Ok(())
}

pub(super) fn colorize_diff_summary_line(line: &str, _supports_color: bool) -> Option<String> {
    let trimmed = line.trim_start();
    let is_summary = trimmed.contains(" file changed")
        || trimmed.contains(" files changed")
        || trimmed.contains(" insertion(+)")
        || trimmed.contains(" insertions(+)")
        || trimmed.contains(" deletion(-)")
        || trimmed.contains(" deletions(-)");
    if is_summary {
        Some(line.to_string())
    } else {
        None
    }
}

fn looks_like_diff_content(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("diff --")
            || trimmed.starts_with("index ")
            || trimmed.starts_with("@@")
            || trimmed.starts_with("---")
            || trimmed.starts_with("+++")
            || trimmed.starts_with("new file mode")
            || trimmed.starts_with("deleted file mode")
    })
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
