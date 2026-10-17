use hashbrown::HashMap;

use anstyle::{AnsiColor, Color, Style as AnsiStyle};
use vtcode_commons::diff_paths::{is_diff_addition_line, is_diff_deletion_line, is_diff_header_line};
use vtcode_commons::diff_theme::{diff_add_fg, diff_del_fg, diff_gutter_fg};
use vtcode_core::config::constants::tools;
use vtcode_core::tools::tool_intent;
use vtcode_core::utils::diff_styles::{
    DiffColorLevel, DiffTheme, diff_add_bg, diff_add_word_bg, diff_del_bg, diff_del_word_bg,
};
use vtcode_core::utils::style_helpers::bold_color;

pub(crate) struct GitStyles {
    pub(crate) add: Option<AnsiStyle>,
    pub(crate) remove: Option<AnsiStyle>,
    pub(crate) header: Option<AnsiStyle>,
    pub(crate) file_old: Option<AnsiStyle>,
    pub(crate) file_new: Option<AnsiStyle>,
    pub(crate) hunk: Option<AnsiStyle>,
    /// Stronger addition background for changed intraline spans.
    pub(crate) add_word: Option<AnsiStyle>,
    /// Stronger deletion background for changed intraline spans.
    pub(crate) remove_word: Option<AnsiStyle>,
    /// Accessible addition marker foreground for the active terminal level.
    pub(crate) addition_fg: Color,
    /// Accessible deletion marker foreground for the active terminal level.
    pub(crate) deletion_fg: Color,
    /// Accessible gutter foreground for the active terminal level.
    pub(crate) gutter_fg: Color,
}

impl GitStyles {
    pub(crate) fn new() -> Self {
        Self::new_for(DiffTheme::detect(), DiffColorLevel::detect())
    }

    pub(crate) fn new_for(theme: DiffTheme, level: DiffColorLevel) -> Self {
        let addition_bg = (level != DiffColorLevel::Ansi16).then(|| diff_add_bg(theme, level));
        let deletion_bg = (level != DiffColorLevel::Ansi16).then(|| diff_del_bg(theme, level));
        let addition_fg = diff_add_fg(theme, level);
        let deletion_fg = diff_del_fg(theme, level);
        let gutter_fg = diff_gutter_fg(theme, level);

        let body_style = |is_addition: bool| {
            let fg = if is_addition { addition_fg } else { deletion_fg };
            if level == DiffColorLevel::Ansi16 {
                AnsiStyle::new().fg_color(Some(fg))
            } else {
                AnsiStyle::new().bg_color(if is_addition { addition_bg } else { deletion_bg })
            }
        };
        let word_style = |is_addition: bool| {
            let bg = if is_addition {
                diff_add_word_bg(theme, level)
            } else {
                diff_del_word_bg(theme, level)
            };
            (level != DiffColorLevel::Ansi16).then(|| AnsiStyle::new().bg_color(Some(bg)))
        };
        Self {
            add: Some(body_style(true)),
            remove: Some(body_style(false)),
            header: Some(
                AnsiStyle::new()
                    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
                    .effects(anstyle::Effects::BOLD),
            ),
            file_old: Some(AnsiStyle::new().fg_color(Some(deletion_fg)).effects(anstyle::Effects::BOLD)),
            file_new: Some(AnsiStyle::new().fg_color(Some(addition_fg)).effects(anstyle::Effects::BOLD)),
            hunk: Some(
                AnsiStyle::new()
                    .fg_color(Some(Color::Ansi(AnsiColor::Cyan)))
                    .effects(anstyle::Effects::BOLD),
            ),
            add_word: word_style(true),
            remove_word: word_style(false),
            addition_fg,
            deletion_fg,
            gutter_fg,
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

        let token = trimmed.split_whitespace().last().unwrap_or(trimmed).trim_matches('"');

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
    fn from_components(classes: HashMap<String, AnsiStyle>, suffixes: Vec<(String, AnsiStyle)>) -> Self {
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
    // File headers get red/green foregrounds, `@@` hunks get a bold cyan
    // foreground, and other metadata (`diff --git`, `index`, ...) stays dim
    // with no band.
    if trimmed.starts_with("--- ") {
        return git.file_old;
    }
    if trimmed.starts_with("+++ ") {
        return git.file_new;
    }
    if trimmed.starts_with("@@") {
        return git.hunk.or(git.header);
    }
    if is_diff_header_line(trimmed) {
        return git.header;
    }
    if is_diff_addition_line(trimmed) {
        return git.add;
    }
    if is_diff_deletion_line(trimmed) {
        return git.remove;
    }

    if tool_name.is_some_and(|name| {
        tool_intent::is_command_session_tool(name)
            || matches!(name, tools::WRITE_FILE | tools::EDIT_FILE | tools::APPLY_PATCH)
    }) && let Some(style) = ls.style_for_line(trimmed)
    {
        return Some(style);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::Effects;

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
    fn diff_content_styles_use_row_tint_and_ansi16_fallback() {
        let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let remove = git.remove.expect("remove style should exist");
        let add = git.add.expect("add style should exist");
        // DIMMED red is unreadable; the row tint carries the body and the
        // marker remains bright.
        assert!(!remove.get_effects().contains(Effects::DIMMED));
        assert!(!add.get_effects().contains(Effects::DIMMED));
        assert_eq!(remove.get_fg_color(), None);
        assert_eq!(add.get_fg_color(), None);
        assert_eq!(remove.get_bg_color(), Some(diff_del_bg(DiffTheme::Dark, DiffColorLevel::TrueColor)));
        assert_eq!(add.get_bg_color(), Some(diff_add_bg(DiffTheme::Dark, DiffColorLevel::TrueColor)));

        let ansi16 = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::Ansi16);
        let ansi16_remove = ansi16.remove.expect("ansi16 removal style");
        let ansi16_add = ansi16.add.expect("ansi16 addition style");
        assert_eq!(ansi16_remove.get_fg_color(), Some(Color::Ansi(AnsiColor::BrightRed)));
        assert_eq!(ansi16_add.get_fg_color(), Some(Color::Ansi(AnsiColor::BrightGreen)));
        assert_eq!(ansi16_remove.get_bg_color(), None);
        assert_eq!(ansi16.add_word, None);
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
    fn file_and_hunk_headers_keep_metadata_separate_from_row_tints() {
        let git = GitStyles::new_for(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let ls = LsStyles::from_components(HashMap::new(), Vec::new());
        assert_eq!(select_line_style(None, "--- a/README.md", &git, &ls), git.file_old);
        assert_eq!(select_line_style(None, "+++ b/README.md", &git, &ls), git.file_new);
        assert_eq!(select_line_style(None, "@@ -100 +100 @@", &git, &ls), git.hunk);
        for style in [git.file_old, git.file_new, git.hunk] {
            let style = style.expect("header style exists");
            assert_eq!(style.get_bg_color(), None);
            assert!(style.get_effects().contains(Effects::BOLD));
        }
        let header = git.header.expect("metadata header exists");
        assert_eq!(header.get_bg_color(), None);
        let add_word = git.add_word.expect("addition word style");
        let remove_word = git.remove_word.expect("deletion word style");
        assert_eq!(add_word.get_bg_color(), Some(diff_add_word_bg(DiffTheme::Dark, DiffColorLevel::TrueColor)));
        assert_eq!(remove_word.get_bg_color(), Some(diff_del_word_bg(DiffTheme::Dark, DiffColorLevel::TrueColor)));
    }

    #[test]
    fn light_diff_headers_use_accessible_shared_foregrounds() {
        let git = GitStyles::new_for(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert_eq!(
            git.file_old.expect("old file header").get_fg_color(),
            Some(diff_del_fg(DiffTheme::Light, DiffColorLevel::TrueColor))
        );
        assert_eq!(
            git.file_new.expect("new file header").get_fg_color(),
            Some(diff_add_fg(DiffTheme::Light, DiffColorLevel::TrueColor))
        );
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
