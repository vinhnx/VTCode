use vtcode_commons::ansi_capabilities::{ColorScheme, detect_color_scheme};

use crate::color_math::relative_luminance;
use crate::registry::theme_definition;
use crate::types::DEFAULT_THEME_ID;

/// Report whether a theme matches the detected terminal light/dark scheme.
pub fn theme_matches_terminal_scheme(theme_id: &str) -> bool {
    let scheme = detect_color_scheme();
    let theme_is_light = is_light_theme(theme_id);

    match scheme {
        ColorScheme::Light => theme_is_light,
        ColorScheme::Dark | ColorScheme::Unknown => !theme_is_light,
    }
}

/// Report whether a built-in theme should be treated as a light theme.
pub fn is_light_theme(theme_id: &str) -> bool {
    theme_definition(theme_id)
        .map(|theme| relative_luminance(theme.palette.background) > 0.5)
        .unwrap_or(false)
}

/// Suggest a built-in theme that matches the current terminal scheme.
pub fn suggest_theme_for_terminal() -> &'static str {
    match detect_color_scheme() {
        ColorScheme::Light => "vitesse-light",
        ColorScheme::Dark | ColorScheme::Unknown => DEFAULT_THEME_ID,
    }
}
