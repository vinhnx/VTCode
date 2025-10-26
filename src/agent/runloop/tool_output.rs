use anstyle::{Ansi256Color, AnsiColor, Color, Effects, RgbColor, Style as AnsiStyle};
use anyhow::{Context, Result};
use serde_json::Value;
use shell_words::split as shell_split;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::{defaults, tools};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::tools::{PlanCompletionState, StepStatus, TaskPlan};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color as RatColor, Modifier as RatModifier, Style as RatStyle};
use ratatui::widgets::{Block, BorderType, Padding, Widget};
use unicode_width::UnicodeWidthStr;

const INLINE_STREAM_MAX_LINES: usize = 30;

use crate::agent::runloop::text_tools::CodeFenceBlock;

struct PanelContentLine {
    text: String,
    style: MessageStyle,
}

impl PanelContentLine {
    fn new(text: impl Into<String>, style: MessageStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }
}

struct ToolPanel {
    title: Option<String>,
    lines: Vec<String>,
    border_style: RatStyle,
}

impl ToolPanel {
    fn new(title: Option<String>, lines: Vec<String>, border_style: RatStyle) -> Self {
        Self {
            title,
            lines,
            border_style,
        }
    }
}

impl Widget for ToolPanel {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut block = Block::bordered()
            .border_style(self.border_style)
            .border_type(BorderType::Rounded)
            .padding(Padding::new(1, 1, 0, 0));
        if let Some(title) = self.title {
            block = block.title(title);
        }
        let inner = block.inner(area);
        block.render(area, buf);
        let text_style = RatStyle::default();
        for (index, line) in self.lines.into_iter().enumerate() {
            if inner.height <= index as u16 {
                break;
            }
            buf.set_string(inner.left(), inner.top() + index as u16, line, text_style);
        }
    }
}

fn render_widget_lines<W: Widget>(widget: W, width: u16, height: u16) -> Vec<String> {
    let area = Rect::new(0, 0, width.max(1), height.max(1));
    let mut buffer = Buffer::empty(area);
    widget.render(area, &mut buffer);
    let mut lines = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        while line.ends_with(' ') {
            line.pop();
        }
        lines.push(line);
    }
    lines
}

fn convert_color(color: Color) -> Option<RatColor> {
    match color {
        Color::Ansi(ansi) => Some(match ansi {
            AnsiColor::Black => RatColor::Black,
            AnsiColor::Red => RatColor::Red,
            AnsiColor::Green => RatColor::Green,
            AnsiColor::Yellow => RatColor::Yellow,
            AnsiColor::Blue => RatColor::Blue,
            AnsiColor::Magenta => RatColor::Magenta,
            AnsiColor::Cyan => RatColor::Cyan,
            AnsiColor::White => RatColor::White,
            AnsiColor::BrightBlack => RatColor::DarkGray,
            AnsiColor::BrightRed => RatColor::LightRed,
            AnsiColor::BrightGreen => RatColor::LightGreen,
            AnsiColor::BrightYellow => RatColor::LightYellow,
            AnsiColor::BrightBlue => RatColor::LightBlue,
            AnsiColor::BrightMagenta => RatColor::LightMagenta,
            AnsiColor::BrightCyan => RatColor::LightCyan,
            AnsiColor::BrightWhite => RatColor::Gray,
        }),
        Color::Ansi256(Ansi256Color(value)) => Some(RatColor::Indexed(value)),
        Color::Rgb(RgbColor(r, g, b)) => Some(RatColor::Rgb(r, g, b)),
    }
}

fn ratatui_style_from_ansi(style: AnsiStyle) -> RatStyle {
    let mut resolved = RatStyle::default();
    if let Some(color) = style.get_fg_color().and_then(convert_color) {
        resolved = resolved.fg(color);
    }
    if let Some(color) = style.get_bg_color().and_then(convert_color) {
        resolved = resolved.bg(color);
    }
    let effects = style.get_effects();
    if effects.contains(Effects::BOLD) {
        resolved = resolved.add_modifier(RatModifier::BOLD);
    }
    if effects.contains(Effects::DIMMED) {
        resolved = resolved.add_modifier(RatModifier::DIM);
    }
    if effects.contains(Effects::ITALIC) {
        resolved = resolved.add_modifier(RatModifier::ITALIC);
    }
    if effects.contains(Effects::UNDERLINE)
        || effects.contains(Effects::DOUBLE_UNDERLINE)
        || effects.contains(Effects::CURLY_UNDERLINE)
        || effects.contains(Effects::DOTTED_UNDERLINE)
        || effects.contains(Effects::DASHED_UNDERLINE)
    {
        resolved = resolved.add_modifier(RatModifier::UNDERLINED);
    }
    if effects.contains(Effects::BLINK) {
        resolved = resolved.add_modifier(RatModifier::SLOW_BLINK);
    }
    if effects.contains(Effects::INVERT) {
        resolved = resolved.add_modifier(RatModifier::REVERSED);
    }
    if effects.contains(Effects::HIDDEN) {
        resolved = resolved.add_modifier(RatModifier::HIDDEN);
    }
    if effects.contains(Effects::STRIKETHROUGH) {
        resolved = resolved.add_modifier(RatModifier::CROSSED_OUT);
    }
    resolved
}

fn ratatui_style_from_message(style: MessageStyle) -> RatStyle {
    ratatui_style_from_ansi(style.style())
}

fn compute_panel_dimensions(
    lines: &[PanelContentLine],
    min_width: u16,
    max_width: u16,
) -> (u16, u16) {
    let max_line_width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.text.as_str()) as u16)
        .max()
        .unwrap_or(0);
    let inner_width = max_line_width.saturating_add(2);
    let width = inner_width.saturating_add(2).clamp(min_width, max_width);
    let height = (lines.len() as u16).saturating_add(2).max(3);
    (width, height)
}

fn render_panel(
    renderer: &mut AnsiRenderer,
    title: Option<String>,
    lines: Vec<PanelContentLine>,
    border_style: MessageStyle,
    min_width: u16,
    max_width: u16,
) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    let (width, height) = compute_panel_dimensions(&lines, min_width, max_width);
    let text_lines: Vec<String> = lines.iter().map(|line| line.text.clone()).collect();
    let widget = ToolPanel::new(title, text_lines, ratatui_style_from_message(border_style));
    let rendered = render_widget_lines(widget, width, height);

    for (index, line) in rendered.into_iter().enumerate() {
        let style = if index == 0 || index + 1 == height as usize {
            border_style
        } else {
            lines
                .get(index - 1)
                .map(|line| line.style)
                .unwrap_or(border_style)
        };
        renderer.line(style, line.trim_end())?;
    }

    Ok(())
}

fn clamp_panel_text(text: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index + 1 >= limit {
            truncated.push('…');
            break;
        }
        truncated.push(ch);
    }
    truncated
}

pub(crate) fn render_tool_output(
    renderer: &mut AnsiRenderer,
    tool_name: Option<&str>,
    val: &Value,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let allow_tool_ansi = vt_config.map(|cfg| cfg.ui.allow_tool_ansi).unwrap_or(false);

    // Handle special tools first (they have their own enhanced display)
    match tool_name {
        Some(tools::UPDATE_PLAN) => return render_plan_update(renderer, val),
        Some(tools::WRITE_FILE) | Some(tools::CREATE_FILE) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
        }
        Some(tools::GIT_DIFF) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            let output_mode = vt_config
                .map(|cfg| cfg.ui.tool_output_mode)
                .unwrap_or(ToolOutputMode::Compact);
            let tail_limit = resolve_stdout_tail_limit(vt_config);
            return render_git_diff(
                renderer,
                val,
                output_mode,
                tail_limit,
                &git_styles,
                &ls_styles,
                allow_tool_ansi,
                vt_config,
            );
        }
        Some(tools::RUN_TERMINAL_CMD) | Some(tools::BASH) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_terminal_command_panel(
                renderer,
                val,
                &git_styles,
                &ls_styles,
                vt_config,
                allow_tool_ansi,
            );
        }
        Some(tools::CURL) => {
            let output_mode = vt_config
                .map(|cfg| cfg.ui.tool_output_mode)
                .unwrap_or(ToolOutputMode::Compact);
            let tail_limit = resolve_stdout_tail_limit(vt_config);
            return render_curl_result(
                renderer,
                val,
                output_mode,
                tail_limit,
                allow_tool_ansi,
                vt_config,
            );
        }
        Some(tools::LIST_FILES) => {
            let ls_styles = LsStyles::from_env();
            return render_list_dir_output(renderer, val, &ls_styles);
        }
        Some(tools::READ_FILE) => {
            return render_read_file_output(renderer, val);
        }
        _ => {}
    }

    // For other tools, render a simple status header
    render_simple_tool_status(renderer, tool_name, val)?;

    // Render security notice if present
    if let Some(notice) = val.get("security_notice").and_then(Value::as_str) {
        renderer.line(MessageStyle::Info, notice)?;
    }

    // Handle MCP tools
    if let Some(tool) = tool_name
        && tool.starts_with("mcp_")
    {
        if let Some(profile) = resolve_mcp_renderer_profile(tool, vt_config) {
            match profile {
                McpRendererProfile::Context7 => render_mcp_context7_output(renderer, val)?,
                McpRendererProfile::SequentialThinking => {
                    render_mcp_sequential_output(renderer, val)?
                }
            }
        } else {
            // Generic MCP tool - render content field
            render_generic_mcp_output(renderer, val)?;
        }
    }

    // Render stdout/stderr
    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);
    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();

    if let Some(stdout) = val.get("stdout").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stdout",
            stdout,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::Response,
            allow_tool_ansi,
            vt_config,
        )?;
    }
    if let Some(stderr) = val.get("stderr").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stderr",
            stderr,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::Error,
            allow_tool_ansi,
            vt_config,
        )?;
    }
    Ok(())
}

fn render_simple_tool_status(
    renderer: &mut AnsiRenderer,
    _tool_name: Option<&str>,
    val: &Value,
) -> Result<()> {
    // Status is now rendered in the tool summary line
    // Only render error details if present
    let has_error = val.get("error").is_some() || val.get("error_type").is_some();

    if has_error {
        render_error_details(renderer, val)?;
    }

    Ok(())
}

fn render_error_details(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Render error message with better formatting
    if let Some(error_msg) = val.get("message").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Error, &format!("  Error: {}", error_msg))?;
    }

    // Render error type
    if let Some(error_type) = val.get("error_type").and_then(|v| v.as_str()) {
        let type_description = match error_type {
            "InvalidParameters" => "Invalid parameters provided",
            "ToolNotFound" => "Tool not found",
            "ResourceNotFound" => "Resource not found",
            "PermissionDenied" => "Permission denied",
            "ExecutionError" => "Execution error",
            "PolicyViolation" => "Policy violation",
            "Timeout" => "Operation timed out",
            "NetworkError" => "Network error",
            "EncodingError" => "Encoding error",
            "FileSystemError" => "File system error",
            _ => error_type,
        };
        renderer.line(MessageStyle::Info, &format!("  Type: {}", type_description))?;
    }

    // Render original error details if available
    if let Some(original) = val.get("original_error").and_then(|v| v.as_str()) {
        if !original.trim().is_empty() {
            // Truncate very long error messages
            let display_error = if original.len() > 200 {
                format!("{}...", &original[..197])
            } else {
                original.to_string()
            };
            renderer.line(MessageStyle::Info, &format!("  Details: {}", display_error))?;
        }
    }

    // Render file path if error is file-related
    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  Path: {}", path))?;
    }

    // Render line/column info if available
    if let Some(line) = val.get("line").and_then(|v| v.as_u64()) {
        if let Some(col) = val.get("column").and_then(|v| v.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  Location: line {}, column {}", line, col),
            )?;
        } else {
            renderer.line(MessageStyle::Info, &format!("  Location: line {}", line))?;
        }
    }

    // Render recovery suggestions if available
    if let Some(suggestions) = val.get("recovery_suggestions").and_then(|v| v.as_array()) {
        if !suggestions.is_empty() {
            renderer.line(MessageStyle::Info, "")?;
            renderer.line(MessageStyle::Info, "  Suggestions:")?;
            for (idx, suggestion) in suggestions.iter().take(5).enumerate() {
                if let Some(text) = suggestion.as_str() {
                    renderer.line(MessageStyle::Info, &format!("    {}. {}", idx + 1, text))?;
                }
            }
            if suggestions.len() > 5 {
                renderer.line(
                    MessageStyle::Info,
                    &format!("    ... and {} more", suggestions.len() - 5),
                )?;
            }
        }
    }

    Ok(())
}

fn render_plan_update(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(error) = val.get("error") {
        renderer.line(MessageStyle::Info, "[plan] Update failed")?;
        render_plan_error(renderer, error)?;
        return Ok(());
    }

    let plan_value = match val.get("plan").cloned() {
        Some(value) => value,
        None => {
            renderer.line(MessageStyle::Error, "[plan] No plan data returned")?;
            return Ok(());
        }
    };

    let plan: TaskPlan =
        serde_json::from_value(plan_value).context("Plan tool returned malformed plan payload")?;

    let heading = val
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Plan updated");

    renderer.line(MessageStyle::Info, &format!("[plan] {}", heading))?;

    if matches!(plan.summary.status, PlanCompletionState::Empty) {
        renderer.line(MessageStyle::Info, "  No tasks defined")?;
        return Ok(());
    }

    render_plan_panel(renderer, &plan)?;
    Ok(())
}

fn render_mcp_context7_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Status is now rendered in the tool summary line, so we skip it here

    // Show query if present
    if let Some(meta) = val.get("meta").and_then(|value| value.as_object()) {
        if let Some(query) = meta.get("query").and_then(|value| value.as_str()) {
            renderer.line(MessageStyle::Info, &format!("  {}", shorten(query, 120)))?;
        }
    }

    // Show snippet count
    if let Some(messages) = val.get("messages").and_then(|value| value.as_array())
        && !messages.is_empty()
    {
        renderer.line(
            MessageStyle::Response,
            &format!("  {} snippets retrieved", messages.len()),
        )?;
    }

    // Show errors if any
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

fn render_mcp_sequential_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let summary = val
        .get("summary")
        .and_then(|value| value.as_str())
        .unwrap_or("Sequential reasoning summary unavailable");

    // Status is now rendered in the tool summary line, so we skip it here

    renderer.line(MessageStyle::Info, &format!("  {}", shorten(summary, 120)))?;

    // Show event count if present
    if let Some(events) = val.get("events").and_then(|value| value.as_array())
        && !events.is_empty()
    {
        renderer.line(
            MessageStyle::Response,
            &format!("  {} reasoning steps", events.len()),
        )?;
    }

    // Show errors if any
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

fn render_generic_mcp_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Render MCP content field
    if let Some(content) = val.get("content").and_then(|v| v.as_array()) {
        for (idx, item) in content.iter().enumerate() {
            // Handle text content
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                if !text.trim().is_empty() {
                    // Try to parse as JSON and format nicely
                    if let Ok(json_val) = serde_json::from_str::<Value>(text) {
                        if content.len() > 1 {
                            renderer
                                .line(MessageStyle::Info, &format!("  [content {}]", idx + 1))?;
                        }
                        render_formatted_json(renderer, &json_val)?;
                    } else {
                        // Plain text - check for markdown code blocks
                        if text.contains("```") {
                            render_text_with_code_blocks(renderer, text)?;
                        } else {
                            // Regular text output
                            for line in text.lines() {
                                renderer.line(MessageStyle::Response, line)?;
                            }
                        }
                    }
                }
            }
            // Handle type/text structure
            else if let Some(text) = item.get("type").and_then(|t| {
                if t.as_str() == Some("text") {
                    item.get("text").and_then(|v| v.as_str())
                } else {
                    None
                }
            }) {
                if !text.trim().is_empty() {
                    // Try to parse as JSON and format nicely
                    if let Ok(json_val) = serde_json::from_str::<Value>(text) {
                        if content.len() > 1 {
                            renderer
                                .line(MessageStyle::Info, &format!("  [content {}]", idx + 1))?;
                        }
                        render_formatted_json(renderer, &json_val)?;
                    } else {
                        // Plain text - check for markdown code blocks
                        if text.contains("```") {
                            render_text_with_code_blocks(renderer, text)?;
                        } else {
                            // Regular text output
                            for line in text.lines() {
                                renderer.line(MessageStyle::Response, line)?;
                            }
                        }
                    }
                }
            }
            // Handle image content
            else if item.get("type").and_then(|t| t.as_str()) == Some("image") {
                renderer.line(MessageStyle::Info, "  [image content]")?;
                if let Some(mime) = item.get("mimeType").and_then(|v| v.as_str()) {
                    renderer.line(MessageStyle::Info, &format!("    type: {}", mime))?;
                }
            }
            // Handle resource content
            else if item.get("type").and_then(|t| t.as_str()) == Some("resource") {
                if let Some(uri) = item.get("uri").and_then(|v| v.as_str()) {
                    renderer.line(MessageStyle::Info, &format!("  [resource: {}]", uri))?;
                }
            }
        }
    }

    // Render meta field if present and interesting
    if let Some(meta) = val.get("meta").and_then(|v| v.as_object()) {
        if !meta.is_empty() {
            renderer.line(MessageStyle::Info, "")?;
            for (key, value) in meta {
                if let Some(text) = value.as_str() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("  {}: {}", key, shorten(text, 100)),
                    )?;
                } else if let Some(num) = value.as_u64() {
                    renderer.line(MessageStyle::Info, &format!("  {}: {}", key, num))?;
                }
            }
        }
    }

    Ok(())
}

fn render_text_with_code_blocks(renderer: &mut AnsiRenderer, text: &str) -> Result<()> {
    let mut in_code_block = false;

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_code_block {
                // End of code block
                in_code_block = false;
            } else {
                // Start of code block
                in_code_block = true;
                let lang = line.trim_start().trim_start_matches("```").trim();
                if !lang.is_empty() {
                    renderer.line(MessageStyle::Info, &format!("  [{}]", lang))?;
                }
            }
        } else if in_code_block {
            // Inside code block - use syntax highlighting if possible
            renderer.line(MessageStyle::Response, &format!("  {}", line))?;
        } else {
            // Regular text
            renderer.line(MessageStyle::Response, line)?;
        }
    }

    Ok(())
}

fn render_formatted_json(renderer: &mut AnsiRenderer, json: &Value) -> Result<()> {
    // Fields to skip rendering (internal/meta fields that aren't useful to display)
    const SKIP_FIELDS: &[&str] = &["model", "_meta", "isError"];

    match json {
        Value::Object(map) => {
            for (key, value) in map {
                // Skip internal/meta fields
                if SKIP_FIELDS.contains(&key.as_str()) {
                    continue;
                }

                match value {
                    Value::String(s) => {
                        renderer.line(
                            MessageStyle::Response,
                            &format!("  \x1b[36m{}\x1b[0m: {}", key, s),
                        )?;
                    }
                    Value::Number(n) => {
                        renderer.line(
                            MessageStyle::Response,
                            &format!("  \x1b[36m{}\x1b[0m: {}", key, n),
                        )?;
                    }
                    Value::Bool(b) => {
                        renderer.line(
                            MessageStyle::Response,
                            &format!("  \x1b[36m{}\x1b[0m: {}", key, b),
                        )?;
                    }
                    Value::Null => {
                        renderer.line(
                            MessageStyle::Response,
                            &format!("  \x1b[36m{}\x1b[0m: null", key),
                        )?;
                    }
                    Value::Array(arr) => {
                        renderer.line(
                            MessageStyle::Response,
                            &format!("  \x1b[36m{}\x1b[0m: [{}]", key, arr.len()),
                        )?;
                    }
                    Value::Object(_) => {
                        renderer.line(
                            MessageStyle::Response,
                            &format!("  \x1b[36m{}\x1b[0m: {{...}}", key),
                        )?;
                    }
                }
            }
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter().enumerate() {
                renderer.line(
                    MessageStyle::Response,
                    &format!("  [{}]: {}", idx, serde_json::to_string(item)?),
                )?;
            }
        }
        Value::String(s) => {
            renderer.line(MessageStyle::Response, s)?;
        }
        _ => {
            renderer.line(MessageStyle::Response, &json.to_string())?;
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

fn resolve_mcp_renderer_profile(
    tool_name: &str,
    vt_config: Option<&VTCodeConfig>,
) -> Option<McpRendererProfile> {
    let config = vt_config?;
    config.mcp.ui.renderer_for_tool(tool_name)
}

fn render_plan_panel(renderer: &mut AnsiRenderer, plan: &TaskPlan) -> Result<()> {
    const PANEL_WIDTH: u16 = 60;
    let content_width = PANEL_WIDTH.saturating_sub(4) as usize;

    let mut lines = Vec::new();
    let progress = format!(
        "  Progress: {}/{} completed",
        plan.summary.completed_steps, plan.summary.total_steps
    );
    lines.push(PanelContentLine::new(
        clamp_panel_text(&progress, content_width),
        MessageStyle::Info,
    ));

    let explanation_line = plan
        .explanation
        .as_ref()
        .and_then(|text| text.lines().next())
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| clamp_panel_text(line, content_width));

    if explanation_line.is_some() || !plan.steps.is_empty() {
        lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
    }

    if let Some(line) = explanation_line {
        lines.push(PanelContentLine::new(line, MessageStyle::Info));
        if !plan.steps.is_empty() {
            lines.push(PanelContentLine::new(String::new(), MessageStyle::Info));
        }
    }

    for (index, step) in plan.steps.iter().enumerate() {
        let (checkbox, _style) = match step.status {
            StepStatus::Pending => ("[ ]", MessageStyle::Info),
            StepStatus::InProgress => ("[>]", MessageStyle::Tool),
            StepStatus::Completed => ("[x]", MessageStyle::Response),
        };
        let step_text = step.step.trim();
        let step_number = index + 1;
        let content = format!("{step_number}. {checkbox} {step_text}");
        let truncated = clamp_panel_text(&content, content_width);
        lines.push(PanelContentLine::new(truncated, MessageStyle::Info));
    }

    render_panel(
        renderer,
        None,
        lines,
        MessageStyle::Info,
        PANEL_WIDTH,
        PANEL_WIDTH,
    )
}

fn render_plan_error(renderer: &mut AnsiRenderer, error: &Value) -> Result<()> {
    let error_message = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Plan update failed due to an unknown error.");
    let error_type = error
        .get("error_type")
        .and_then(|value| value.as_str())
        .unwrap_or("Unknown");

    renderer.line(
        MessageStyle::Error,
        &format!("  {} ({})", error_message, error_type),
    )?;

    if let Some(original_error) = error
        .get("original_error")
        .and_then(|value| value.as_str())
        .filter(|message| !message.is_empty())
    {
        renderer.line(
            MessageStyle::Info,
            &format!("  Details: {}", original_error),
        )?;
    }

    if let Some(suggestions) = error
        .get("recovery_suggestions")
        .and_then(|value| value.as_array())
    {
        let tips: Vec<_> = suggestions
            .iter()
            .filter_map(|suggestion| suggestion.as_str())
            .collect();
        if !tips.is_empty() {
            renderer.line(MessageStyle::Info, "  Recovery suggestions:")?;
            for tip in tips {
                renderer.line(MessageStyle::Info, &format!("    - {}", tip))?;
            }
        }
    }

    Ok(())
}

/// Resolves the tail limit for tool output from config.
/// Prefers ui.tool_output_max_lines, falls back to pty.stdout_tail_lines for backward compatibility.
fn resolve_stdout_tail_limit(config: Option<&VTCodeConfig>) -> usize {
    config
        .map(|cfg| {
            // Prefer the new unified tool_output_max_lines setting
            if cfg.ui.tool_output_max_lines > 0 {
                cfg.ui.tool_output_max_lines
            } else {
                // Fall back to PTY-specific setting for backward compatibility
                cfg.pty.stdout_tail_lines
            }
        })
        .filter(|&lines| lines > 0)
        .unwrap_or(defaults::DEFAULT_PTY_STDOUT_TAIL_LINES)
}

/// Spools oversized tool output to disk and returns the log path.
/// Returns None if spooling is disabled or the content is below the threshold.
fn spool_output_if_needed(
    content: &str,
    tool_name: &str,
    config: Option<&VTCodeConfig>,
) -> Result<Option<PathBuf>> {
    let threshold = config
        .map(|cfg| cfg.ui.tool_output_spool_bytes)
        .unwrap_or(200_000);

    if content.len() < threshold {
        return Ok(None);
    }

    // Determine spool directory
    let spool_dir = config
        .and_then(|cfg| cfg.ui.tool_output_spool_dir.as_deref())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".vtcode/tool-output"));

    // Create directory if it doesn't exist
    fs::create_dir_all(&spool_dir)
        .with_context(|| format!("Failed to create spool directory: {}", spool_dir.display()))?;

    // Generate unique filename with timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let filename = format!("{}-{}.log", tool_name.replace('/', "-"), timestamp);
    let log_path = spool_dir.join(filename);

    // Write content to file
    let mut file = fs::File::create(&log_path)
        .with_context(|| format!("Failed to create spool file: {}", log_path.display()))?;
    file.write_all(content.as_bytes())
        .with_context(|| format!("Failed to write to spool file: {}", log_path.display()))?;

    Ok(Some(log_path))
}

/// Streaming tail iterator that extracts the last N lines without buffering all lines.
/// Uses SmallVec for stack allocation when tail is small (≤32 lines).
/// Optimized to use a single pass with modulo indexing instead of VecDeque.
#[cfg_attr(
    feature = "profiling",
    tracing::instrument(skip(text), level = "trace")
)]
fn tail_lines_streaming<'a>(text: &'a str, limit: usize) -> (SmallVec<[&'a str; 32]>, usize) {
    if text.is_empty() {
        return (SmallVec::new(), 0);
    }
    if limit == 0 {
        return (SmallVec::new(), text.lines().count());
    }

    // Use a fixed-size buffer with modulo indexing to avoid VecDeque overhead
    let mut buffer: SmallVec<[&'a str; 32]> = SmallVec::with_capacity(limit);
    let mut total = 0usize;
    let mut write_idx = 0usize;

    for line in text.lines() {
        if buffer.len() < limit {
            buffer.push(line);
        } else {
            // Circular buffer: overwrite oldest entry
            buffer[write_idx] = line;
            write_idx = (write_idx + 1) % limit;
        }
        total += 1;
    }

    // If we wrapped around, rotate to get correct order
    if total > limit {
        buffer.rotate_left(write_idx);
    }

    (buffer, total)
}

/// Legacy wrapper for backward compatibility (used in tests).
#[inline]
#[cfg(test)]
fn tail_lines(text: &str, limit: usize) -> (Vec<&str>, usize) {
    let (tail, total) = tail_lines_streaming(text, limit);
    (tail.into_vec(), total)
}

/// Streaming line selection that avoids buffering all lines when possible.
/// Returns SmallVec for efficient stack allocation on small outputs.
/// In Full mode, still uses tail_limit as a safety cap to prevent unbounded memory growth.
#[cfg_attr(
    feature = "profiling",
    tracing::instrument(skip(content), level = "trace")
)]
fn select_stream_lines_streaming<'a>(
    content: &'a str,
    mode: ToolOutputMode,
    tail_limit: usize,
    prefer_full: bool,
) -> (SmallVec<[&'a str; 32]>, usize, bool) {
    if content.is_empty() {
        return (SmallVec::new(), 0, false);
    }

    // Even in Full mode, use tail_limit as a safety cap to prevent unbounded memory
    // The caller (render_stream_section) will further cap at INLINE_STREAM_MAX_LINES if needed
    let effective_limit = if prefer_full || matches!(mode, ToolOutputMode::Full) {
        tail_limit.max(1000) // Use at least 1000 lines in full mode, but respect higher limits
    } else {
        tail_limit
    };

    let (tail, total) = tail_lines_streaming(content, effective_limit);
    let truncated = total > tail.len();
    (tail, total, truncated)
}

/// Legacy wrapper for backward compatibility (used in tests).
#[inline]
#[cfg(test)]
fn select_stream_lines(
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    prefer_full: bool,
) -> (Vec<&str>, usize, bool) {
    let (lines, total, truncated) =
        select_stream_lines_streaming(content, mode, tail_limit, prefer_full);
    (lines.into_vec(), total, truncated)
}

fn render_write_file_preview(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    // Status is now rendered in the tool summary line, so we skip it here

    // Show encoding if specified
    if let Some(encoding) = payload.get("encoding").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    // Show file creation status
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
        for line in diff_content.lines() {
            let display = format!("  {line}");
            if let Some(style) =
                select_line_style(Some(tools::WRITE_FILE), line, git_styles, ls_styles)
            {
                renderer.line_with_style(style, &display)?;
            } else {
                renderer.line(MessageStyle::Response, &display)?;
            }
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

#[cfg_attr(
    feature = "profiling",
    tracing::instrument(
        skip(renderer, content, git_styles, ls_styles, config),
        level = "debug"
    )
)]
fn render_stream_section(
    renderer: &mut AnsiRenderer,
    title: &str,
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    fallback_style: MessageStyle,
    allow_ansi: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    let is_mcp_tool = tool_name.map_or(false, |name| name.starts_with("mcp_"));
    let force_tail_mode = matches!(tool_name, Some(tools::RUN_TERMINAL_CMD) | Some(tools::BASH));
    let normalized_content = if allow_ansi {
        Cow::Borrowed(content)
    } else {
        strip_ansi_codes(content)
    };

    // Check if we should spool to disk
    if let Some(tool) = tool_name {
        if let Ok(Some(log_path)) =
            spool_output_if_needed(normalized_content.as_ref(), tool, config)
        {
            use std::fmt::Write as _;
            // Content was spooled, show a message with tail preview using streaming
            let (tail, total) = tail_lines_streaming(normalized_content.as_ref(), 20);

            // Reuse buffer for spool message
            let mut msg_buffer = String::with_capacity(256);
            let _ = write!(
                &mut msg_buffer,
                "[{}] Output too large ({} bytes, {} lines), spooled to: {}",
                title.to_ascii_uppercase(),
                content.len(),
                total,
                log_path.display()
            );
            renderer.line(MessageStyle::Info, &msg_buffer)?;
            renderer.line(MessageStyle::Info, "Last 20 lines:")?;

            msg_buffer.clear();
            msg_buffer.reserve(128);
            let prefix = if is_mcp_tool { "" } else { "  " };

            for line in &tail {
                if line.is_empty() {
                    msg_buffer.clear();
                } else {
                    msg_buffer.clear();
                    msg_buffer.push_str(prefix);
                    msg_buffer.push_str(line);
                }
                if let Some(style) = select_line_style(tool_name, line, git_styles, ls_styles) {
                    renderer.line_with_style(style, &msg_buffer)?;
                } else {
                    renderer.line(fallback_style, &msg_buffer)?;
                }
            }
            return Ok(());
        }
    }

    let prefer_full = renderer.prefers_untruncated_output() && !force_tail_mode;
    let (lines_vec, total, mut truncated) =
        select_stream_lines_streaming(normalized_content.as_ref(), mode, tail_limit, prefer_full);

    // Convert to Vec only if we need to re-tail for INLINE_STREAM_MAX_LINES
    let mut lines_vec = lines_vec;
    if prefer_full && lines_vec.len() > INLINE_STREAM_MAX_LINES {
        let (tail, _) = tail_lines_streaming(normalized_content.as_ref(), INLINE_STREAM_MAX_LINES);
        lines_vec = tail;
        truncated = true;
    }

    if lines_vec.is_empty() {
        return Ok(());
    }

    // Reuse buffer for formatting to avoid allocations
    let mut format_buffer = String::with_capacity(64);

    if truncated {
        let hidden = total.saturating_sub(lines_vec.len());
        if hidden > 0 {
            use std::fmt::Write as _;
            let prefix = if is_mcp_tool { "" } else { "  " };
            format_buffer.clear();
            let _ = write!(
                &mut format_buffer,
                "{prefix}... first {hidden} {title} lines hidden ..."
            );
            renderer.line(MessageStyle::Info, &format_buffer)?;
        }
    }

    if !is_mcp_tool {
        format_buffer.clear();
        format_buffer.push('[');
        for ch in title.chars() {
            format_buffer.push(ch.to_ascii_uppercase());
        }
        format_buffer.push(']');
        renderer.line(MessageStyle::Info, &format_buffer)?;
    }

    // Reuse a single String buffer for all lines to avoid repeated allocations
    let mut display_buffer = String::with_capacity(128);
    let prefix = if is_mcp_tool { "" } else { "  " };

    for line in &lines_vec {
        if line.is_empty() {
            display_buffer.clear();
        } else {
            display_buffer.clear();
            display_buffer.push_str(prefix);
            display_buffer.push_str(line);
        }

        if let Some(style) = select_line_style(tool_name, line, git_styles, ls_styles) {
            renderer.line_with_style(style, &display_buffer)?;
        } else {
            renderer.line(fallback_style, &display_buffer)?;
        }
    }

    Ok(())
}

fn describe_code_fence_header(language: Option<&str>) -> String {
    let Some(lang) = language
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return "Code block".to_string();
    };

    let lower = lang.to_ascii_lowercase();
    match lower.as_str() {
        "sh" | "bash" | "zsh" | "shell" | "pwsh" | "powershell" | "cmd" | "batch" | "bat" => {
            format!("Shell ({lower})")
        }
        _ => {
            let mut chars = lower.chars();
            let Some(first) = chars.next() else {
                return "Code block".to_string();
            };
            let mut label = first.to_uppercase().collect::<String>();
            label.extend(chars);
            format!("{label} block")
        }
    }
}

pub(crate) fn render_code_fence_blocks(
    renderer: &mut AnsiRenderer,
    blocks: &[CodeFenceBlock],
) -> Result<()> {
    const MIN_WIDTH: u16 = 40;
    const MAX_WIDTH: u16 = 96;
    let content_limit = MAX_WIDTH.saturating_sub(4) as usize;
    for (index, block) in blocks.iter().enumerate() {
        let header = describe_code_fence_header(block.language.as_deref());

        let mut lines = Vec::new();

        if block.lines.is_empty() {
            lines.push(PanelContentLine::new(
                clamp_panel_text("    (no content)", content_limit),
                MessageStyle::Info,
            ));
        } else {
            lines.push(PanelContentLine::new(String::new(), MessageStyle::Response));
            for line in &block.lines {
                let display = format!("    {}", line);
                lines.push(PanelContentLine::new(
                    clamp_panel_text(&display, content_limit),
                    MessageStyle::Response,
                ));
            }
        }

        render_panel(
            renderer,
            Some(clamp_panel_text(&header, content_limit)),
            lines,
            MessageStyle::Response,
            MIN_WIDTH,
            MAX_WIDTH,
        )?;

        if index + 1 < blocks.len() {
            renderer.line(MessageStyle::Response, "")?;
        }
    }

    Ok(())
}

fn detect_output_language(stdout: &str) -> Option<&'static str> {
    let trimmed = stdout.trim();

    // JSON detection
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if serde_json::from_str::<Value>(trimmed).is_ok() {
            return Some("json");
        }
    }

    // XML/HTML detection
    if trimmed.starts_with('<') && trimmed.contains('>') {
        return Some("xml");
    }

    // YAML detection (common patterns)
    if trimmed.contains(":\n") || trimmed.contains(": ") {
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.len() > 1 && lines.iter().any(|l| l.contains(": ")) {
            return Some("yaml");
        }
    }

    None
}

fn apply_syntax_color(text: &str, language: Option<&str>) -> String {
    match language {
        Some("json") => {
            // Simple JSON coloring
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                return colorize_json(&parsed);
            }
            text.to_string()
        }
        _ => text.to_string(),
    }
}

fn colorize_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut result = String::from("\x1b[90m{\x1b[0m");
            let entries: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    format!(
                        "\x1b[36m\"{}\"\x1b[0m\x1b[90m:\x1b[0m{}",
                        k,
                        colorize_json(v)
                    )
                })
                .collect();
            result.push_str(&entries.join("\x1b[90m,\x1b[0m"));
            result.push_str("\x1b[90m}\x1b[0m");
            result
        }
        Value::Array(arr) => {
            let mut result = String::from("\x1b[90m[\x1b[0m");
            let entries: Vec<String> = arr.iter().map(colorize_json).collect();
            result.push_str(&entries.join("\x1b[90m,\x1b[0m"));
            result.push_str("\x1b[90m]\x1b[0m");
            result
        }
        Value::String(s) => format!("\x1b[32m\"{}\"\x1b[0m", s),
        Value::Number(n) => format!("\x1b[33m{}\x1b[0m", n),
        Value::Bool(b) => format!("\x1b[35m{}\x1b[0m", b),
        Value::Null => String::from("\x1b[90mnull\x1b[0m"),
    }
}

fn render_terminal_command_panel(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    vt_config: Option<&VTCodeConfig>,
    allow_ansi: bool,
) -> Result<()> {
    // Status is now rendered in the tool summary line, so we skip it here

    // Show exit code if available
    if let Some(exit_code) = payload.get("exit_code").and_then(|v| v.as_i64()) {
        let code_style = if exit_code == 0 {
            MessageStyle::Response
        } else {
            MessageStyle::Error
        };
        renderer.line(code_style, &format!("  exit code: {}", exit_code))?;
    }

    let command_tokens = parse_command_tokens(payload);
    // Show command if available
    if let Some(display) = command_display_string(payload, command_tokens.as_deref()) {
        renderer.line(MessageStyle::Info, &format!("  $ {}", display))?;
    }

    let stdout_raw = payload.get("stdout").and_then(Value::as_str).unwrap_or("");
    let stderr_raw = payload.get("stderr").and_then(Value::as_str).unwrap_or("");
    let stdout = preprocess_terminal_stdout(command_tokens.as_deref(), stdout_raw);
    let stderr = stderr_raw;

    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);

    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        renderer.line(MessageStyle::Info, "(no output)")?;
        return Ok(());
    }

    if !stdout.trim().is_empty() {
        render_stream_section(
            renderer,
            "stdout",
            stdout.as_ref(),
            output_mode,
            tail_limit,
            Some(tools::RUN_TERMINAL_CMD),
            git_styles,
            ls_styles,
            MessageStyle::Response,
            allow_ansi,
            vt_config,
        )?;
    }
    if !stderr.trim().is_empty() {
        render_stream_section(
            renderer,
            "stderr",
            stderr,
            output_mode,
            tail_limit,
            Some(tools::RUN_TERMINAL_CMD),
            git_styles,
            ls_styles,
            MessageStyle::Error,
            allow_ansi,
            vt_config,
        )?;
    }

    Ok(())
}

fn command_display_string(payload: &Value, tokens: Option<&[String]>) -> Option<String> {
    let truncate = |text: &str| {
        if text.len() > 80 {
            format!("{}…", &text[..77])
        } else {
            text.to_string()
        }
    };

    if let Some(parts) = tokens {
        if !parts.is_empty() {
            let joined = parts.join(" ");
            return Some(truncate(&joined));
        }
    }

    payload
        .get("command")
        .and_then(|v| v.as_str())
        .map(truncate)
}

fn parse_command_tokens(payload: &Value) -> Option<Vec<String>> {
    if let Some(array) = payload.get("command").and_then(Value::as_array) {
        let mut tokens = Vec::new();
        for value in array {
            if let Some(segment) = value.as_str() {
                if !segment.is_empty() {
                    tokens.push(segment.to_string());
                }
            }
        }
        if !tokens.is_empty() {
            return Some(tokens);
        }
    }

    if let Some(command_str) = payload.get("command").and_then(Value::as_str) {
        if command_str.trim().is_empty() {
            return None;
        }
        if let Ok(segments) = shell_split(command_str) {
            if !segments.is_empty() {
                return Some(segments);
            }
        }
    }
    None
}

fn normalized_command_name(tokens: &[String]) -> Option<String> {
    tokens
        .first()
        .and_then(|cmd| Path::new(cmd).file_name())
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
}

fn command_is_multicol_listing(tokens: &[String]) -> bool {
    normalized_command_name(tokens)
        .map(|name| {
            matches!(
                name.as_str(),
                "ls" | "dir" | "vdir" | "gls" | "colorls" | "exa" | "eza"
            )
        })
        .unwrap_or(false)
}

fn listing_has_single_column_flag(tokens: &[String]) -> bool {
    tokens.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-1" | "--format=single-column"
                | "--long"
                | "-l"
                | "--tree"
                | "--grid=never"
                | "--no-grid"
        )
    })
}

fn preprocess_terminal_stdout<'a>(tokens: Option<&[String]>, stdout: &'a str) -> Cow<'a, str> {
    if stdout.trim().is_empty() {
        return Cow::Borrowed(stdout);
    }

    if let Some(parts) = tokens {
        if command_is_multicol_listing(parts) && !listing_has_single_column_flag(parts) {
            let plain = strip_ansi_codes(stdout);
            let mut rows = String::with_capacity(plain.len());
            for entry in plain.split_whitespace() {
                if entry.is_empty() {
                    continue;
                }
                rows.push_str(entry);
                rows.push('\n');
            }
            return Cow::Owned(rows);
        }
    }

    Cow::Borrowed(stdout)
}

fn strip_ansi_codes(input: &str) -> Cow<'_, str> {
    if !input.contains('\x1b') {
        return Cow::Borrowed(input);
    }

    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                while let Some(next) = chars.next() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        output.push(ch);
    }
    Cow::Owned(output)
}

/// Statistics for a single diff file
struct DiffFileStats {
    additions: usize,
    deletions: usize,
    total_lines: usize,
}

impl DiffFileStats {
    fn from_diff(content: &str) -> Self {
        let mut additions = 0;
        let mut deletions = 0;
        let mut total_lines = 0;

        for line in content.lines() {
            total_lines += 1;
            if line.starts_with('+') && !line.starts_with("+++") {
                additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                deletions += 1;
            }
        }

        Self {
            additions,
            deletions,
            total_lines,
        }
    }

    fn summary(&self) -> String {
        format!(
            "+{} -{} ({} lines)",
            self.additions, self.deletions, self.total_lines
        )
    }
}

#[cfg_attr(
    feature = "profiling",
    tracing::instrument(
        skip(renderer, payload, git_styles, ls_styles, config),
        level = "debug"
    )
)]
fn render_git_diff(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    allow_ansi: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    let sections = diff_sections(payload);
    if sections.is_empty() {
        return Ok(());
    }

    // In compact mode with many files, show summaries + tail previews
    let should_virtualize = matches!(mode, ToolOutputMode::Compact) && sections.len() > 3;

    if should_virtualize {
        // Show per-file summaries
        for (label, content) in &sections {
            if content.trim().is_empty() {
                continue;
            }
            let stats = DiffFileStats::from_diff(content);
            renderer.line(
                MessageStyle::Info,
                &format!("  {} {}", label, stats.summary()),
            )?;
        }

        // Show tail preview of the last file
        if let Some((last_label, last_content)) = sections.last() {
            if !last_content.trim().is_empty() {
                renderer.line(MessageStyle::Info, "")?;
                renderer.line(MessageStyle::Info, &format!("Preview of {}:", last_label))?;
                render_stream_section(
                    renderer,
                    "",
                    last_content,
                    mode,
                    tail_limit.min(20), // Limit preview to 20 lines
                    Some(tools::GIT_DIFF),
                    git_styles,
                    ls_styles,
                    MessageStyle::Info,
                    allow_ansi,
                    config,
                )?;
            }
        }
    } else {
        // Full mode or few files: show everything
        for (label, content) in sections {
            if content.trim().is_empty() {
                continue;
            }

            render_stream_section(
                renderer,
                &label,
                &content,
                mode,
                tail_limit,
                Some(tools::GIT_DIFF),
                git_styles,
                ls_styles,
                MessageStyle::Info,
                allow_ansi,
                config,
            )?;
        }
    }

    Ok(())
}

fn diff_sections(payload: &Value) -> Vec<(String, String)> {
    payload
        .get("files")
        .and_then(|value| value.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    let formatted = file.get("formatted")?.as_str()?.trim();
                    if formatted.is_empty() {
                        return None;
                    }
                    let label = file
                        .get("path")
                        .and_then(|value| value.as_str())
                        .unwrap_or("diff");
                    Some((label.to_string(), strip_diff_fences(formatted).into_owned()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn strip_diff_fences(input: &str) -> Cow<'_, str> {
    // Fast path: check first line without collecting
    let mut lines_iter = input.lines();
    let first_line = match lines_iter.next() {
        Some(line) if line.trim().starts_with("```") => line,
        _ => return Cow::Borrowed(input),
    };

    // Count lines efficiently
    let line_count = 1 + lines_iter.clone().count();
    if line_count < 2 {
        return Cow::Borrowed(input);
    }

    // Check last line
    let last_line = lines_iter.clone().last();
    let has_closing_fence = last_line.is_some_and(|line| line.trim() == "```");

    if !has_closing_fence {
        // Only strip first line
        let first_len = first_line.len();
        let remainder_start = if input.as_bytes().get(first_len) == Some(&b'\n') {
            first_len + 1
        } else {
            first_len
        };
        return Cow::Borrowed(&input[remainder_start..]);
    }

    // Strip both first and last - need to rebuild
    let mut result = String::with_capacity(input.len());
    let mut lines = input.lines();
    lines.next(); // Skip first

    let middle_lines: SmallVec<[&str; 32]> = lines.collect();
    if middle_lines.is_empty() {
        return Cow::Owned(String::new());
    }

    // Join all but last
    for (i, line) in middle_lines.iter().enumerate() {
        if i == middle_lines.len() - 1 {
            break; // Skip last line (closing fence)
        }
        if i > 0 {
            result.push('\n');
        }
        result.push_str(line);
    }

    Cow::Owned(result)
}

fn render_curl_result(
    renderer: &mut AnsiRenderer,
    val: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
    allow_ansi: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    // Status is now rendered in the tool summary line, so we skip it here

    // Reuse buffer for formatting
    use std::fmt::Write as _;
    let mut msg_buffer = String::with_capacity(128);

    // Show HTTP status if available
    if let Some(status) = val.get("status").and_then(|v| v.as_u64()) {
        let status_style = if status >= 200 && status < 300 {
            MessageStyle::Response
        } else if status >= 400 {
            MessageStyle::Error
        } else {
            MessageStyle::Info
        };
        msg_buffer.clear();
        let _ = write!(&mut msg_buffer, "  HTTP {}", status);
        renderer.line(status_style, &msg_buffer)?;
    }

    // Show content type if available
    if let Some(content_type) = val.get("content_type").and_then(|v| v.as_str()) {
        msg_buffer.clear();
        let _ = write!(&mut msg_buffer, "  Content-Type: {}", content_type);
        renderer.line(MessageStyle::Info, &msg_buffer)?;
    }

    // Body output
    if let Some(body) = val.get("body").and_then(Value::as_str)
        && !body.trim().is_empty()
    {
        let normalized_body = if allow_ansi {
            Cow::Borrowed(body)
        } else {
            strip_ansi_codes(body)
        };

        // Check if we should spool to disk
        if let Ok(Some(log_path)) =
            spool_output_if_needed(normalized_body.as_ref(), tools::CURL, config)
        {
            let (tail, total) = tail_lines_streaming(normalized_body.as_ref(), 20);
            msg_buffer.clear();
            let _ = write!(
                &mut msg_buffer,
                "Response body too large ({} bytes, {} lines), spooled to: {}",
                body.len(),
                total,
                log_path.display()
            );
            renderer.line(MessageStyle::Info, &msg_buffer)?;
            renderer.line(MessageStyle::Info, "Last 20 lines:")?;
            for line in &tail {
                renderer.line(MessageStyle::Response, line.trim_end())?;
            }
            return Ok(());
        }

        let prefer_full = renderer.prefers_untruncated_output();
        let (lines, _total, _truncated) =
            select_stream_lines_streaming(normalized_body.as_ref(), mode, tail_limit, prefer_full);

        // Detect language for syntax coloring
        let language = detect_output_language(normalized_body.as_ref());

        for line in &lines {
            let colored_line = if language.is_some() {
                apply_syntax_color(line.trim_end(), language)
            } else {
                line.trim_end().to_string()
            };

            renderer.line(MessageStyle::Response, &colored_line)?;
        }
    } else {
        renderer.line(MessageStyle::Info, "(no response body)")?;
    }

    Ok(())
}

fn render_list_dir_output(
    renderer: &mut AnsiRenderer,
    val: &Value,
    ls_styles: &LsStyles,
) -> Result<()> {
    // Show path being listed
    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  {}", path))?;
    }

    // Show pagination info
    if let Some(page) = val.get("page").and_then(|v| v.as_u64()) {
        if let Some(total) = val.get("total_items").and_then(|v| v.as_u64()) {
            renderer.line(
                MessageStyle::Info,
                &format!("  Page {} ({} items total)", page, total),
            )?;
        }
    }

    // Render items
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

    // Show "has more" indicator
    if val
        .get("has_more")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        renderer.line(MessageStyle::Info, "  ... more items available")?;
    }

    Ok(())
}

fn render_read_file_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Show encoding if specified
    if let Some(encoding) = val.get("encoding").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  encoding: {}", encoding))?;
    }

    // Show file size
    if let Some(size) = val.get("size").and_then(|v| v.as_u64()) {
        renderer.line(
            MessageStyle::Info,
            &format!("  size: {}", format_size(size)),
        )?;
    }

    // Show line range if partial read
    if let Some(start) = val.get("start_line").and_then(|v| v.as_u64()) {
        if let Some(end) = val.get("end_line").and_then(|v| v.as_u64()) {
            renderer.line(MessageStyle::Info, &format!("  lines: {}-{}", start, end))?;
        }
    }

    // Content is typically shown via stdout, so we don't duplicate it here
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

struct GitStyles {
    add: Option<AnsiStyle>,
    remove: Option<AnsiStyle>,
    header: Option<AnsiStyle>,
}

impl GitStyles {
    fn new() -> Self {
        Self {
            add: Some(AnsiStyle::new().fg_color(Some(AnsiColor::Green.into()))),
            remove: Some(AnsiStyle::new().fg_color(Some(AnsiColor::Red.into()))),
            header: Some(
                AnsiStyle::new()
                    .bold()
                    .fg_color(Some(AnsiColor::Yellow.into())),
            ),
        }
    }
}

struct LsStyles {
    classes: HashMap<String, AnsiStyle>,
    suffixes: Vec<(String, AnsiStyle)>,
}

impl LsStyles {
    fn from_env() -> Self {
        let mut classes: HashMap<String, AnsiStyle> = HashMap::new();
        let suffixes: Vec<(String, AnsiStyle)> = Vec::new();

        // For now, skip parsing LS_COLORS and just use defaults
        // TODO: Implement ANSI parsing if needed

        // Default styles
        classes.insert(
            "di".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Blue.into())),
        );
        classes.insert(
            "ln".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Cyan.into())),
        );
        classes.insert(
            "ex".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Green.into())),
        );
        classes.insert(
            "pi".to_string(),
            AnsiStyle::new().fg_color(Some(AnsiColor::Yellow.into())),
        );
        classes.insert(
            "so".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Magenta.into())),
        );
        classes.insert(
            "bd".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Yellow.into())),
        );
        classes.insert(
            "cd".to_string(),
            AnsiStyle::new()
                .bold()
                .fg_color(Some(AnsiColor::Yellow.into())),
        );

        LsStyles { classes, suffixes }
    }

    fn style_for_line(&self, line: &str) -> Option<AnsiStyle> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        let token = trimmed
            .split_whitespace()
            .last()
            .unwrap_or(trimmed)
            .trim_matches('"');

        let mut name = token;
        let mut class_hint: Option<&str> = None;

        if let Some(stripped) = name.strip_suffix('/') {
            name = stripped;
            class_hint = Some("di");
        } else if let Some(stripped) = name.strip_suffix('@') {
            name = stripped;
            class_hint = Some("ln");
        } else if let Some(stripped) = name.strip_suffix('*') {
            name = stripped;
            class_hint = Some("ex");
        } else if let Some(stripped) = name.strip_suffix('|') {
            name = stripped;
            class_hint = Some("pi");
        } else if let Some(stripped) = name.strip_suffix('=') {
            name = stripped;
            class_hint = Some("so");
        }

        if class_hint.is_none() {
            match trimmed.chars().next() {
                Some('d') => class_hint = Some("di"),
                Some('l') => class_hint = Some("ln"),
                Some('p') => class_hint = Some("pi"),
                Some('s') => class_hint = Some("so"),
                Some('b') => class_hint = Some("bd"),
                Some('c') => class_hint = Some("cd"),
                _ => {}
            }
        }

        if let Some(code) = class_hint {
            if let Some(style) = self.classes.get(code) {
                return Some(*style);
            }
        }

        let lower = name
            .trim_matches(|c| matches!(c, '"' | ',' | ' ' | '\u{0009}'))
            .to_ascii_lowercase();
        for (suffix, style) in &self.suffixes {
            if lower.ends_with(suffix) {
                return Some(*style);
            }
        }

        if lower.ends_with('*') {
            if let Some(style) = self.classes.get("ex") {
                return Some(*style);
            }
        }

        None
    }

    #[cfg(test)]
    fn from_components(
        classes: HashMap<String, AnsiStyle>,
        suffixes: Vec<(String, AnsiStyle)>,
    ) -> Self {
        Self { classes, suffixes }
    }
}

fn select_line_style(
    tool_name: Option<&str>,
    line: &str,
    git: &GitStyles,
    ls: &LsStyles,
) -> Option<AnsiStyle> {
    match tool_name {
        Some(name)
            if matches!(
                name,
                tools::RUN_TERMINAL_CMD
                    | tools::BASH
                    | tools::WRITE_FILE
                    | tools::EDIT_FILE
                    | tools::APPLY_PATCH
                    | tools::SRGN
                    | tools::GIT_DIFF
            ) =>
        {
            let trimmed = line.trim_start();
            if trimmed.starts_with("diff --")
                || trimmed.starts_with("index ")
                || trimmed.starts_with("@@")
            {
                return git.header;
            }

            if trimmed.starts_with('+') {
                return git.add;
            }
            if trimmed.starts_with('-') {
                return git.remove;
            }

            if let Some(style) = ls.style_for_line(trimmed) {
                return Some(style);
            }
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_terminal_followup_message(
        command: &str,
        absorbed: bool,
        exit_code: Option<i32>,
    ) -> String {
        let command = command.replace('\n', " ");
        if absorbed {
            format!("Absorbed terminal output for `{}`.", command)
        } else {
            match exit_code {
                Some(code) => format!("Captured `{}` output (exit code {}).", command, code),
                None => format!("Captured `{}` output.", command),
            }
        }
    }

    #[test]
    fn describes_shell_code_fence_as_shell_header() {
        let header = describe_code_fence_header(Some("bash"));
        assert_eq!(header, "Shell (bash)");

        let rust_header = describe_code_fence_header(Some("rust"));
        assert_eq!(rust_header, "Rust block");

        let empty_header = describe_code_fence_header(None);
        assert_eq!(empty_header, "Code block");
    }

    #[test]
    fn detects_git_diff_styling() {
        let git = GitStyles::new();
        let ls = LsStyles::from_components(HashMap::new(), Vec::new());
        let added = select_line_style(Some("run_terminal_cmd"), "+added line", &git, &ls);
        assert_eq!(added, git.add);
        let removed = select_line_style(Some("run_terminal_cmd"), "-removed line", &git, &ls);
        assert_eq!(removed, git.remove);
        let header = select_line_style(
            Some("run_terminal_cmd"),
            "diff --git a/file b/file",
            &git,
            &ls,
        );
        assert_eq!(header, git.header);
    }

    #[test]
    fn detects_ls_styles_for_directories_and_executables() {
        use anstyle::AnsiColor;

        let git = GitStyles::new();
        let dir_style = AnsiStyle::new().bold();
        let exec_style =
            AnsiStyle::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green.into())));
        let mut classes = HashMap::new();
        classes.insert("di".to_string(), dir_style);
        classes.insert("ex".to_string(), exec_style);
        let ls = LsStyles::from_components(classes, Vec::new());
        let directory = select_line_style(Some("run_terminal_cmd"), "folder/", &git, &ls);
        assert_eq!(directory, Some(dir_style));
        let executable = select_line_style(Some("run_terminal_cmd"), "script*", &git, &ls);
        assert_eq!(executable, Some(exec_style));
    }

    #[test]
    fn non_terminal_tools_do_not_apply_special_styles() {
        let git = GitStyles::new();
        let ls = LsStyles::from_components(HashMap::new(), Vec::new());
        let styled = select_line_style(Some("context7"), "+added", &git, &ls);
        assert!(styled.is_none());
    }

    #[test]
    fn applies_extension_based_styles() {
        let git = GitStyles::new();
        let mut suffixes = Vec::new();
        suffixes.push((
            ".rs".to_string(),
            AnsiStyle::new().fg_color(Some(anstyle::AnsiColor::Red.into())),
        ));
        let ls = LsStyles::from_components(HashMap::new(), suffixes);
        let styled = select_line_style(Some("run_terminal_cmd"), "main.rs", &git, &ls);
        assert!(styled.is_some());
    }

    #[test]
    fn extension_matching_requires_dot_boundary() {
        let git = GitStyles::new();
        let mut suffixes = Vec::new();
        suffixes.push((
            ".rs".to_string(),
            AnsiStyle::new().fg_color(Some(anstyle::AnsiColor::Green.into())),
        ));
        let ls = LsStyles::from_components(HashMap::new(), suffixes);

        let without_extension = select_line_style(Some("run_terminal_cmd"), "helpers", &git, &ls);
        assert!(without_extension.is_none());

        let with_extension = select_line_style(Some("run_terminal_cmd"), "helpers.rs", &git, &ls);
        assert!(with_extension.is_some());
    }

    #[test]
    fn followup_message_references_command() {
        let message = build_terminal_followup_message("ls -a", true, None);
        assert_eq!(message, "Absorbed terminal output for `ls -a`.");
    }

    #[test]
    fn followup_message_includes_exit_code() {
        let message = build_terminal_followup_message("npm test", false, Some(1));
        assert_eq!(message, "Captured `npm test` output (exit code 1).");
    }

    #[test]
    fn followup_message_collapses_whitespace() {
        let message = build_terminal_followup_message("echo foo\nbar", true, None);
        assert!(message.contains("echo foo bar"));
        assert!(!message.contains('\n'));
    }

    #[test]
    fn compact_mode_truncates_when_not_inline() {
        let content = (1..=50)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, total, truncated) =
            select_stream_lines(&content, ToolOutputMode::Compact, 10, false);
        assert_eq!(total, 50);
        assert_eq!(lines.len(), 10);
        assert!(truncated);
        assert_eq!(lines.first().copied(), Some("line-41"));
    }

    #[test]
    fn inline_rendering_preserves_full_scrollback() {
        let content = (1..=30)
            .map(|index| format!("row-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (lines, total, truncated) =
            select_stream_lines(&content, ToolOutputMode::Compact, 5, true);
        assert_eq!(total, 30);
        assert_eq!(lines.len(), 30);
        assert!(!truncated);
        assert_eq!(lines.first().copied(), Some("row-1"));
        assert_eq!(lines.last().copied(), Some("row-30"));
    }

    // Tests removed - render_tool_status_header function no longer exists
}
