use anstyle::{AnsiColor, Color, Style};
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
            MessageStyle::Output,
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
    let heading = if val.get("error").is_some() {
        val.get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("Plan update failed")
    } else {
        val.get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("Task plan updated")
    };

    renderer.line(MessageStyle::Tool, &format!("[plan] {}", heading))?;

    if let Some(error) = val.get("error") {
        render_plan_error(renderer, error)?;
        return Ok(());
    }

    let plan_value = match val.get("plan").cloned() {
        Some(value) => value,
        None => {
            renderer.line(
                MessageStyle::Error,
                "  Plan update response did not include a plan payload.",
            )?;
            return Ok(());
        }
    };

    let plan: TaskPlan =
        serde_json::from_value(plan_value).context("Plan tool returned malformed plan payload")?;

    renderer.line(
        MessageStyle::Output,
        &format!(
            "  Version {} · updated {}",
            plan.version,
            plan.updated_at.to_rfc3339()
        ),
    )?;

    if matches!(plan.summary.status, PlanCompletionState::Empty) {
        renderer.line(
            MessageStyle::Info,
            "  No TODO items recorded. Use update_plan to add tasks.",
        )?;
        if let Some(explanation) = plan.explanation.as_ref() {
            let trimmed = explanation.trim();
            if !trimmed.is_empty() {
                renderer.line(MessageStyle::Info, &format!("  Note: {}", trimmed))?;
            }
        }
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

    let meta = val.get("meta").and_then(|value| value.as_object());
    let provider = val
        .get("provider")
        .and_then(|value| value.as_str())
        .unwrap_or("context7");
    let tool_used = val
        .get("tool")
        .and_then(|value| value.as_str())
        .unwrap_or("context7");

    renderer.line(
        MessageStyle::Tool,
        &format!("[{}:{}] status: {}", provider, tool_used, status),
    )?;

    if let Some(meta) = meta {
        if let Some(query) = meta.get("query").and_then(|value| value.as_str()) {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("┇ query: {}", shorten(query, 160)),
            )?;
        }
        if let Some(scope) = meta.get("scope").and_then(|value| value.as_str()) {
            renderer.line(MessageStyle::ToolDetail, &format!("┇ scope: {}", scope))?;
        }
        if let Some(limit) = meta.get("max_results").and_then(|value| value.as_u64()) {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("┇ max_results: {}", limit),
            )?;
        }
    }

    if let Some(messages) = val.get("messages").and_then(|value| value.as_array())
        && !messages.is_empty()
    {
        renderer.line(MessageStyle::ToolDetail, "┇ snippets:")?;
        for message in messages.iter().take(3) {
            if let Some(content) = message.get("content").and_then(|value| value.as_str()) {
                renderer.line(
                    MessageStyle::ToolDetail,
                    &format!("┇ · {}", shorten(content, 200)),
                )?;
            }
        }
        if messages.len() > 3 {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("┇ · … {} more", messages.len() - 3),
            )?;
        }
    }

    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        renderer.line(MessageStyle::Error, "┇ provider errors:")?;
        for err in errors.iter().take(2) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::Error, &format!("┇ · {}", shorten(msg, 160)))?;
            }
        }
        if errors.len() > 2 {
            renderer.line(
                MessageStyle::Error,
                &format!("┇ · … {} more", errors.len() - 2),
            )?;
        }
    }

    if let Some(input) = val.get("input").and_then(|value| value.as_object())
        && let Some(name) = input.get("LibraryName").and_then(|value| value.as_str())
    {
        let candidate = name.trim();
        if !candidate.is_empty() {
            let lowered = candidate.to_lowercase();
            if lowered != "tokio" && levenshtein(&lowered, "tokio") <= 2 {
                renderer.line(MessageStyle::Info, "┇ suggestion: did you mean 'tokio'?")?;
            }
        }
    }

    renderer.line(MessageStyle::ToolDetail, "┗ context7 lookup complete")?;
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
    let events = val.get("events").and_then(|value| value.as_array());
    let has_errors = val
        .get("errors")
        .and_then(|value| value.as_array())
        .is_some_and(|errors| !errors.is_empty());

    let base_style = sequential_tool_status_style(status, has_errors);
    let header_style = base_style.bold();

    renderer.line_with_style(header_style, "  ┏ sequential-thinking")?;

    renderer.line(MessageStyle::ToolDetail, &format!("┇ status: {}", status))?;
    renderer.line(
        MessageStyle::ToolDetail,
        &format!("┇ summary: {}", shorten(summary, 200)),
    )?;

    if let Some(events) = events {
        renderer.line(MessageStyle::ToolDetail, "┇ trace:")?;
        for event in events.iter().take(5) {
            if let Some(kind) = event.get("type").and_then(|value| value.as_str())
                && let Some(content) = event.get("content").and_then(|value| value.as_str())
            {
                renderer.line(
                    MessageStyle::ToolDetail,
                    &format!("┇ · [{}] {}", kind, shorten(content, 160)),
                )?;
            }
        }
        if events.len() > 5 {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("┇ · … {} more", events.len() - 5),
            )?;
        }
    }

    if let Some(errors) = val.get("errors").and_then(|value| value.as_array())
        && !errors.is_empty()
    {
        renderer.line(MessageStyle::Error, "┇ errors:")?;
        for err in errors.iter().take(3) {
            if let Some(msg) = err.get("message").and_then(|value| value.as_str()) {
                renderer.line(MessageStyle::Error, &format!("┇ · {}", shorten(msg, 160)))?;
            }
        }
        if errors.len() > 3 {
            renderer.line(
                MessageStyle::Error,
                &format!("┇ · … {} more", errors.len() - 3),
            )?;
        }
    }

    renderer.line_with_style(base_style, "  ┗ sequential-thinking finished")?;
    Ok(())
}

fn sequential_tool_status_style(status: &str, has_errors: bool) -> Style {
    if has_errors || is_failure_status(status) {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)))
    } else if is_success_status(status) {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)))
    } else {
        Style::new().fg_color(Some(Color::Ansi(AnsiColor::Blue)))
    }
}

fn is_failure_status(status: &str) -> bool {
    let normalized = status.trim().to_ascii_lowercase();
    normalized.contains("fail")
        || normalized.contains("error")
        || normalized.contains("cancel")
        || normalized.contains("timeout")
        || normalized.contains("abort")
}

fn is_success_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "ok" | "okay" | "success" | "succeeded" | "completed" | "complete" | "done" | "finished"
    )
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

fn levenshtein(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut current = vec![0; b_len + 1];

    for (i, a_ch) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, b_ch) in b.chars().enumerate() {
            let cost = if a_ch == b_ch { 0 } else { 1 };
            current[j + 1] = std::cmp::min(
                std::cmp::min(current[j] + 1, prev[j + 1] + 1),
                prev[j] + cost,
            );
        }
        prev.copy_from_slice(&current);
    }

    prev[b_len]
}

fn resolve_mcp_renderer_profile(
    tool_name: &str,
    vt_config: Option<&VTCodeConfig>,
) -> Option<McpRendererProfile> {
    let config = vt_config?;
    config.mcp.ui.renderer_for_tool(tool_name)
}

fn render_plan_panel(renderer: &mut AnsiRenderer, plan: &TaskPlan) -> Result<()> {
    renderer.line(
        MessageStyle::Tool,
        &format!(
            "  Todo List · {}/{} done · {}",
            plan.summary.completed_steps,
            plan.summary.total_steps,
            plan.summary.status.description()
        ),
    )?;

    renderer.line(
        MessageStyle::Output,
        &format!(
            "  Progress: {}/{} completed",
            plan.summary.completed_steps, plan.summary.total_steps
        ),
    )?;

    if let Some(explanation) = plan.explanation.as_ref() {
        for (index, line) in explanation
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .enumerate()
        {
            if index == 0 {
                renderer.line(MessageStyle::Info, &format!("  Note: {}", line))?;
            } else {
                renderer.line(MessageStyle::Info, &format!("        {}", line))?;
            }
        }
    }

    if !plan.steps.is_empty() {
        renderer.line(MessageStyle::Tool, "  Steps:")?;
    }

    for (index, step) in plan.steps.iter().enumerate() {
        let checkbox = match step.status {
            StepStatus::Pending => "[ ]",
            StepStatus::InProgress => "[>]",
            StepStatus::Completed => "[✓]",
        };
        let step_text = step.step.trim();
        let step_number = index + 1;
        let content = match step.status {
            StepStatus::Completed => {
                // ANSI strikethrough: \x1b[9m for strikethrough, \x1b[29m to reset
                format!("    {step_number}. {checkbox} \x1b[9m{step_text}\x1b[29m")
            }
            StepStatus::InProgress => {
                format!("    {step_number}. {checkbox} {step_text} (in progress)")
            }
            StepStatus::Pending => {
                format!("    {step_number}. {checkbox} {step_text}")
            }
        };
        renderer.line(MessageStyle::Output, &content)?;
    }

    renderer.line(MessageStyle::Output, "")?;
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

    renderer.line(MessageStyle::Tool, &format!("[write_file] {path}"))?;
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
        renderer.line(MessageStyle::Tool, "[diff]")?;
        for line in diff_content.lines() {
            let display = format!("  {line}");
            if let Some(style) =
                select_line_style(Some(tools::WRITE_FILE), line, git_styles, ls_styles)
            {
                renderer.line_with_style(style, &display)?;
            } else {
                renderer.line(MessageStyle::Output, &display)?;
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
        renderer.line(MessageStyle::Tool, &format!("[{}]", title.to_uppercase()))?;
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
        rows.push(CommandPanelRow::new(header, MessageStyle::Tool));
        rows.push(CommandPanelRow::blank(MessageStyle::Output));

        if block.lines.is_empty() {
            rows.push(CommandPanelRow::new(
                "    (no content)".to_string(),
                MessageStyle::Info,
            ));
        } else {
            for line in &block.lines {
                let display = format!("    {}", line);
                rows.push(CommandPanelRow::new(display, MessageStyle::Output));
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
            renderer.line(MessageStyle::Output, "")?;
        }
    }

    Ok(())
}

fn render_terminal_command_panel(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let output_mode = ToolOutputMode::Compact;
    let tail_limit = defaults::DEFAULT_PTY_STDOUT_TAIL_LINES;
    let prefer_full = renderer.prefers_untruncated_output();
    let command_display = payload
        .get("command")
        .and_then(|value| value.as_str())
        .unwrap_or("(command)");
    let description = payload
        .get("description")
        .and_then(|value| value.as_str())
        .or_else(|| payload.get("summary").and_then(|value| value.as_str()));
    let success = payload
        .get("success")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let exit_code = payload.get("exit_code").and_then(|value| value.as_i64());
    let shell_label = match payload.get("mode").and_then(|value| value.as_str()) {
        Some("pty") => "PTY",
        _ => "Command",
    };

    let mut summary = format!(
        "{}  {} {}",
        if success { "✓" } else { "✗" },
        shell_label,
        command_display
    );

    if let Some(desc) = description {
        let trimmed = desc.trim();
        if !trimmed.is_empty() {
            if trimmed.starts_with('(') {
                summary.push(' ');
                summary.push_str(trimmed);
            } else {
                summary.push(' ');
                summary.push('(');
                summary.push_str(trimmed);
                summary.push(')');
            }
        }
    }

    if !success {
        if let Some(code) = exit_code {
            summary.push_str(&format!(" (exit {code})"));
        }
    }

    let stdout = payload.get("stdout").and_then(Value::as_str).unwrap_or("");
    let (stdout_lines, stdout_total, stdout_truncated) =
        select_stream_lines(stdout, output_mode, tail_limit, prefer_full);

    let stderr = payload.get("stderr").and_then(Value::as_str).unwrap_or("");
    let (stderr_lines, stderr_total, stderr_truncated) =
        select_stream_lines(stderr, output_mode, tail_limit, prefer_full);

    let mut rows = Vec::new();
    let header_style = if success {
        MessageStyle::Status
    } else {
        MessageStyle::Error
    };
    rows.push(CommandPanelRow::new(summary, header_style));

    let mut has_output = false;

    if !stdout_lines.is_empty() {
        rows.push(CommandPanelRow::blank(MessageStyle::Output));
        if !stderr_lines.is_empty() {
            rows.push(CommandPanelRow::new(
                "stdout:".to_string(),
                MessageStyle::Output,
            ));
        }
        for &line in stdout_lines.iter() {
            let display = format!("    {line}");
            if let Some(style) =
                select_line_style(Some(tools::RUN_TERMINAL_CMD), line, git_styles, ls_styles)
            {
                rows.push(CommandPanelRow::with_override(
                    display,
                    MessageStyle::Output,
                    style,
                ));
            } else {
                rows.push(CommandPanelRow::new(display, MessageStyle::Output));
            }
        }
        if stdout_truncated {
            rows.push(CommandPanelRow::new(
                format!(
                    "    … showing last {}/{} stdout lines",
                    stdout_lines.len(),
                    stdout_total
                ),
                MessageStyle::Info,
            ));
        }
        has_output = true;
    }

    if !stderr_lines.is_empty() {
        rows.push(CommandPanelRow::blank(MessageStyle::Output));
        rows.push(CommandPanelRow::new(
            "stderr:".to_string(),
            MessageStyle::Error,
        ));
        for &line in stderr_lines.iter() {
            let display = format!("    {line}");
            rows.push(CommandPanelRow::new(display, MessageStyle::Error));
        }
        if stderr_truncated {
            rows.push(CommandPanelRow::new(
                format!(
                    "    … showing last {}/{} stderr lines",
                    stderr_lines.len(),
                    stderr_total
                ),
                MessageStyle::Info,
            ));
        }
        has_output = true;
    }

    if !has_output {
        rows.push(CommandPanelRow::blank(MessageStyle::Output));
        rows.push(CommandPanelRow::new(
            "    (no output)".to_string(),
            MessageStyle::Info,
        ));
    }

    let panel_lines = build_command_panel_display(rows);

    for line in panel_lines {
        if let Some(style) = line.override_style {
            renderer.line_with_override_style(line.style, style, &line.display)?;
        } else {
            renderer.line(line.style, &line.display)?;
        }
    }

    let follow_message = build_terminal_followup_message(command_display, success, exit_code);
    renderer.line(MessageStyle::Output, "")?;
    renderer.line(MessageStyle::Response, &follow_message)?;

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
                renderer.line(MessageStyle::Response, "")?;
                let mut current_old = None;
                let mut current_new = None;
                for line in formatted.lines() {
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
                        renderer.line(MessageStyle::Response, &display)?;
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

const TERMINAL_FOLLOWUP_LABEL_MAX: usize = 80;

fn build_terminal_followup_message(
    command_display: &str,
    success: bool,
    exit_code: Option<i64>,
) -> String {
    let mut normalized = String::new();
    for segment in command_display.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(segment);
    }

    let collapsed = if normalized.is_empty() {
        "(command)".to_string()
    } else {
        shorten(&normalized, TERMINAL_FOLLOWUP_LABEL_MAX)
    };

    if success {
        format!("Absorbed terminal output for `{}`.", collapsed)
    } else if let Some(code) = exit_code {
        format!("Captured `{}` output (exit code {}).", collapsed, code)
    } else {
        format!("Captured `{}` output for review.", collapsed)
    }
}

fn render_curl_result(
    renderer: &mut AnsiRenderer,
    val: &Value,
    mode: ToolOutputMode,
    tail_limit: usize,
) -> Result<()> {
    const PREVIEW_LINE_MAX: usize = 120;
    const NOTICE_MAX: usize = 160;

    renderer.line(MessageStyle::Tool, "[curl] HTTPS fetch summary")?;

    // URL
    if let Some(url) = val.get("url").and_then(Value::as_str) {
        renderer.line(MessageStyle::Output, &format!("  url: {url}"))?;
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
            MessageStyle::Output,
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
                MessageStyle::Tool,
                &format!("[curl] body tail ({} lines)", lines.len()),
            )?;

            for line in lines {
                let trimmed = line.trim_end();
                renderer.line(
                    MessageStyle::Output,
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
