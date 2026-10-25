use vtcode_commons::ansi_capabilities::{ColorScheme, detect_color_scheme};

use crate::theme::color_math::{contrast_ratio, relative_luminance};
use crate::theme::registry::{available_theme_suites, theme_definition, theme_suite_id};
use crate::theme::types::{DEFAULT_THEME_ID, ThemeSuite};

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

/// Pick the theme to apply after a terminal light/dark scheme change.
///
/// Prefers a light/dark twin from the same suite as `current_theme_id` so
/// suite-aware themes stay within their family; falls back to
/// [`suggest_theme_for_terminal`] for single-theme or custom selections.
pub fn theme_for_terminal_scheme_change(current_theme_id: &str, dark: bool) -> &'static str {
    if let Some(suite_id) = theme_suite_id(current_theme_id)
        && let Some(suite) = available_theme_suites().into_iter().find(|suite| suite.id == suite_id)
    {
        if let Some(theme_id) = suite_twin(&suite, current_theme_id, dark) {
            return theme_id;
        }
    }
    suggest_theme_for_terminal()
}

/// The suite variant for the requested scheme.
///
/// Prefers the twin that shares the current id's qualifier (`-hard`,
/// `-material`, ...), so `gruvbox-light` ⇄ `gruvbox-dark` and
/// `gruvbox-material` ⇄ `gruvbox-material-light`. When the suite carries that
/// qualifier on one side only, or the plain ids exist, the plain twin wins
/// before readability ranking decides.
fn suite_twin(suite: &ThemeSuite, current: &str, dark: bool) -> Option<&'static str> {
    let qualifier = suite_qualifier(suite.id, current);
    let candidates: Vec<&'static str> = suite
        .theme_ids
        .iter()
        .copied()
        .filter(|theme_id| is_light_theme(theme_id) != dark)
        .collect();
    if let Some(&exact) = candidates
        .iter()
        .find(|theme_id| suite_qualifier(suite.id, theme_id) == qualifier)
    {
        return Some(exact);
    }
    if let Some(&plain) = candidates
        .iter()
        .find(|theme_id| suite_qualifier(suite.id, theme_id).is_empty())
    {
        return Some(plain);
    }
    candidates
        .into_iter()
        .max_by(|left, right| variant_readability(left).total_cmp(&variant_readability(right)))
}

/// The qualifier a theme id adds after its suite id and scheme word, if any:
/// `gruvbox-dark-hard` → `hard`, `gruvbox-material-dark` → `material`,
/// `gruvbox` → empty.
fn suite_qualifier<'a>(suite_id: &str, theme_id: &'a str) -> &'a str {
    let qualifier = theme_id.strip_prefix(suite_id).unwrap_or_default().trim_start_matches('-');
    if qualifier == "light" || qualifier == "dark" {
        return "";
    }
    qualifier
        .strip_prefix("light-")
        .or_else(|| qualifier.strip_prefix("dark-"))
        .unwrap_or(qualifier)
}

/// Contrast of a theme's text against its own background; used only to rank
/// variants inside one suite.
fn variant_readability(theme_id: &str) -> f32 {
    theme_definition(theme_id)
        .map(|theme| contrast_ratio(theme.palette.foreground, theme.palette.background))
        .unwrap_or(0.0)
}
