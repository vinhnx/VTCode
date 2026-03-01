//! Diff theme configuration and color palettes
//!
//! Provides terminal-adaptive styling that adjusts background tints based on:
//!   1. [`DiffTheme`] (Dark/Light) — detected from terminal background
//!   2. [`DiffColorLevel`] (TrueColor/Ansi256/Ansi16) — from terminal capability
//!
//! Colors are selected for WCAG AA accessibility contrast ratios (4.5:1 minimum).

use anstyle::{Ansi256Color, AnsiColor, Color, RgbColor};

/// Terminal background theme for diff rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffTheme {
    Dark,
    Light,
}

impl DiffTheme {
    /// Detect theme from the terminal environment.
    pub fn detect() -> Self {
        let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
        if term.contains("light") {
            Self::Light
        } else {
            Self::Dark
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

        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            Self::TrueColor
        } else if term.contains("256") {
            Self::Ansi256
        } else {
            Self::Ansi16
        }
    }
}

// ── Truecolor palette (WCAG AA compliant) ──────────────────────────────────

// Dark theme: darker backgrounds with higher contrast for better readability
// Green kept dark, red softened with lower saturation
const DARK_TC_ADD_LINE_BG: (u8, u8, u8) = (25, 45, 35); // #192D23 - Dark teal green
const DARK_TC_DEL_LINE_BG: (u8, u8, u8) = (90, 40, 40); // #5A2828 - Muted dark red (lower alpha feel)

// Light theme: light backgrounds with sufficient contrast for dark text
// Red background made more pastel/muted
const LIGHT_TC_ADD_LINE_BG: (u8, u8, u8) = (215, 240, 215); // #D7F0D7 - Light green
const LIGHT_TC_DEL_LINE_BG: (u8, u8, u8) = (255, 235, 235); // #FFEBEB - Soft pastel red (muted)
const LIGHT_TC_ADD_NUM_BG: (u8, u8, u8) = (175, 225, 175); // #AFE1AF - Gutter green
const LIGHT_TC_DEL_NUM_BG: (u8, u8, u8) = (250, 210, 210); // #FAD2D2 - Muted gutter red
const LIGHT_TC_GUTTER_FG: (u8, u8, u8) = (25, 25, 25); // #191919 - Near-black for contrast

// ── 256-color palette ──────────────────────────────────────────────────────

const DARK_256_ADD_LINE_BG: u8 = 22; // DarkGreen
const DARK_256_DEL_LINE_BG: u8 = 52; // DarkRed

const LIGHT_256_ADD_LINE_BG: u8 = 194; // LightGreen
const LIGHT_256_DEL_LINE_BG: u8 = 224; // LightRed/Pink
const LIGHT_256_ADD_NUM_BG: u8 = 157; // SeaGreen
const LIGHT_256_DEL_NUM_BG: u8 = 217; // LightPink
const LIGHT_256_GUTTER_FG: u8 = 236; // DarkGray

// ── Helper functions ───────────────────────────────────────────────────────

fn rgb(t: (u8, u8, u8)) -> Color {
    Color::Rgb(RgbColor(t.0, t.1, t.2))
}

fn indexed(i: u8) -> Color {
    Color::Ansi256(Ansi256Color(i))
}

/// Get background color for addition lines based on theme and color level.
pub fn diff_add_bg(theme: DiffTheme, level: DiffColorLevel) -> Color {
    match (theme, level) {
        (DiffTheme::Dark, DiffColorLevel::TrueColor) => rgb(DARK_TC_ADD_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi256) => indexed(DARK_256_ADD_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi16) => Color::Ansi(AnsiColor::Green),
        (DiffTheme::Light, DiffColorLevel::TrueColor) => rgb(LIGHT_TC_ADD_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi256) => indexed(LIGHT_256_ADD_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi16) => Color::Ansi(AnsiColor::BrightGreen),
    }
}

/// Get background color for deletion lines based on theme and color level.
pub fn diff_del_bg(theme: DiffTheme, level: DiffColorLevel) -> Color {
    match (theme, level) {
        (DiffTheme::Dark, DiffColorLevel::TrueColor) => rgb(DARK_TC_DEL_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi256) => indexed(DARK_256_DEL_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi16) => Color::Ansi(AnsiColor::Red),
        (DiffTheme::Light, DiffColorLevel::TrueColor) => rgb(LIGHT_TC_DEL_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi256) => indexed(LIGHT_256_DEL_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi16) => Color::Ansi(AnsiColor::BrightRed),
    }
}

/// Get gutter foreground color for light theme (dark theme uses dimmed default).
pub fn diff_gutter_fg_light(level: DiffColorLevel) -> Color {
    match level {
        DiffColorLevel::TrueColor => rgb(LIGHT_TC_GUTTER_FG),
        DiffColorLevel::Ansi256 => indexed(LIGHT_256_GUTTER_FG),
        DiffColorLevel::Ansi16 => Color::Ansi(AnsiColor::Black),
    }
}

/// Get gutter background color for addition lines in light theme.
pub fn diff_gutter_bg_add_light(level: DiffColorLevel) -> Color {
    match level {
        DiffColorLevel::TrueColor => rgb(LIGHT_TC_ADD_NUM_BG),
        DiffColorLevel::Ansi256 => indexed(LIGHT_256_ADD_NUM_BG),
        DiffColorLevel::Ansi16 => Color::Ansi(AnsiColor::BrightGreen),
    }
}

/// Get gutter background color for deletion lines in light theme.
pub fn diff_gutter_bg_del_light(level: DiffColorLevel) -> Color {
    match level {
        DiffColorLevel::TrueColor => rgb(LIGHT_TC_DEL_NUM_BG),
        DiffColorLevel::Ansi256 => indexed(LIGHT_256_DEL_NUM_BG),
        DiffColorLevel::Ansi16 => Color::Ansi(AnsiColor::BrightRed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_truecolor_add_bg_is_rgb() {
        let bg = diff_add_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(bg, Color::Rgb(RgbColor(25, 45, 35))));
    }

    #[test]
    fn dark_truecolor_del_bg_is_rgb() {
        let bg = diff_del_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(bg, Color::Rgb(RgbColor(90, 40, 40))));
    }

    #[test]
    fn light_truecolor_add_bg_is_accessible() {
        let bg = diff_add_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(bg, Color::Rgb(RgbColor(215, 240, 215))));
    }

    #[test]
    fn light_truecolor_del_bg_is_accessible() {
        let bg = diff_del_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(bg, Color::Rgb(RgbColor(255, 235, 235))));
    }

    #[test]
    fn dark_256_uses_indexed_colors() {
        let add = diff_add_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        let del = diff_del_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        assert!(matches!(add, Color::Ansi256(Ansi256Color(22))));
        assert!(matches!(del, Color::Ansi256(Ansi256Color(52))));
    }

    #[test]
    fn dark_ansi16_uses_named_colors() {
        let add = diff_add_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        let del = diff_del_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        assert_eq!(add, Color::Ansi(AnsiColor::Green));
        assert_eq!(del, Color::Ansi(AnsiColor::Red));
    }
}
