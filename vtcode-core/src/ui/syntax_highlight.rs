#[cfg(feature = "tui")]
pub use vtcode_tui::ui::syntax_highlight::{
    available_themes, default_theme_name, find_syntax_by_extension, find_syntax_by_name,
    find_syntax_by_token, find_syntax_plain_text, highlight_code_to_ansi,
    highlight_code_to_anstyle_line_segments, highlight_code_to_line_segments,
    highlight_code_to_segments, highlight_line_for_diff, highlight_line_to_anstyle_segments,
    load_theme, should_highlight, syntax_set,
};

use crate::ui::theme::get_syntax_theme_for_ui_theme;

// ── Headless stubs ──────────────────────────────────────────────────────────
//
// When the TUI crate is not compiled in, syntax highlighting is unavailable.
// Every function returns a no-op / passthrough result so callers compile
// without cfg-gating each call site.

#[cfg(not(feature = "tui"))]
mod headless_highlight {
    use anstyle::Style;

    pub fn available_themes() -> Vec<&'static str> {
        vec!["plain"]
    }
    pub fn default_theme_name() -> &'static str {
        "plain"
    }
    pub fn find_syntax_by_extension(_ext: &str) -> Option<&'static str> {
        None
    }
    pub fn find_syntax_by_name(_name: &str) -> Option<&'static str> {
        None
    }
    pub fn find_syntax_by_token(_token: &str) -> Option<&'static str> {
        None
    }
    pub fn find_syntax_plain_text() -> &'static str {
        "text"
    }
    pub fn highlight_code_to_ansi(code: &str, _language: Option<&str>, _theme: &str) -> String {
        code.to_string()
    }
    pub fn highlight_code_to_anstyle_line_segments(
        code: &str,
        _language: Option<&str>,
        _theme: &str,
    ) -> Vec<Vec<(Style, String)>> {
        code.lines()
            .map(|line| vec![(Style::default(), line.to_string())])
            .collect()
    }
    pub fn highlight_code_to_line_segments(
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Vec<Vec<(Style, String)>> {
        highlight_code_to_anstyle_line_segments(code, language, theme)
    }
    pub fn highlight_code_to_segments(
        code: &str,
        _language: Option<&str>,
        _theme: &str,
    ) -> Vec<(Style, String)> {
        vec![(Style::default(), code.to_string())]
    }
    pub fn highlight_line_for_diff(
        line: &str,
        _language: Option<&str>,
    ) -> Option<Vec<(Style, String)>> {
        Some(vec![(Style::default(), line.to_string())])
    }
    pub fn highlight_line_to_anstyle_segments(
        line: &str,
        _language: Option<&str>,
        _theme: &str,
    ) -> Vec<(Style, String)> {
        vec![(Style::default(), line.to_string())]
    }
    pub fn load_theme(_theme: &str) -> Option<&'static str> {
        None
    }
    pub fn should_highlight(_language: Option<&str>, _theme: &str) -> bool {
        false
    }
    pub fn syntax_set() -> Option<()> {
        None
    }
}

#[cfg(not(feature = "tui"))]
pub use headless_highlight::*;

/// Get the recommended syntax theme for the current core UI theme.
#[inline]
pub fn get_active_syntax_theme() -> &'static str {
    get_syntax_theme_for_ui_theme(&crate::ui::theme::active_theme_id())
}

/// Get the recommended syntax theme for a specific UI theme.
#[inline]
pub fn get_syntax_theme(theme: &str) -> &'static str {
    get_syntax_theme_for_ui_theme(theme)
}
