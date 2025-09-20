use anstyle::Style;
use serde_json::Value;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(crate) fn render_tool_output(tool_name: Option<&str>, val: &Value) {
    let mut renderer = AnsiRenderer::stdout();
    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();
    if let Some(stdout) = val.get("stdout").and_then(|value| value.as_str())
        && !stdout.trim().is_empty()
    {
        let _ = renderer.line(MessageStyle::Tool, "[stdout]");
        for line in stdout.lines() {
            let indented = format!("  {}", line);
            if let Some(style) = select_line_style(tool_name, line, &git_styles, &ls_styles) {
                let _ = renderer.line_with_style(style, &indented);
            } else {
                let _ = renderer.line(MessageStyle::Output, &indented);
            }
        }
    }
    if let Some(stderr) = val.get("stderr").and_then(|value| value.as_str())
        && !stderr.trim().is_empty()
    {
        let _ = renderer.line(MessageStyle::Tool, "[stderr]");
        let formatted = stderr
            .lines()
            .map(|line| format!("  {}", line))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = renderer.line(MessageStyle::Error, &formatted);
    }
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
