use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::styles::{GitStyles, LsStyles, select_line_style};

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

fn format_start_only_hunk_header(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    if !trimmed.starts_with("@@ -") {
        return None;
    }

    let rest = trimmed.strip_prefix("@@ -")?;
    let mut parts = rest.split_whitespace();
    let old_part = parts.next()?;
    let new_part = parts.next()?;

    if !new_part.starts_with('+') {
        return None;
    }

    let old_start = old_part.split(',').next()?.parse::<usize>().ok()?;
    let new_start = new_part
        .trim_start_matches('+')
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;

    Some(format!("@@ -{} +{} @@", old_start, new_start))
}

fn is_addition_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('+') && !trimmed.starts_with("+++")
}

fn is_deletion_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && !trimmed.starts_with("---")
}

fn parse_diff_git_path(line: &str) -> Option<String> {
    let mut parts = line.split_whitespace();
    if parts.next()? != "diff" {
        return None;
    }
    if parts.next()? != "--git" {
        return None;
    }
    let _old = parts.next()?;
    let new_path = parts.next()?;
    Some(new_path.trim_start_matches("b/").to_string())
}

fn parse_apply_patch_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("*** ")?;
    let (kind, path) = rest.split_once(':')?;
    let kind = kind.trim();
    if !matches!(kind, "Update File" | "Add File" | "Delete File") {
        return None;
    }
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

fn parse_diff_marker_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !(trimmed.starts_with("--- ") || trimmed.starts_with("+++ ")) {
        return None;
    }
    let path = trimmed.split_whitespace().nth(1)?;
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.trim_start_matches("a/")
            .trim_start_matches("b/")
            .to_string(),
    )
}

pub(super) fn format_diff_content_lines(diff_content: &str) -> Vec<String> {
    #[derive(Default)]
    struct DiffBlock {
        header: String,
        path: String,
        lines: Vec<String>,
        additions: usize,
        deletions: usize,
    }

    let mut preface: Vec<String> = Vec::new();
    let mut blocks: Vec<DiffBlock> = Vec::new();
    let mut current: Option<DiffBlock> = None;

    for line in diff_content.lines() {
        if let Some(path) = parse_diff_git_path(line).or_else(|| parse_apply_patch_path(line)) {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(DiffBlock {
                header: line.to_string(),
                path,
                lines: Vec::new(),
                additions: 0,
                deletions: 0,
            });
            continue;
        }

        let rewritten = format_start_only_hunk_header(line).unwrap_or_else(|| line.to_string());
        if let Some(block) = current.as_mut() {
            if is_addition_line(line) {
                block.additions += 1;
            } else if is_deletion_line(line) {
                block.deletions += 1;
            }
            block.lines.push(rewritten);
        } else {
            preface.push(rewritten);
        }
    }

    if let Some(block) = current {
        blocks.push(block);
    }

    if blocks.is_empty() {
        let mut additions = 0usize;
        let mut deletions = 0usize;
        let mut fallback_path: Option<String> = None;
        let mut summary_insert_index: Option<usize> = None;
        let mut lines: Vec<String> = Vec::new();

        for line in diff_content.lines() {
            if fallback_path.is_none() {
                fallback_path =
                    parse_diff_marker_path(line).or_else(|| parse_apply_patch_path(line));
            }
            if summary_insert_index.is_none() && line.trim_start().starts_with("+++ ") {
                summary_insert_index = Some(lines.len());
            }
            if is_addition_line(line) {
                additions += 1;
            } else if is_deletion_line(line) {
                deletions += 1;
            }
            let rewritten = format_start_only_hunk_header(line).unwrap_or_else(|| line.to_string());
            lines.push(rewritten);
        }

        let path = fallback_path.unwrap_or_else(|| "file".to_string());
        let summary = format!("• Edited {} (+{} -{})", path, additions, deletions);

        let mut output = Vec::with_capacity(lines.len() + 1);
        if let Some(idx) = summary_insert_index {
            output.extend(lines[..=idx].iter().cloned());
            output.push(summary);
            output.extend(lines[idx + 1..].iter().cloned());
        } else {
            output.push(summary);
            output.extend(lines);
        }
        return output;
    }

    let mut output = Vec::new();
    output.extend(preface);
    for block in blocks {
        output.push(block.header);
        output.push(format!(
            "• Edited {} (+{} -{})",
            block.path, block.additions, block.deletions
        ));
        output.extend(block.lines);
    }
    output
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
        renderer.line(MessageStyle::ToolDetail, "▼ diff")?;
        render_diff_content(renderer, diff_content, git_styles, ls_styles)?;
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
    // Show file metadata with aligned formatting (no size information)
    if let Some(encoding) = get_string(val, "encoding") {
        renderer.line(
            MessageStyle::ToolDetail,
            &format!("{:16} {}", "encoding", encoding),
        )?;
    }

    // Removed size display to comply with request of no file size information
    if let Some(start) = get_u64(val, "start_line")
        && let Some(end) = get_u64(val, "end_line")
    {
        renderer.line(
            MessageStyle::ToolDetail,
            &format!("{:16} {}-{}", "lines", start, end),
        )?;
    }

    if let Some(content) = get_string(val, "content")
        && looks_like_diff_content(content)
    {
        let git_styles = GitStyles::new();
        let ls_styles = LsStyles::from_env();
        renderer.line(MessageStyle::ToolDetail, "▼ diff")?;
        render_diff_content(renderer, content, &git_styles, &ls_styles)?;
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
    let indent = MessageStyle::ToolDetail.indent();

    for line in lines_to_render {
        let truncated = truncate_text_safe(&line, MAX_DIFF_LINE_LENGTH);
        if let Some(style) =
            select_line_style(Some(tools::WRITE_FILE), truncated, git_styles, ls_styles)
        {
            let mut buffer = String::with_capacity(indent.len() + truncated.len());
            buffer.push_str(indent);
            buffer.push_str(truncated);
            renderer.line_with_style(style, &buffer)?;
        } else {
            renderer.line(MessageStyle::ToolDetail, truncated)?;
        }
    }

    Ok(())
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
mod tests {
    use super::*;

    #[test]
    fn formats_unified_diff_with_summary_and_hunk_headers() {
        let diff = "\
diff --git a/file1.txt b/file1.txt
index 0000000..1111111 100644
--- a/file1.txt
+++ b/file1.txt
@@ -1,2 +1,2 @@
-old
+new
";
        let lines = format_diff_content_lines(diff);
        assert_eq!(lines[0], "diff --git a/file1.txt b/file1.txt");
        assert_eq!(lines[1], "• Edited file1.txt (+1 -1)");
        assert!(lines.iter().any(|line| line == "@@ -1 +1 @@"));
    }

    #[test]
    fn formats_diff_without_git_header_with_summary_after_plus() {
        let diff = "\
--- a/file2.txt
+++ b/file2.txt
@@ -2,3 +2,3 @@
-before
+after
";
        let lines = format_diff_content_lines(diff);
        let plus_index = lines
            .iter()
            .position(|line| line.starts_with("+++ "))
            .expect("plus header exists");
        assert_eq!(lines[plus_index + 1], "• Edited file2.txt (+1 -1)");
        assert!(lines.iter().any(|line| line == "@@ -2 +2 @@"));
    }
}
