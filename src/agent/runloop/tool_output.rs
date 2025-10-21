use anstyle::Style;
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::{defaults, tools};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::tools::{PlanCompletionState, StepStatus, TaskPlan};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::text_tools::CodeFenceBlock;

// Box drawing characters
const BOX_TOP_LEFT: &str = "┌";
const BOX_TOP_RIGHT: &str = "┐";
const BOX_BOTTOM_LEFT: &str = "└";
const BOX_BOTTOM_RIGHT: &str = "┘";
const BOX_HORIZONTAL: &str = "─";
const BOX_VERTICAL: &str = "│";
const BOX_T_RIGHT: &str = "├";
const BOX_T_LEFT: &str = "┤";

fn render_table_border(
    renderer: &mut AnsiRenderer,
    width: usize,
    style: MessageStyle,
) -> Result<()> {
    let line = format!(
        "{}{}{}",
        BOX_TOP_LEFT,
        BOX_HORIZONTAL.repeat(width - 2),
        BOX_TOP_RIGHT
    );
    renderer.line(style, &line)
}

fn render_table_separator(
    renderer: &mut AnsiRenderer,
    width: usize,
    style: MessageStyle,
) -> Result<()> {
    let line = format!(
        "{}{}{}",
        BOX_T_RIGHT,
        BOX_HORIZONTAL.repeat(width - 2),
        BOX_T_LEFT
    );
    renderer.line(style, &line)
}

fn render_table_bottom(
    renderer: &mut AnsiRenderer,
    width: usize,
    style: MessageStyle,
) -> Result<()> {
    let line = format!(
        "{}{}{}",
        BOX_BOTTOM_LEFT,
        BOX_HORIZONTAL.repeat(width - 2),
        BOX_BOTTOM_RIGHT
    );
    renderer.line(style, &line)
}

fn render_table_row(
    renderer: &mut AnsiRenderer,
    content: &str,
    width: usize,
    style: MessageStyle,
) -> Result<()> {
    let content_len = content.chars().count();
    let padding = if content_len < width - 4 {
        width - 4 - content_len
    } else {
        0
    };
    let line = format!(
        "{} {} {}{}",
        BOX_VERTICAL,
        content,
        " ".repeat(padding),
        BOX_VERTICAL
    );
    renderer.line(style, &line)
}

pub(crate) fn render_tool_output(
    renderer: &mut AnsiRenderer,
    tool_name: Option<&str>,
    val: &Value,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    // Handle special tools first
    match tool_name {
        Some(tools::UPDATE_PLAN) => return render_plan_update(renderer, val),
        Some(tools::WRITE_FILE) => {
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
            );
        }
        Some(tools::RUN_TERMINAL_CMD) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_terminal_command_panel(renderer, val, &git_styles, &ls_styles);
        }
        Some(tools::CURL) => {
            let output_mode = vt_config
                .map(|cfg| cfg.ui.tool_output_mode)
                .unwrap_or(ToolOutputMode::Compact);
            let tail_limit = resolve_stdout_tail_limit(vt_config);
            return render_curl_result(renderer, val, output_mode, tail_limit);
        }
        _ => {}
    }

    // Render security notice if present
    if let Some(notice) = val.get("security_notice").and_then(Value::as_str) {
        renderer.line(MessageStyle::Info, notice)?;
    }

    // Handle MCP tools
    if let Some(tool) = tool_name
        && let Some(profile) = resolve_mcp_renderer_profile(tool, vt_config)
    {
        match profile {
            McpRendererProfile::Context7 => render_mcp_context7_output(renderer, val)?,
            McpRendererProfile::SequentialThinking => render_mcp_sequential_output(renderer, val)?,
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
        )?;
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
    let status = val
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");

    let tool_used = val
        .get("tool")
        .and_then(|value| value.as_str())
        .unwrap_or("context7");

    renderer.line(
        MessageStyle::Info,
        &format!("[mcp:{}] {}", tool_used, status),
    )?;

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
    let status = val
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");

    let summary = val
        .get("summary")
        .and_then(|value| value.as_str())
        .unwrap_or("Sequential reasoning summary unavailable");

    renderer.line(MessageStyle::Info, &format!("[mcp:thinking] {}", status))?;

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
    const TABLE_WIDTH: usize = 60;

    // Table header
    render_table_border(renderer, TABLE_WIDTH, MessageStyle::Info)?;

    // Progress row
    let progress = format!(
        "Progress: {}/{} · {}",
        plan.summary.completed_steps,
        plan.summary.total_steps,
        plan.summary.status.description()
    );
    render_table_row(renderer, &progress, TABLE_WIDTH, MessageStyle::Info)?;

    // Separator if we have explanation or steps
    if plan.explanation.is_some() || !plan.steps.is_empty() {
        render_table_separator(renderer, TABLE_WIDTH, MessageStyle::Info)?;
    }

    // Optional explanation
    if let Some(explanation) = plan.explanation.as_ref() {
        let first_line = explanation.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            let truncated = if first_line.len() > TABLE_WIDTH - 6 {
                format!("{}…", &first_line[..TABLE_WIDTH - 7])
            } else {
                first_line.to_string()
            };
            render_table_row(renderer, &truncated, TABLE_WIDTH, MessageStyle::Info)?;
            if !plan.steps.is_empty() {
                render_table_separator(renderer, TABLE_WIDTH, MessageStyle::Info)?;
            }
        }
    }

    // Steps
    for (index, step) in plan.steps.iter().enumerate() {
        let checkbox = match step.status {
            StepStatus::Pending => "[ ]",
            StepStatus::InProgress => "[▸]",
            StepStatus::Completed => "[✓]",
        };
        let step_text = step.step.trim();
        let step_number = index + 1;

        let content = format!("{step_number}. {checkbox} {step_text}");
        let truncated = if content.len() > TABLE_WIDTH - 6 {
            format!("{}…", &content[..TABLE_WIDTH - 7])
        } else {
            content
        };

        render_table_row(renderer, &truncated, TABLE_WIDTH, MessageStyle::Info)?;
    }

    // Table bottom
    render_table_bottom(renderer, TABLE_WIDTH, MessageStyle::Info)?;

    Ok(())
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

fn resolve_stdout_tail_limit(config: Option<&VTCodeConfig>) -> usize {
    config
        .map(|cfg| cfg.pty.stdout_tail_lines)
        .filter(|&lines| lines > 0)
        .unwrap_or(defaults::DEFAULT_PTY_STDOUT_TAIL_LINES)
}

fn tail_lines(text: &str, limit: usize) -> (Vec<&str>, usize) {
    if text.is_empty() {
        return (Vec::new(), 0);
    }
    if limit == 0 {
        return (Vec::new(), text.lines().count());
    }

    let mut ring = VecDeque::with_capacity(limit);
    let mut total = 0;
    for line in text.lines() {
        total += 1;
        if ring.len() == limit {
            ring.pop_front();
        }
        ring.push_back(line);
    }

    (ring.into_iter().collect(), total)
}

fn select_stream_lines(
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    prefer_full: bool,
) -> (Vec<&str>, usize, bool) {
    if content.is_empty() {
        return (Vec::new(), 0, false);
    }

    if prefer_full || matches!(mode, ToolOutputMode::Full) {
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        return (lines, total, false);
    }

    let (tail, total) = tail_lines(content, tail_limit);
    let truncated = total > tail.len();
    (tail, total, truncated)
}

fn render_write_file_preview(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let path = payload
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or("(unknown path)");
    let mode = payload
        .get("mode")
        .and_then(|value| value.as_str())
        .unwrap_or("overwrite");
    let bytes_written = payload
        .get("bytes_written")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    renderer.line(MessageStyle::Info, &format!("[write_file] {path}"))?;
    renderer.line(
        MessageStyle::Info,
        &format!("  mode={mode} | bytes={bytes_written}"),
    )?;

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
                &format!("  … diff truncated ({omitted} lines omitted)"),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "  … diff truncated")?;
        }
    }

    Ok(())
}

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
) -> Result<()> {
    let is_mcp_tool = tool_name.map_or(false, |name| name.starts_with("mcp_"));
    let prefer_full = renderer.prefers_untruncated_output();
    let (lines, total, truncated) = select_stream_lines(content, mode, tail_limit, prefer_full);

    if lines.is_empty() {
        return Ok(());
    }

    if truncated {
        let prefix = if is_mcp_tool { "" } else { "  " };
        renderer.line(
            MessageStyle::Info,
            &format!(
                "{prefix}... showing last {}/{} {} lines",
                lines.len(),
                total,
                title
            ),
        )?;
    }

    if !is_mcp_tool {
        renderer.line(MessageStyle::Info, &format!("[{}]", title.to_uppercase()))?;
    }

    for line in lines {
        let display = if line.is_empty() {
            String::new()
        } else {
            let prefix = if is_mcp_tool { "" } else { "  " };
            format!("{prefix}{line}")
        };

        if let Some(style) = select_line_style(tool_name, line, git_styles, ls_styles) {
            renderer.line_with_style(style, &display)?;
        } else {
            renderer.line(fallback_style, &display)?;
        }
    }

    Ok(())
}

struct CommandPanelRow {
    text: String,
    style: MessageStyle,
    override_style: Option<Style>,
}

impl CommandPanelRow {
    fn new(text: String, style: MessageStyle) -> Self {
        Self {
            text,
            style,
            override_style: None,
        }
    }

    #[allow(dead_code)]
    fn with_override(text: String, style: MessageStyle, override_style: Style) -> Self {
        Self {
            text,
            style,
            override_style: Some(override_style),
        }
    }

    fn blank(style: MessageStyle) -> Self {
        Self::new(String::new(), style)
    }
}

#[allow(dead_code)]
struct CommandPanelDisplayLine {
    display: String,
    style: MessageStyle,
    override_style: Option<Style>,
}

fn build_command_panel_display(rows: Vec<CommandPanelRow>) -> Vec<CommandPanelDisplayLine> {
    rows.into_iter()
        .map(|row| CommandPanelDisplayLine {
            display: row.text,
            style: row.style,
            override_style: row.override_style,
        })
        .collect()
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
    for (index, block) in blocks.iter().enumerate() {
        let header = describe_code_fence_header(block.language.as_deref());

        let mut rows = Vec::new();
        rows.push(CommandPanelRow::new(header, MessageStyle::Info));
        rows.push(CommandPanelRow::blank(MessageStyle::Response));

        if block.lines.is_empty() {
            rows.push(CommandPanelRow::new(
                "    (no content)".to_string(),
                MessageStyle::Info,
            ));
        } else {
            for line in &block.lines {
                let display = format!("    {}", line);
                rows.push(CommandPanelRow::new(display, MessageStyle::Response));
            }
        }

        let panel_lines = build_command_panel_display(rows);
        for line in panel_lines {
            if let Some(style) = line.override_style {
                renderer.line_with_override_style(line.style, style, &line.display)?;
            } else {
                renderer.line(line.style, &line.display)?;
            }
        }

        if index + 1 < blocks.len() {
            renderer.line(MessageStyle::Response, "")?;
        }
    }

    Ok(())
}

fn render_terminal_command_panel(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    _git_styles: &GitStyles,
    _ls_styles: &LsStyles,
) -> Result<()> {
    const TABLE_WIDTH: usize = 70;
    let output_mode = ToolOutputMode::Compact;
    let tail_limit = defaults::DEFAULT_PTY_STDOUT_TAIL_LINES;
    let prefer_full = renderer.prefers_untruncated_output();

    let command_display = payload
        .get("command")
        .and_then(|value| value.as_str())
        .unwrap_or("(command)");

    let success = payload
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let exit_code = payload.get("exit_code").and_then(|value| value.as_i64());

    let shell_label = match payload.get("mode").and_then(|value| value.as_str()) {
        Some("pty") => "pty",
        _ => "cmd",
    };

    // Header
    let status_icon = if success { "✓" } else { "✗" };
    let mut header = format!("{} [{}] {}", status_icon, shell_label, command_display);

    if !success {
        if let Some(code) = exit_code {
            header.push_str(&format!(" (exit {})", code));
        }
    }

    let header_style = if success {
        MessageStyle::Info
    } else {
        MessageStyle::Error
    };

    let truncated_header = if header.len() > TABLE_WIDTH - 6 {
        format!("{}…", &header[..TABLE_WIDTH - 7])
    } else {
        header
    };

    render_table_border(renderer, TABLE_WIDTH, header_style)?;
    render_table_row(renderer, &truncated_header, TABLE_WIDTH, header_style)?;

    let stdout = payload.get("stdout").and_then(Value::as_str).unwrap_or("");
    let (stdout_lines, stdout_total, stdout_truncated) =
        select_stream_lines(stdout, output_mode, tail_limit, prefer_full);

    let stderr = payload.get("stderr").and_then(Value::as_str).unwrap_or("");
    let (stderr_lines, stderr_total, stderr_truncated) =
        select_stream_lines(stderr, output_mode, tail_limit, prefer_full);

    // Output section
    if !stdout_lines.is_empty() || !stderr_lines.is_empty() {
        render_table_separator(renderer, TABLE_WIDTH, MessageStyle::Info)?;
    }

    // Render stdout
    if !stdout_lines.is_empty() {
        for &line in stdout_lines.iter().take(10) {
            let truncated = if line.len() > TABLE_WIDTH - 6 {
                format!("{}…", &line[..TABLE_WIDTH - 7])
            } else {
                line.to_string()
            };
            render_table_row(renderer, &truncated, TABLE_WIDTH, MessageStyle::Info)?;
        }
        if stdout_truncated {
            let msg = format!("… {} more lines", stdout_total - stdout_lines.len());
            render_table_row(renderer, &msg, TABLE_WIDTH, MessageStyle::Info)?;
        }
    }

    // Render stderr
    if !stderr_lines.is_empty() {
        if !stdout_lines.is_empty() {
            render_table_separator(renderer, TABLE_WIDTH, MessageStyle::Error)?;
        }
        for &line in stderr_lines.iter().take(5) {
            let truncated = if line.len() > TABLE_WIDTH - 6 {
                format!("{}…", &line[..TABLE_WIDTH - 7])
            } else {
                line.to_string()
            };
            render_table_row(renderer, &truncated, TABLE_WIDTH, MessageStyle::Error)?;
        }
        if stderr_truncated {
            let msg = format!("… {} more lines", stderr_total - stderr_lines.len());
            render_table_row(renderer, &msg, TABLE_WIDTH, MessageStyle::Info)?;
        }
    }

    // No output indicator
    if stdout_lines.is_empty() && stderr_lines.is_empty() {
        render_table_row(renderer, "(no output)", TABLE_WIDTH, MessageStyle::Info)?;
    }

    render_table_bottom(renderer, TABLE_WIDTH, MessageStyle::Info)?;

    Ok(())
}

fn render_git_diff(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let _ = (mode, tail_limit);
    let addition_total = payload
        .get("addition_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let deletion_total = payload
        .get("deletion_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let staged = payload
        .get("staged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let file_count = payload
        .get("file_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    renderer.line(
        MessageStyle::Info,
        &format!(
            "  files: {} | +{} -{} | source: {}",
            file_count,
            addition_total,
            deletion_total,
            if staged { "staged" } else { "working tree" }
        ),
    )?;

    if let Some(files) = payload.get("files").and_then(|v| v.as_array()) {
        for file in files.iter().take(20) {
            let path = file
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("<unknown>");
            let status = file
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let summary = file.get("summary").and_then(|v| v.as_object());
            let additions = summary
                .and_then(|m| m.get("additions"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let deletions = summary
                .and_then(|m| m.get("deletions"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let previous = file.get("previous_path").and_then(|v| v.as_str());

            renderer.line(
                MessageStyle::Info,
                &match previous {
                    Some(old) if old != path => format!(
                        "    {} ({} | +{} -{} | was {})",
                        path, status, additions, deletions, old
                    ),
                    _ => format!("    {} ({} | +{} -{})", path, status, additions, deletions),
                },
            )?;
        }

        if files.len() > 20 {
            renderer.line(
                MessageStyle::Info,
                &format!("    … {} more files omitted", files.len() - 20),
            )?;
        }
    }

    if let Some(files) = payload.get("files").and_then(|v| v.as_array()) {
        for file in files.iter().take(3) {
            if let Some(formatted) = file.get("formatted").and_then(|v| v.as_str()) {
                if formatted.trim().is_empty() {
                    continue;
                }
                renderer.line(MessageStyle::Info, "")?;
                let mut current_old = None;
                let mut current_new = None;
                for line in formatted.lines() {
                    // Skip code fence markers
                    if line.trim() == "```" || line.trim().starts_with("```") {
                        continue;
                    }

                    if line.starts_with("@@") {
                        if let Some((old, new)) = parse_hunk_header(line) {
                            current_old = Some(old);
                            current_new = Some(new);
                        }
                    }
                    let style_opt =
                        select_line_style(Some(tools::GIT_DIFF), line, git_styles, ls_styles);
                    let (display, updated_old, updated_new) =
                        inject_line_numbers(line, current_old, current_new);
                    current_old = updated_old;
                    current_new = updated_new;
                    if let Some(style) = style_opt {
                        renderer.line_with_style(style, &display)?;
                    } else {
                        renderer.line(MessageStyle::Info, &display)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn parse_hunk_header(line: &str) -> Option<((usize, usize), (usize, usize))> {
    // Example: @@ -17,6 +17,7 @@
    let mut parts = line.split_whitespace();
    let _ = parts.next(); // @@
    let old = parts.next()?.trim_start_matches('-');
    let new = parts.next()?.trim_start_matches('+');
    Some((parse_range(old)?, parse_range(new)?))
}

fn parse_range(spec: &str) -> Option<(usize, usize)> {
    let mut parts = spec.split(',');
    let start = parts.next()?.parse::<usize>().ok()?;
    let len = parts
        .next()
        .map(|s| s.parse::<usize>().ok())
        .unwrap_or(Some(1))?;
    Some((start, len))
}

fn inject_line_numbers(
    line: &str,
    current_old: Option<(usize, usize)>,
    current_new: Option<(usize, usize)>,
) -> (String, Option<(usize, usize)>, Option<(usize, usize)>) {
    if line.starts_with("@@") {
        return (line.to_string(), current_old, current_new);
    }

    let mut old_state = current_old;
    let mut new_state = current_new;
    let mut prefix = String::new();

    match line.chars().next() {
        Some('+') => {
            let new_line = new_state.map(|(line_no, _)| line_no).unwrap_or(0);
            prefix.push_str(&format!("{:>5} |{:>5} | ", "", new_line));
            if let Some((line_no, remaining)) = new_state {
                new_state = Some((line_no + 1, remaining.saturating_sub(1)));
            }
        }
        Some('-') => {
            let old_line = old_state.map(|(line_no, _)| line_no).unwrap_or(0);
            prefix.push_str(&format!("{:>5} |{:>5} | ", old_line, ""));
            if let Some((line_no, remaining)) = old_state {
                old_state = Some((line_no + 1, remaining.saturating_sub(1)));
            }
        }
        _ => {
            let old_line = old_state.map(|(line_no, _)| line_no).unwrap_or(0);
            let new_line = new_state.map(|(line_no, _)| line_no).unwrap_or(0);
            prefix.push_str(&format!("{:>5} |{:>5} | ", old_line, new_line));
            if let Some((line_no, remaining)) = old_state {
                old_state = Some((line_no + 1, remaining.saturating_sub(1)));
            }
            if let Some((line_no, remaining)) = new_state {
                new_state = Some((line_no + 1, remaining.saturating_sub(1)));
            }
        }
    }

    (format!("{}{}", prefix, line), old_state, new_state)
}

fn render_curl_result(
    renderer: &mut AnsiRenderer,
    val: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
) -> Result<()> {
    const PREVIEW_LINE_MAX: usize = 120;
    const NOTICE_MAX: usize = 160;

    renderer.line(MessageStyle::Info, "[curl] HTTPS fetch summary")?;

    // URL
    if let Some(url) = val.get("url").and_then(Value::as_str) {
        renderer.line(MessageStyle::Response, &format!("  url: {url}"))?;
    }

    // Summary parts
    let mut summary_parts = Vec::new();

    if let Some(status) = val.get("status").and_then(Value::as_u64) {
        summary_parts.push(format!("status={status}"));
    }
    if let Some(content_type) = val.get("content_type").and_then(Value::as_str)
        && !content_type.is_empty()
    {
        summary_parts.push(format!("type={content_type}"));
    }
    if let Some(bytes_read) = val.get("bytes_read").and_then(Value::as_u64) {
        summary_parts.push(format!("bytes={bytes_read}"));
    } else if let Some(content_length) = val.get("content_length").and_then(Value::as_u64) {
        summary_parts.push(format!("bytes={content_length}"));
    }
    if val
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        summary_parts.push("body=truncated".to_string());
    }
    if let Some(saved_path) = val.get("saved_path").and_then(Value::as_str) {
        summary_parts.push(format!("saved={saved_path}"));
    }

    if !summary_parts.is_empty() {
        renderer.line(
            MessageStyle::Response,
            &format!("  {}", summary_parts.join(" | ")),
        )?;
    }

    // Notices
    if let Some(cleanup_hint) = val.get("cleanup_hint").and_then(Value::as_str) {
        renderer.line(
            MessageStyle::Info,
            &format!("  cleanup: {}", truncate_text(cleanup_hint, NOTICE_MAX)),
        )?;
    }
    if let Some(notice) = val.get("security_notice").and_then(Value::as_str) {
        renderer.line(
            MessageStyle::Info,
            &format!("  notice: {}", truncate_text(notice, NOTICE_MAX)),
        )?;
    }

    // Body
    if let Some(body) = val.get("body").and_then(Value::as_str)
        && !body.trim().is_empty()
    {
        let prefer_full = renderer.prefers_untruncated_output();
        let (lines, total, truncated) = select_stream_lines(body, mode, tail_limit, prefer_full);

        if !lines.is_empty() {
            if truncated {
                renderer.line(
                    MessageStyle::Info,
                    &format!("  ... showing last {}/{} body lines", lines.len(), total),
                )?;
            }

            renderer.line(
                MessageStyle::Info,
                &format!("[curl] body tail ({} lines)", lines.len()),
            )?;

            for line in lines {
                let trimmed = line.trim_end();
                renderer.line(
                    MessageStyle::Response,
                    &format!("  {}", truncate_text(trimmed, PREVIEW_LINE_MAX)),
                )?;
            }
        }
    }

    Ok(())
}

fn truncate_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }

    let mut truncated = text
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

struct GitStyles {
    add: Option<Style>,
    remove: Option<Style>,
    header: Option<Style>,
}

impl GitStyles {
    fn new() -> Self {
        Self {
            add: anstyle_git::parse("green").ok(),
            remove: anstyle_git::parse("red").ok(),
            header: anstyle_git::parse("bold yellow").ok(),
        }
    }
}

struct LsStyles {
    classes: HashMap<String, Style>,
    suffixes: Vec<(String, Style)>,
}

impl LsStyles {
    fn from_env() -> Self {
        let mut classes = HashMap::new();
        let mut suffixes = Vec::new();

        if let Ok(ls_colors) = std::env::var("LS_COLORS") {
            for part in ls_colors.split(':') {
                if let Some((key, value)) = part.split_once('=') {
                    if let Some(style) = anstyle_ls::parse(value) {
                        if let Some(pattern) = key.strip_prefix("*.") {
                            let extension = pattern.to_ascii_lowercase();
                            if !extension.is_empty() {
                                suffixes.push((format!(".{}", extension), style));
                            }
                        } else if !key.is_empty() {
                            classes.insert(key.to_string(), style);
                        }
                    }
                }
            }
        }

        if !classes.contains_key("di") {
            if let Some(style) = anstyle_ls::parse("01;34") {
                classes.insert("di".to_string(), style);
            }
        }
        if !classes.contains_key("ln") {
            if let Some(style) = anstyle_ls::parse("01;36") {
                classes.insert("ln".to_string(), style);
            }
        }
        if !classes.contains_key("ex") {
            if let Some(style) = anstyle_ls::parse("01;32") {
                classes.insert("ex".to_string(), style);
            }
        }
        if !classes.contains_key("pi") {
            if let Some(style) = anstyle_ls::parse("33") {
                classes.insert("pi".to_string(), style);
            }
        }
        if !classes.contains_key("so") {
            if let Some(style) = anstyle_ls::parse("01;35") {
                classes.insert("so".to_string(), style);
            }
        }
        if !classes.contains_key("bd") {
            if let Some(style) = anstyle_ls::parse("01;33") {
                classes.insert("bd".to_string(), style);
            }
        }
        if !classes.contains_key("cd") {
            if let Some(style) = anstyle_ls::parse("01;33") {
                classes.insert("cd".to_string(), style);
            }
        }

        suffixes.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        Self { classes, suffixes }
    }

    fn style_for_line(&self, line: &str) -> Option<Style> {
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
    fn from_components(classes: HashMap<String, Style>, suffixes: Vec<(String, Style)>) -> Self {
        Self { classes, suffixes }
    }
}

fn select_line_style(
    tool_name: Option<&str>,
    line: &str,
    git: &GitStyles,
    ls: &LsStyles,
) -> Option<Style> {
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
        let dir_style = Style::new().bold();
        let exec_style = Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)));
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
            Style::new().fg_color(Some(anstyle::AnsiColor::Red.into())),
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
            Style::new().fg_color(Some(anstyle::AnsiColor::Green.into())),
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
}
