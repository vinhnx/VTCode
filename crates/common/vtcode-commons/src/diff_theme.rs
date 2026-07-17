//! Diff theme configuration and color palettes
//!
//! Uses subtle red/green tints for diff line backgrounds.

use anstyle::{AnsiColor, Color};

use crate::ansi_capabilities::{ColorScheme, detect_color_scheme};

/// Terminal background theme for diff rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffTheme {
    Dark,
    Light,
}

impl DiffTheme {
    /// Detect theme from the terminal environment.
    pub fn detect() -> Self {
        match detect_color_scheme() {
            ColorScheme::Light => Self::Light,
            ColorScheme::Dark | ColorScheme::Unknown => Self::Dark,
        }
    }

    pub fn is_light(self) -> bool {
        self == Self::Light
    }
}

/// Terminal color capability level for palette selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffColorLevel {
    TrueColor,
    Ansi256,
    Ansi16,
}

impl DiffColorLevel {
    /// Detect color level from terminal capabilities.
    pub fn detect() -> Self {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let term = std::env::var("TERM").unwrap_or_default();
        let term_program = std::env::var("TERM_PROGRAM").ok();
        let has_wt_session = std::env::var_os("WT_SESSION").is_some();
        let has_force_color_override = std::env::var_os("FORCE_COLOR").is_some();

        diff_color_level_for_terminal(
            base_diff_color_level(&colorterm, &term),
            term_program.as_deref(),
            has_wt_session,
            has_force_color_override,
        )
    }
}

fn base_diff_color_level(colorterm: &str, term: &str) -> DiffColorLevel {
    let colorterm = colorterm.to_ascii_lowercase();
    let term = term.to_ascii_lowercase();

    if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        DiffColorLevel::TrueColor
    } else if term.contains("256") {
        DiffColorLevel::Ansi256
    } else {
        DiffColorLevel::Ansi16
    }
}

fn diff_color_level_for_terminal(
    base_level: DiffColorLevel,
    term_program: Option<&str>,
    has_wt_session: bool,
    has_force_color_override: bool,
) -> DiffColorLevel {
    if has_force_color_override {
        return base_level;
    }

    if has_wt_session || (base_level == DiffColorLevel::Ansi16 && is_windows_terminal(term_program))
    {
        return DiffColorLevel::TrueColor;
    }

    base_level
}

fn is_windows_terminal(term_program: Option<&str>) -> bool {
    let Some(program) = term_program else {
        return false;
    };

    let normalized = program.trim().to_ascii_lowercase();
    normalized.contains("windows_terminal") || normalized.contains("windows terminal")
}

// ── Standard ANSI red/green backgrounds ────────────────────────────────────

/// Get background color for addition lines based on theme and color level.
pub fn diff_add_bg(theme: DiffTheme, _level: DiffColorLevel) -> Color {
    match theme {
        DiffTheme::Dark => Color::Rgb(anstyle::RgbColor(20, 58, 45)),
        DiffTheme::Light => Color::Rgb(anstyle::RgbColor(218, 246, 225)),
    }
}

/// Get background color for deletion lines based on theme and color level.
pub fn diff_del_bg(theme: DiffTheme, _level: DiffColorLevel) -> Color {
    match theme {
        DiffTheme::Dark => Color::Rgb(anstyle::RgbColor(70, 38, 42)),
        DiffTheme::Light => Color::Rgb(anstyle::RgbColor(255, 224, 224)),
    }
}

/// Get gutter foreground color for light theme (dark theme uses dimmed default).
pub fn diff_gutter_fg_light(_level: DiffColorLevel) -> Color {
    Color::Ansi(AnsiColor::Black)
}

/// Get gutter background color for addition lines in light theme.
pub fn diff_gutter_bg_add_light(_level: DiffColorLevel) -> Color {
    Color::Ansi(AnsiColor::BrightGreen)
}

/// Get gutter background color for deletion lines in light theme.
pub fn diff_gutter_bg_del_light(_level: DiffColorLevel) -> Color {
    Color::Ansi(AnsiColor::BrightRed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_add_bg_is_subtle_green_tint() {
        let bg = diff_add_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert_eq!(bg, Color::Rgb(anstyle::RgbColor(20, 58, 45)));
    }

    #[test]
    fn dark_del_bg_is_subtle_red_tint() {
        let bg = diff_del_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert_eq!(bg, Color::Rgb(anstyle::RgbColor(70, 38, 42)));
    }

    #[test]
    fn light_add_bg_is_subtle_green_tint() {
        let bg = diff_add_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert_eq!(bg, Color::Rgb(anstyle::RgbColor(218, 246, 225)));
    }

    #[test]
    fn light_del_bg_is_subtle_red_tint() {
        let bg = diff_del_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert_eq!(bg, Color::Rgb(anstyle::RgbColor(255, 224, 224)));
    }

    #[test]
    fn all_levels_use_same_theme_tints() {
        for level in [
            DiffColorLevel::TrueColor,
            DiffColorLevel::Ansi256,
            DiffColorLevel::Ansi16,
        ] {
            assert_eq!(
                diff_add_bg(DiffTheme::Dark, level),
                Color::Rgb(anstyle::RgbColor(20, 58, 45))
            );
            assert_eq!(
                diff_del_bg(DiffTheme::Dark, level),
                Color::Rgb(anstyle::RgbColor(70, 38, 42))
            );
            assert_eq!(
                diff_add_bg(DiffTheme::Light, level),
                Color::Rgb(anstyle::RgbColor(218, 246, 225))
            );
            assert_eq!(
                diff_del_bg(DiffTheme::Light, level),
                Color::Rgb(anstyle::RgbColor(255, 224, 224))
            );
        }
    }

    #[test]
    fn wt_session_promotes_ansi16_to_truecolor() {
        assert_eq!(
            diff_color_level_for_terminal(DiffColorLevel::Ansi16, None, true, false),
            DiffColorLevel::TrueColor
        );
    }

    #[test]
    fn windows_terminal_term_program_promotes_ansi16_to_truecolor() {
        assert_eq!(
            diff_color_level_for_terminal(
                DiffColorLevel::Ansi16,
                Some("Windows_Terminal"),
                false,
                false
            ),
            DiffColorLevel::TrueColor
        );
    }

    #[test]
    fn non_windows_terminal_keeps_ansi16() {
        assert_eq!(
            diff_color_level_for_terminal(DiffColorLevel::Ansi16, Some("WezTerm"), false, false),
            DiffColorLevel::Ansi16
        );
    }

    #[test]
    fn force_color_keeps_ansi16_when_wt_session_exists() {
        assert_eq!(
            diff_color_level_for_terminal(DiffColorLevel::Ansi16, None, true, true),
            DiffColorLevel::Ansi16
        );
    }

    #[test]
    fn force_color_keeps_ansi256_when_wt_session_exists() {
        assert_eq!(
            diff_color_level_for_terminal(DiffColorLevel::Ansi256, None, true, true),
            DiffColorLevel::Ansi256
        );
    }

    #[test]
    fn base_level_detects_truecolor_from_colorterm() {
        assert_eq!(base_diff_color_level("truecolor", "xterm-256color"), DiffColorLevel::TrueColor);
    }

    #[test]
    fn base_level_detects_ansi256_from_term() {
        assert_eq!(base_diff_color_level("", "xterm-256color"), DiffColorLevel::Ansi256);
    }

    #[test]
    fn base_level_falls_back_to_ansi16() {
        assert_eq!(base_diff_color_level("", "xterm"), DiffColorLevel::Ansi16);
    }
}
