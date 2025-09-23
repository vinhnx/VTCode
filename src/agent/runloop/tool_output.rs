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

struct LsStyles {
    dir: Option<Style>,
    exec: Option<Style>,
}

impl LsStyles {
    fn from_env() -> Self {
        let mut styles = Self {
            dir: None,
            exec: None,
        };
        if let Ok(ls_colors) = std::env::var("LS_COLORS") {
            for part in ls_colors.split(':') {
                if let Some((key, value)) = part.split_once('=') {
                    match key {
                        "di" => styles.dir = anstyle_ls::parse(value),
                        "ex" => styles.exec = anstyle_ls::parse(value),
                        _ => {}
                    }
                }
            }
        }
        styles
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

            let cleaned = trimmed.trim_end();
            if cleaned.ends_with('/') {
                return ls.dir;
            }
            if cleaned.ends_with('*') {
                return ls.exec;
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
        let ls = LsStyles {
            dir: None,
            exec: None,
        };
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
        let ls = LsStyles {
            dir: Some(dir_style),
            exec: Some(exec_style),
        };
        let directory = select_line_style(Some("run_terminal_cmd"), "folder/", &git, &ls);
        assert_eq!(directory, Some(dir_style));
        let executable = select_line_style(Some("run_terminal_cmd"), "script*", &git, &ls);
        assert_eq!(executable, Some(exec_style));
    }

    #[test]
    fn non_terminal_tools_do_not_apply_special_styles() {
        let git = GitStyles::new();
        let ls = LsStyles {
            dir: None,
            exec: None,
        };
        let styled = select_line_style(Some("context7"), "+added", &git, &ls);
        assert!(styled.is_none());
    }
}
