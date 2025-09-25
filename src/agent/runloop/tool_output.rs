use anstyle::Style;
use anyhow::{Context, Result};
use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::{PlanCompletionState, TaskPlan};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(crate) fn render_tool_output(
    renderer: &mut AnsiRenderer,
    tool_name: Option<&str>,
    val: &Value,
) -> Result<()> {
    if tool_name == Some(tools::UPDATE_PLAN) {
        render_plan_update(renderer, val)?;
        return Ok(());
    }

    if tool_name == Some(tools::CURL) {
        render_curl_result(renderer, val)?;
    } else if let Some(notice) = val.get("security_notice").and_then(|value| value.as_str()) {
        renderer.line(MessageStyle::Info, notice)?;
    }

    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();
    if let Some(stdout) = val.get("stdout").and_then(|value| value.as_str())
        && !stdout.trim().is_empty()
    {
        for line in stdout.lines() {
            let indented = format!("  {}", line);
            if let Some(style) = select_line_style(tool_name, line, &git_styles, &ls_styles) {
                renderer.line_with_style(style, &indented)?;
            } else {
                renderer.line(MessageStyle::Output, &indented)?;
            }
        }
    }
    if let Some(stderr) = val.get("stderr").and_then(|value| value.as_str())
        && !stderr.trim().is_empty()
    {
        renderer.line(MessageStyle::Tool, "[stderr]")?;
        let formatted = stderr
            .lines()
            .map(|line| format!("  {}", line))
            .collect::<Vec<_>>()
            .join("\n");
        renderer.line(MessageStyle::Error, &formatted)?;
    }
    Ok(())
}

fn render_plan_update(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    let plan_value = val
        .get("plan")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Plan tool output missing 'plan' field"))?;
    let plan: TaskPlan =
        serde_json::from_value(plan_value).context("Plan tool returned malformed plan payload")?;
    let message = val
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("Task plan updated");

    renderer.line(MessageStyle::Tool, &format!("[plan] {}", message))?;
    renderer.line(
        MessageStyle::Output,
        &format!(
            "  Version {} · updated {}",
            plan.version,
            plan.updated_at.to_rfc3339()
        ),
    )?;

    match plan.summary.status {
        PlanCompletionState::Empty => {
            renderer.line(
                MessageStyle::Info,
                "  No TODO items recorded. Use update_plan to add tasks.",
            )?;
        }
        _ => {
            renderer.line(
                MessageStyle::Output,
                &format!(
                    "  Progress: {}/{} completed · {}",
                    plan.summary.completed_steps,
                    plan.summary.total_steps,
                    plan.summary.status.description()
                ),
            )?;
        }
    }

    if let Some(explanation) = plan.explanation.as_ref() {
        renderer.line(
            MessageStyle::Output,
            &format!("  Explanation: {}", explanation),
        )?;
    }

    for step in plan.steps.iter() {
        let mut line = format!("  - {} {}", step.status.checkbox(), step.step);
        if let Some(note) = step.status.status_note() {
            line.push_str(note);
        }
        renderer.line(MessageStyle::Output, &line)?;
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
