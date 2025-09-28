use anstyle::Style;
use anyhow::{Context, Result};
use serde_json::Value;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::{defaults, tools};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::tools::{PlanCompletionState, StepStatus, TaskPlan};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(crate) fn render_tool_output(
    renderer: &mut AnsiRenderer,
    tool_name: Option<&str>,
    val: &Value,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    if tool_name == Some(tools::UPDATE_PLAN) {
        render_plan_update(renderer, val)?;
        return Ok(());
    }

    if let Some(notice) = val.get("security_notice").and_then(|value| value.as_str()) {
        renderer.line(MessageStyle::Info, notice)?;
    }

    if let Some(tool) = tool_name {
        if tool.starts_with("mcp_context7") {
            render_mcp_context7_output(renderer, val)?;
        } else if tool.starts_with("mcp_sequentialthinking") {
            render_mcp_sequential_output(renderer, val)?;
        }
    }

    if tool_name == Some(tools::CURL) {
        render_curl_result(renderer, val)?;
    }

    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();
    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);

    if let Some(stdout) = val.get("stdout").and_then(|value| value.as_str()) {
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
    if let Some(stderr) = val.get("stderr").and_then(|value| value.as_str()) {
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

    renderer.line(MessageStyle::Tool, "┏ sequential-thinking")?;
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

    renderer.line(MessageStyle::ToolDetail, "┗ sequential-thinking finished")?;
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
            StepStatus::Completed => "[x]",
        };
        let mut content = format!("    {:>2}. {} {}", index + 1, checkbox, step.step.trim());
        if matches!(step.status, StepStatus::InProgress) {
            content.push_str(" (in progress)");
        }
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

fn tail_lines<'a>(text: &'a str, limit: usize) -> (Vec<&'a str>, usize) {
    if limit == 0 {
        return (Vec::new(), text.lines().count());
    }

    let mut lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    if total == 0 {
        return (Vec::new(), 0);
    }
    if total <= limit {
        return (lines, total);
    }
    let start = total.saturating_sub(limit);
    let tail = lines.split_off(start);
    (tail, total)
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
    let (lines, total) = match mode {
        ToolOutputMode::Full => {
            let all: Vec<&str> = content.lines().collect();
            let total = all.len();
            (all, total)
        }
        ToolOutputMode::Compact => {
            let (tail, total) = tail_lines(content, tail_limit);
            (tail, total)
        }
    };

    if lines.is_empty() {
        return Ok(());
    }

    if matches!(mode, ToolOutputMode::Compact) && total > lines.len() {
        let summary_prefix = if is_mcp_tool { "" } else { "  " };
        renderer.line(
            MessageStyle::Info,
            &format!(
                "{summary_prefix}... showing last {}/{} {} lines",
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
            "".to_string()
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

fn render_curl_result(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    renderer.line(MessageStyle::Tool, "[curl] HTTPS fetch summary")?;

    if let Some(url) = val.get("url").and_then(|value| value.as_str()) {
        renderer.line(MessageStyle::Output, &format!("  URL: {url}"))?;
    }

    if let Some(status) = val.get("status").and_then(|value| value.as_u64()) {
        renderer.line(MessageStyle::Output, &format!("  Status: {status}"))?;
    }

    if let Some(content_type) = val.get("content_type").and_then(|value| value.as_str())
        && !content_type.is_empty()
    {
        renderer.line(
            MessageStyle::Output,
            &format!("  Content-Type: {content_type}"),
        )?;
    }

    if let Some(bytes_read) = val.get("bytes_read").and_then(|value| value.as_u64()) {
        renderer.line(MessageStyle::Output, &format!("  Bytes read: {bytes_read}"))?;
    } else if let Some(content_length) = val.get("content_length").and_then(|value| value.as_u64())
    {
        renderer.line(
            MessageStyle::Output,
            &format!("  Content length: {content_length}"),
        )?;
    }

    if val
        .get("truncated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        renderer.line(
            MessageStyle::Info,
            "  Body truncated to the configured policy limit.",
        )?;
    }

    if let Some(saved_path) = val.get("saved_path").and_then(|value| value.as_str()) {
        renderer.line(MessageStyle::Output, &format!("  Saved to: {saved_path}"))?;
    }

    if let Some(cleanup_hint) = val.get("cleanup_hint").and_then(|value| value.as_str()) {
        renderer.line(
            MessageStyle::Info,
            &format!("  Cleanup hint: {cleanup_hint}"),
        )?;
    }

    if let Some(notice) = val.get("security_notice").and_then(|value| value.as_str()) {
        renderer.line(MessageStyle::Info, &format!("  Security notice: {notice}"))?;
    }

    if let Some(body) = val.get("body").and_then(|value| value.as_str())
        && !body.trim().is_empty()
    {
        renderer.line(MessageStyle::Tool, "[curl] Body preview")?;
        for line in body.lines() {
            renderer.line(MessageStyle::Output, &format!("  {line}"))?;
        }
    }

    Ok(())
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

use std::collections::HashMap;

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
        Some("run_terminal_cmd") | Some("bash") => {
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
}
