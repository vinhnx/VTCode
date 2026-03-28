//! Shared theme registry and runtime state for VT Code UI crates.

mod color_math;
mod registry;
mod runtime;
mod scheme;
mod syntax;
#[cfg(test)]
mod tests;
mod types;

pub use registry::{
    available_theme_suites, available_themes, theme_label, theme_suite_id, theme_suite_label,
};
pub use runtime::{
    active_styles, active_theme_id, active_theme_label, banner_color, banner_style, ensure_theme,
    get_minimum_contrast, is_bold_bright_mode, is_safe_colors_only, logo_accent_color,
    rebuild_active_styles, resolve_theme, set_active_theme, set_color_accessibility_config,
    validate_theme_contrast,
};
pub use scheme::{is_light_theme, suggest_theme_for_terminal, theme_matches_terminal_scheme};
pub use syntax::{get_active_syntax_theme, get_syntax_theme_for_ui_theme};
pub use types::{
    ColorAccessibilityConfig, DEFAULT_THEME_ID, ThemeDefinition, ThemePalette, ThemeStyles,
    ThemeSuite, ThemeValidationResult,
};
