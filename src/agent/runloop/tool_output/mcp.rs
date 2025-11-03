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
        renderer.line(MessageStyle::Info, &format!("  {}", shorten(query, 120)))?;
    }

    if let Some(messages) = val.get("messages").and_then(|value| value.as_array())
        && !messages.is_empty()
    {
        renderer.line(
            MessageStyle::Response,
            &format!("  {} snippets retrieved", messages.len()),
        )?;
    }

    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        for err in errors.iter().take(1) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::Error, &format!("  {}", shorten(msg, 120)))?;
            }
        }
        if errors.len() > 1 {
            renderer.line(
                MessageStyle::Error,
                &format!("  … {} more errors", errors.len() - 1),
            )?;
        }
    }

    Ok(())
}

pub(crate) fn render_sequential_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let summary = val
        .get("summary")
        .and_then(|value| value.as_str())
        .unwrap_or("Sequential reasoning summary unavailable");

    renderer.line(MessageStyle::Info, &format!("  {}", shorten(summary, 120)))?;

    if let Some(events) = val.get("events").and_then(|value| value.as_array())
        && !events.is_empty()
    {
        renderer.line(
            MessageStyle::Response,
            &format!("  {} reasoning steps", events.len()),
        )?;
    }

    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        for err in errors.iter().take(1) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::Error, &format!("  {}", shorten(msg, 120)))?;
            }
        }
        if errors.len() > 1 {
            renderer.line(
                MessageStyle::Error,
                &format!("  … {} more errors", errors.len() - 1),
            )?;
        }
    }

    Ok(())
}

pub(crate) fn render_generic_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let mut block_lines: Vec<PanelContentLine> = Vec::new();

    if let Some(content) = val.get("content").and_then(|v| v.as_array()) {
        for (idx, item) in content.iter().enumerate() {
            let mut render_text_content = |text: &str| -> Result<()> {
                if text.trim().is_empty() {
                    return Ok(());
                }
                if let Ok(json_val) = serde_json::from_str::<Value>(text) {
                    if content.len() > 1 {
                        block_lines.push(PanelContentLine::new(
                            format!("  [content {}]", idx + 1),
                            MessageStyle::Info,
                        ));
                    }
                    collect_formatted_json_lines(&mut block_lines, &json_val)?;
                } else if text.contains("```") {
                    collect_text_with_code_blocks(&mut block_lines, text);
                } else {
                    for line in text.lines() {
                        block_lines.push(PanelContentLine::new(line, MessageStyle::Response));
                    }
                }
                Ok(())
            };

            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                render_text_content(text)?;
            } else if let Some(text) = item.get("type").and_then(|t| {
                if t.as_str() == Some("text") {
                    item.get("text").and_then(|v| v.as_str())
                } else {
                    None
                }
            }) {
                render_text_content(text)?;
            } else if item.get("type").and_then(|t| t.as_str()) == Some("image") {
                block_lines.push(PanelContentLine::new(
                    "  [image content]",
                    MessageStyle::Info,
                ));
                if let Some(mime) = item.get("mimeType").and_then(|v| v.as_str()) {
                    block_lines.push(PanelContentLine::new(
                        format!("    type: {}", mime),
                        MessageStyle::Info,
                    ));
                }
            } else if item.get("type").and_then(|t| t.as_str()) == Some("resource")
                && let Some(uri) = item.get("uri").and_then(|v| v.as_str())
            {
                block_lines.push(PanelContentLine::new(
                    format!("  [resource: {}]", uri),
                    MessageStyle::Info,
                ));
            }
        }
    }

    if let Some(meta) = val.get("meta").and_then(|v| v.as_object())
        && !meta.is_empty()
    {
        if !block_lines.is_empty() {
            block_lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
        }
        for (key, value) in meta {
            if let Some(text) = value.as_str() {
                block_lines.push(PanelContentLine::new(
                    format!("  {}: {}", key, shorten(text, 100)),
                    MessageStyle::Info,
                ));
            } else if let Some(num) = value.as_u64() {
                block_lines.push(PanelContentLine::new(
                    format!("  {}: {}", key, num),
                    MessageStyle::Info,
                ));
            }
        }
    }

    if block_lines.is_empty() {
        return Ok(());
    }

    render_left_border_panel(renderer, block_lines)
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
                    lines.push(PanelContentLine::new(
                        format!("  [{}]", lang),
                        MessageStyle::Info,
                    ));
                }
            }
        } else if in_code_block {
            lines.push(PanelContentLine::new(
                format!("  {}", line),
                MessageStyle::Response,
            ));
        } else {
            lines.push(PanelContentLine::new(line, MessageStyle::Response));
        }
    }
}

fn collect_formatted_json_lines(lines: &mut Vec<PanelContentLine>, json: &Value) -> Result<()> {
    const SKIP_FIELDS: &[&str] = &["model", "_meta", "isError"];

    match json {
        Value::Object(map) => {
            for (key, value) in map {
                if SKIP_FIELDS.contains(&key.as_str()) {
                    continue;
                }

                let entry = match value {
                    Value::String(s) => format!("  {}: {}", key, s),
                    Value::Number(n) => format!("  {}: {}", key, n),
                    Value::Bool(b) => format!("  {}: {}", key, b),
                    Value::Null => format!("  {}: null", key),
                    Value::Array(arr) => format!("  {}: [] ({} items)", key, arr.len()),
                    Value::Object(_) => format!("  {}: {{...}}", key),
                };
                lines.push(PanelContentLine::new(entry, MessageStyle::Response));
            }
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                lines.push(PanelContentLine::new(
                    format!("  [{}]: {}", idx, serde_json::to_string(item)?),
                    MessageStyle::Response,
                ));
            }
        }
        Value::String(s) => {
            lines.push(PanelContentLine::new(s, MessageStyle::Response));
        }
        _ => {
            lines.push(PanelContentLine::new(
                json.to_string(),
                MessageStyle::Response,
            ));
        }
    }
    Ok(())
}

fn shorten(text: &str, max_len: usize) -> String {
    const ELLIPSIS: &str = "…";
    if text.chars().count() <= max_len {
        return text.to_string();
    }

    let mut result = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx + ELLIPSIS.len() >= max_len {
            result.push_str(ELLIPSIS);
            break;
        }
        result.push(ch);
    }
    result
}
