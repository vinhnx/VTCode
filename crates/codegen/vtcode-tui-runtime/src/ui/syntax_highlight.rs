pub use vtcode_ui::tui::ui::syntax_highlight::{
    available_themes, default_theme_name, find_syntax_by_extension, find_syntax_by_name, find_syntax_by_token,
    find_syntax_plain_text, highlight_code_to_ansi, highlight_code_to_anstyle_line_segments,
    highlight_code_to_line_segments, highlight_code_to_segments, highlight_line_for_diff,
    highlight_line_to_anstyle_segments, load_theme, should_highlight, syntax_set,
};

use crate::ui::theme::get_syntax_theme_for_ui_theme;

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
