use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::panels::{PanelContentLine, render_left_border_panel};

pub(crate) fn resolve_renderer_profile(
    tool_name: &str,
    vt_config: Option<&VTCodeConfig>,
) -> Option<McpRendererProfile> {
    let config = vt_config?;
    config.mcp.ui.renderer_for_tool(tool_name)
}

pub(crate) fn render_context7_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(meta) = val.get("meta").and_then(|value| value.as_object())
        && let Some(query) = meta.get("query").and_then(|value| value.as_str())
    {
        renderer.line(MessageStyle::ToolDetail, &shorten(query, 120))?;
    }

    if let Some(messages) = val.get("messages").and_then(|value| value.as_array())
        && !messages.is_empty()
    {
        renderer.line(MessageStyle::ToolDetail, &format!("{} snippets retrieved", messages.len()))?;
    }

    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        for err in errors.iter().take(1) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::ToolError, &shorten(msg, 120))?;
            }
        }
        if errors.len() > 1 {
            renderer.line(MessageStyle::ToolError, &format!("… {} more errors", errors.len() - 1))?;
        }
    }

    Ok(())
}

pub(crate) fn render_sequential_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let summary = val
        .get("summary")
        .and_then(|value| value.as_str())
        .unwrap_or("Sequential reasoning summary unavailable");

    renderer.line(MessageStyle::ToolDetail, &shorten(summary, 120))?;

    if let Some(events) = val.get("events").and_then(|value| value.as_array())
        && !events.is_empty()
    {
        renderer.line(MessageStyle::ToolDetail, &format!("{} reasoning steps", events.len()))?;
    }

    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        for err in errors.iter().take(1) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::ToolError, &shorten(msg, 120))?;
            }
        }
        if errors.len() > 1 {
            renderer.line(MessageStyle::ToolError, &format!("… {} more errors", errors.len() - 1))?;
        }
    }

    Ok(())
}

pub(crate) fn render_generic_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let mut block_lines: Vec<PanelContentLine> = Vec::new();

    if let Some(content) = val.get("content").and_then(|v| v.as_array()) {
        for (idx, item) in content.iter().enumerate() {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                let content_number = (content.len() > 1).then_some(idx + 1);
                collect_text_content(&mut block_lines, text, content_number)?;
                continue;
            }

            let details = match item.get("type").and_then(Value::as_str) {
                Some(kind @ ("image" | "audio")) => {
                    block_lines.push(PanelContentLine::new(format!("[{kind} content]"), MessageStyle::ToolDetail));
                    item
                }
                Some(kind @ ("resource" | "resource_link")) => {
                    let resource = item.get("resource").unwrap_or(item);
                    if let Some(uri) = resource.get("uri").and_then(Value::as_str) {
                        block_lines.push(PanelContentLine::new(format!("[resource: {uri}]"), MessageStyle::ToolDetail));
                    }
                    if kind == "resource_link"
                        && let Some(name) = resource.get("name").and_then(Value::as_str)
                    {
                        block_lines.push(PanelContentLine::new(format!("name: {name}"), MessageStyle::ToolDetail));
                    }
                    resource
                }
                _ => continue,
            };
            if let Some(mime) = details.get("mimeType").and_then(Value::as_str) {
                block_lines.push(PanelContentLine::new(format!("type: {mime}"), MessageStyle::ToolDetail));
            }
            if let Some(text) = details.get("text").and_then(Value::as_str) {
                collect_text_content(&mut block_lines, text, None)?;
            }
        }
    }

    if block_lines.iter().all(|line| line.rendered.trim().is_empty())
        && let Some(structured) = val.get("structuredContent")
    {
        block_lines.clear();
        collect_formatted_json_lines(&mut block_lines, structured, &[])?;
        if block_lines.iter().all(|line| line.rendered.trim().is_empty()) {
            block_lines.clear();
            block_lines.push(PanelContentLine::new(structured.to_string(), MessageStyle::ToolDetail));
        }
    }

    if let Some(meta) = val.get("meta").and_then(|v| v.as_object())
        && !meta.is_empty()
    {
        if !block_lines.is_empty() {
            block_lines.push(PanelContentLine::new(String::new(), MessageStyle::ToolDetail));
        }
        for (key, value) in meta {
            if let Some(text) = value.as_str() {
                block_lines
                    .push(PanelContentLine::new(format!("{}: {}", key, shorten(text, 100)), MessageStyle::ToolDetail));
            } else if let Some(num) = value.as_u64() {
                block_lines.push(PanelContentLine::new(format!("{key}: {num}"), MessageStyle::ToolDetail));
            }
        }
    }

    if block_lines.is_empty() {
        return Ok(());
    }

    render_left_border_panel(renderer, block_lines)
}

fn collect_text_content(lines: &mut Vec<PanelContentLine>, text: &str, content_number: Option<usize>) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    if let Ok(json_val) = serde_json::from_str::<Value>(text) {
        if let Some(number) = content_number {
            lines.push(PanelContentLine::new(format!("[content {number}]"), MessageStyle::ToolDetail));
        }
        collect_formatted_json_lines(lines, &json_val, &["model", "_meta", "isError"])?;
    } else if text.contains("```") {
        collect_text_with_code_blocks(lines, text);
    } else {
        for line in text.lines() {
            lines.push(PanelContentLine::new(line, MessageStyle::ToolDetail));
        }
    }
    Ok(())
}

fn collect_text_with_code_blocks(lines: &mut Vec<PanelContentLine>, text: &str) {
    let mut in_code_block = false;

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                in_code_block = false;
            } else {
                in_code_block = true;
                let lang = line.trim_start().trim_start_matches("```").trim();
                if !lang.is_empty() {
                    lines.push(PanelContentLine::new(format!("[{lang}]"), MessageStyle::ToolDetail));
                }
            }
        } else {
            lines.push(PanelContentLine::new(line, MessageStyle::ToolDetail));
        }
    }
}

fn collect_formatted_json_lines(
    lines: &mut Vec<PanelContentLine>,
    json: &Value,
    excluded_fields: &[&str],
) -> Result<()> {
    match json {
        Value::Object(map) => {
            for (key, value) in map {
                if excluded_fields.contains(&key.as_str()) {
                    continue;
                }

                let entry = match value {
                    Value::String(s) => format!("{key}: {s}"),
                    Value::Number(n) => format!("{key}: {n}"),
                    Value::Bool(b) => format!("{key}: {b}"),
                    Value::Null => format!("{key}: null"),
                    Value::Array(arr) => format!("{}: [] ({} items)", key, arr.len()),
                    Value::Object(_) => format!("{key}: {{...}}"),
                };
                lines.push(PanelContentLine::new(entry, MessageStyle::ToolDetail));
            }
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                lines.push(PanelContentLine::new(
                    format!("[{}]: {}", idx, serde_json::to_string(item)?),
                    MessageStyle::ToolDetail,
                ));
            }
        }
        Value::String(s) => {
            lines.push(PanelContentLine::new(s, MessageStyle::ToolDetail));
        }
        _ => {
            lines.push(PanelContentLine::new(json.to_string(), MessageStyle::ToolDetail));
        }
    }
    Ok(())
}

fn shorten(text: &str, max_len: usize) -> String {
    vtcode_commons::formatting::truncate_within(text, max_len, vtcode_ui::design::constants::ELLIPSIS)
}
