use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::styles::{GitStyles, LsStyles, select_line_style};

pub(crate) fn render_write_file_preview(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    if let Some(encoding) = payload.get("encoding").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    if payload
        .get("created")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        renderer.line(MessageStyle::Response, "  File created")?;
    }

    let diff_value = match payload.get("diff_preview") {
        Some(value) => value,
        None => return Ok(()),
    };

    if diff_value
        .get("skipped")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        let reason = diff_value
            .get("reason")
            .and_then(|value| value.as_str())
            .unwrap_or("diff preview skipped");
        renderer.line(
            MessageStyle::Info,
            &format!("  diff preview skipped: {reason}"),
        )?;

        if let Some(detail) = diff_value.get("detail").and_then(|value| value.as_str()) {
            renderer.line(MessageStyle::Info, &format!("  detail: {detail}"))?;
        }

        if let Some(max_bytes) = diff_value.get("max_bytes").and_then(|value| value.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  preview limit: {max_bytes} bytes"),
            )?;
        }
        return Ok(());
    }

    let diff_content = diff_value
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or("");

    if diff_content.is_empty()
        && diff_value
            .get("is_empty")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    {
        renderer.line(MessageStyle::Info, "  No diff changes to display.")?;
    }

    if !diff_content.is_empty() {
        renderer.line(MessageStyle::Info, "[diff]")?;
        const MAX_DIFF_LINES: usize = 300;
        const MAX_LINE_LENGTH: usize = 200;
        let mut line_count = 0;
        let total_lines = diff_content.lines().count();

        for line in diff_content.lines() {
            if line_count >= MAX_DIFF_LINES {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "  ... ({} more lines truncated)",
                        total_lines - MAX_DIFF_LINES
                    ),
                )?;
                break;
            }

            let truncated = if line.len() > MAX_LINE_LENGTH {
                &line[..line
                    .char_indices()
                    .nth(MAX_LINE_LENGTH)
                    .map(|(i, _)| i)
                    .unwrap_or(MAX_LINE_LENGTH)]
            } else {
                line
            };
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
    }

    if diff_value
        .get("truncated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        if let Some(omitted) = diff_value
            .get("omitted_line_count")
            .and_then(|value| value.as_u64())
        {
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
    ls_styles: &LsStyles,
) -> Result<()> {
    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  {}", path))?;
    }

    if let Some(page) = val.get("page").and_then(|v| v.as_u64()) {
        if let Some(total) = val.get("total_items").and_then(|v| v.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  Page {} ({} items total)", page, total),
            )?;
        }
    }

    if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            renderer.line(MessageStyle::Info, "  (empty directory)")?;
        } else {
            for item in items {
                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("file");
                    let size = item.get("size").and_then(|v| v.as_u64());

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

                    if let Some(style) = ls_styles.style_for_line(&display_name) {
                        renderer.line_with_style(style, &display)?;
                    } else {
                        renderer.line(MessageStyle::Response, &display)?;
                    }
                }
            }
        }
    }

    if val
        .get("has_more")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        renderer.line(MessageStyle::Info, "  ... more items available")?;
    }

    Ok(())
}

pub(crate) fn render_read_file_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(encoding) = val.get("encoding").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    if let Some(size) = val.get("size").and_then(|v| v.as_u64()) {
        renderer.line(
            MessageStyle::Info,
            &format!("  size: {}", format_size(size)),
        )?;
    }

    if let Some(start) = val.get("start_line").and_then(|v| v.as_u64()) {
        if let Some(end) = val.get("end_line").and_then(|v| v.as_u64()) {
            renderer.line(MessageStyle::Info, &format!("  lines: {}-{}", start, end))?;
        }
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
