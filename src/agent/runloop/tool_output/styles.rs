use std::collections::HashMap;

use anstyle::{AnsiColor, Color, Effects, Style as AnsiStyle};
use vtcode_commons::diff_paths::{
    is_diff_addition_line, is_diff_deletion_line, is_diff_header_line,
};
use vtcode_core::config::constants::tools;
use vtcode_core::utils::diff_styles::{DiffColorLevel, DiffTheme, diff_add_bg, diff_del_bg};
use vtcode_core::utils::style_helpers::bold_color;

/// Get background color for diff lines based on detected theme and color level.
fn diff_line_bg_color(is_addition: bool) -> Option<Color> {
    let theme = DiffTheme::detect();
    let level = DiffColorLevel::detect();
    let bg = if is_addition {
        diff_add_bg(theme, level)
    } else {
        diff_del_bg(theme, level)
    };
    Some(bg)
}

pub(crate) struct GitStyles {
    pub(crate) add: Option<AnsiStyle>,
    pub(crate) remove: Option<AnsiStyle>,
    pub(crate) header: Option<AnsiStyle>,
}

impl GitStyles {
    pub(crate) fn new() -> Self {
        // Use standard ANSI colors without bold - no theme dependency
        // Background colors adapt to terminal theme (dark/light)
        let remove_effects = Effects::DIMMED;
        Self {
            add: Some(
                AnsiStyle::new()
                    .fg_color(Some(Color::Ansi(AnsiColor::Green)))
                    .bg_color(diff_line_bg_color(true)),
            ),
            remove: Some(
                AnsiStyle::new()
                    .fg_color(Some(Color::Ansi(AnsiColor::Red)))
                    .bg_color(diff_line_bg_color(false))
                    .effects(remove_effects),
            ),
            header: Some(
                AnsiStyle::new()
                    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
                    .bg_color(None),
            ),
        }
    }
}

pub(crate) struct LsStyles {
    classes: HashMap<String, AnsiStyle>,
    suffixes: Vec<(String, AnsiStyle)>,
}

impl LsStyles {
    pub(crate) fn from_env() -> Self {
        let mut classes: HashMap<String, AnsiStyle> = HashMap::new();
        let suffixes: Vec<(String, AnsiStyle)> = Vec::new();

        classes.insert("di".to_string(), bold_color(AnsiColor::Blue));
        classes.insert("ln".to_string(), bold_color(AnsiColor::Cyan));
        classes.insert("ex".to_string(), bold_color(AnsiColor::Green));
        classes.insert("pi".to_string(), bold_color(AnsiColor::Yellow));
        classes.insert("so".to_string(), bold_color(AnsiColor::Magenta));
        classes.insert("bd".to_string(), bold_color(AnsiColor::Yellow));
        classes.insert("cd".to_string(), bold_color(AnsiColor::Yellow));

        LsStyles { classes, suffixes }
    }

    pub(crate) fn style_for_line(&self, line: &str) -> Option<AnsiStyle> {
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

        if let Some(code) = class_hint
            && let Some(style) = self.classes.get(code)
        {
            return Some(*style);
        }

        let lower = name
            .trim_matches(|c| matches!(c, '"' | ',' | ' ' | '\u{0009}'))
            .to_ascii_lowercase();
        for (suffix, style) in &self.suffixes {
            if lower.ends_with(suffix) {
                return Some(*style);
            }
        }

        if lower.ends_with('*')
            && let Some(style) = self.classes.get("ex")
        {
            return Some(*style);
        }

        None
    }

    #[cfg(test)]
    pub(crate) fn from_components(
        classes: HashMap<String, AnsiStyle>,
        suffixes: Vec<(String, AnsiStyle)>,
    ) -> Self {
        Self { classes, suffixes }
    }
}

pub(crate) fn select_line_style(
    tool_name: Option<&str>,
    line: &str,
    git: &GitStyles,
    ls: &LsStyles,
) -> Option<AnsiStyle> {
    let trimmed = line.trim_start();
    // Always detect and style diff lines, even when tool_name is not provided
    // (e.g. git_diff payloads routed through generic rendering path).
    if is_diff_header_line(trimmed) {
        return git.header;
    }
    if is_diff_addition_line(trimmed) {
        return git.add;
    }
    if is_diff_deletion_line(trimmed) {
        return git.remove;
    }

    if let Some(
        tools::UNIFIED_EXEC
        | tools::RUN_PTY_CMD
        | tools::EXECUTE_CODE
        | tools::EXEC_PTY_CMD
        | tools::EXEC
        | tools::SHELL
        | tools::WRITE_FILE
        | tools::EDIT_FILE
        | tools::APPLY_PATCH,
    ) = tool_name
        && let Some(style) = ls.style_for_line(trimmed)
    {
        return Some(style);
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
        let added = select_line_style(Some("run_pty_cmd"), "+added line", &git, &ls);
        assert_eq!(added, git.add);
        let removed = select_line_style(Some("run_pty_cmd"), "-removed line", &git, &ls);
        assert_eq!(removed, git.remove);
        let header = select_line_style(Some("run_pty_cmd"), "diff --git a/file b/file", &git, &ls);
        assert_eq!(header, git.header);
    }

    #[test]
    fn removed_diff_lines_are_dimmed_relative_to_additions() {
        let git = GitStyles::new();
        let remove_effects = git.remove.expect("remove style should exist").get_effects();
        let add_effects = git.add.expect("add style should exist").get_effects();
        assert!(remove_effects.contains(Effects::DIMMED));
        assert!(!add_effects.contains(Effects::DIMMED));
    }

    #[test]
    fn detects_ls_styles_for_directories_and_executables() {
        let git = GitStyles::new();
        use vtcode_core::utils::style_helpers::bold_color;
        let dir_style = bold_color(AnsiColor::Blue);
        let exec_style = bold_color(AnsiColor::Green);
        let mut classes = HashMap::new();
        classes.insert("di".to_string(), dir_style);
        classes.insert("ex".to_string(), exec_style);
        let ls = LsStyles::from_components(classes, Vec::new());
        let directory = select_line_style(Some("run_pty_cmd"), "folder/", &git, &ls);
        assert_eq!(directory, Some(dir_style));
        let executable = select_line_style(Some("run_pty_cmd"), "script*", &git, &ls);
        assert_eq!(executable, Some(exec_style));
    }

    #[test]
    fn non_terminal_tools_do_not_apply_special_styles() {
        let git = GitStyles::new();
        let ls = LsStyles::from_components(HashMap::new(), Vec::new());
        let styled = select_line_style(Some("context7"), "+added", &git, &ls);
        assert_eq!(styled, git.add);
    }

    #[test]
    fn diff_styling_works_without_tool_name() {
        let git = GitStyles::new();
        let ls = LsStyles::from_components(HashMap::new(), Vec::new());
        let header = select_line_style(None, "diff --git a/file b/file", &git, &ls);
        assert_eq!(header, git.header);
        let added = select_line_style(None, "+added", &git, &ls);
        assert_eq!(added, git.add);
    }

    #[test]
    fn applies_extension_based_styles() {
        let git = GitStyles::new();
        use vtcode_core::utils::style_helpers::bold_color;
        let suffixes = vec![(".rs".to_string(), bold_color(AnsiColor::Red))];
        let ls = LsStyles::from_components(HashMap::new(), suffixes);
        let styled = select_line_style(Some("run_pty_cmd"), "main.rs", &git, &ls);
        assert!(styled.is_some());
    }

    #[test]
    fn extension_matching_requires_dot_boundary() {
        let git = GitStyles::new();
        use vtcode_core::utils::style_helpers::bold_color;
        let suffixes = vec![(".rs".to_string(), bold_color(AnsiColor::Green))];
        let ls = LsStyles::from_components(HashMap::new(), suffixes);

        let without_extension = select_line_style(Some("run_pty_cmd"), "helpers", &git, &ls);
        assert!(without_extension.is_none());

        let with_extension = select_line_style(Some("run_pty_cmd"), "helpers.rs", &git, &ls);
        assert!(with_extension.is_some());
    }
}
