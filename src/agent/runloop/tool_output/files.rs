use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::styles::{GitStyles, LsStyles, select_line_style};

/// Constants for line and content limits (compact display)
const MAX_DIFF_LINES: usize = 30; // Reduced for compact view
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
        renderer.line(MessageStyle::Response, "File created")?;
    }

    if let Some(encoding) = get_string(payload, "encoding") {
        renderer.line(MessageStyle::Info, &format!("encoding: {}", encoding))?;
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
                MessageStyle::Info,
                &format!("diff: {} ({})", reason, detail),
            )?;
        } else {
            renderer.line(MessageStyle::Info, &format!("diff: {}", reason))?;
        }
        return Ok(());
    }

    let diff_content = get_string(diff_value, "content").unwrap_or("");

    if diff_content.is_empty() && get_bool(diff_value, "is_empty") {
        renderer.line(MessageStyle::Info, "(no changes)")?;
        return Ok(());
    }

    if !diff_content.is_empty() {
        renderer.line(MessageStyle::Info, "▼ diff")?;
        render_diff_content(renderer, diff_content, git_styles, ls_styles)?;
    }

    if get_bool(diff_value, "truncated") {
        if let Some(omitted) = get_u64(diff_value, "omitted_line_count") {
            renderer.line(
                MessageStyle::Info,
                &format!("… +{} lines (use read_file for full view)", omitted),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "… diff truncated")?;
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
            MessageStyle::Info,
            &format!(
                "  {}{}",
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
            format!("  Showing {} of {} items", count, total)
        } else {
            format!("  {} items total", count)
        };
        renderer.line(MessageStyle::Info, &summary)?;
    }

    // Render items grouped by type
    if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            renderer.line(MessageStyle::Info, "  (empty)")?;
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
                renderer.line(MessageStyle::Info, "  [Directories]")?;
                for (name, _size) in &directories {
                    let name_with_slash = format!("{}/", name);
                    let display = format!("  {:<width$}", name_with_slash, width = max_name_width,);
                    renderer.line(MessageStyle::Response, &display)?;
                }

                // Add visual separation between directories and files
                if !files.is_empty() {
                    renderer.line(MessageStyle::Info, "  ")?; // Add blank line
                }
            }

            // Render files with section header
            if !files.is_empty() {
                renderer.line(MessageStyle::Info, "  [Files]")?;
                for (name, _size) in &files {
                    // Simple file name display without size or emoji
                    let display = format!("  {:<width$}", name, width = max_name_width,);
                    renderer.line(MessageStyle::Response, &display)?;
                }
            }

            let omitted = items.len().saturating_sub(MAX_DISPLAYED_FILES);
            if omitted > 0 {
                renderer.line(
                    MessageStyle::Info,
                    &format!("  + {} more items not shown", omitted),
                )?;
            }
        }
    }

    // Pagination navigation removed - agent should work with first page results
    // If more items exist, agent can call list_files again with specific page parameter

    Ok(())
}

pub(crate) fn render_read_file_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Show file metadata with aligned formatting (no size information)
    if let Some(encoding) = get_string(val, "encoding") {
        renderer.line(
            MessageStyle::Info,
            &format!("  {:16} {}", "encoding", encoding),
        )?;
    }

    // Removed size display to comply with request of no file size information
    if let Some(start) = get_u64(val, "start_line")
        && let Some(end) = get_u64(val, "end_line")
    {
        renderer.line(
            MessageStyle::Info,
            &format!("  {:16} {}-{}", "lines", start, end),
        )?;
    }

    // Content is loaded and available via file tools context; no need to echo summary

    Ok(())
}

/// Render diff content lines with proper truncation and styling (compact format)
fn render_diff_content(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let total_lines = diff_content.lines().count();
    let mut added_count = 0;
    let mut removed_count = 0;
    let mut lines_to_render = Vec::new();

    // Collect lines to render in a single pass
    for (line_count, line) in diff_content.lines().enumerate() {
        if line_count >= MAX_DIFF_LINES {
            lines_to_render.push((
                true,
                format!(
                    "… ({} more lines, view full diff in logs)",
                    total_lines - MAX_DIFF_LINES
                ),
            ));
            break;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            added_count += 1;
        } else if trimmed.starts_with('-') && !trimmed.starts_with("---") {
            removed_count += 1;
        }

        let truncated = truncate_text_safe(line, MAX_DIFF_LINE_LENGTH);
        lines_to_render.push((false, truncated.to_string()));
    }

    // Render all lines compactly (minimal padding)
    for (is_truncation, content) in lines_to_render {
        if is_truncation {
            renderer.line(MessageStyle::Info, &format!("…{}", content))?;
        } else if let Some(style) =
            select_line_style(Some(tools::WRITE_FILE), &content, git_styles, ls_styles)
        {
            renderer.line_with_style(style, &content)?;
        } else {
            renderer.line(MessageStyle::Response, &content)?;
        }
    }

    // Show summary stats if we rendered changes (compact format)
    if added_count + removed_count > 0 {
        renderer.line(
            MessageStyle::Info,
            &format!("▸ +{} -{}", added_count, removed_count),
        )?;
    }

    Ok(())
}
