//! Unified message styles and their logical mappings

use anstyle::{Ansi256Color, AnsiColor, Color, Effects, RgbColor, Style};

/// Standard color palette with semantic names
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub success: Color, // Green
    pub error: Color,   // Red
    pub warning: Color, // Red
    pub info: Color,    // Cyan
    pub accent: Color,  // Magenta
    pub primary: Color, // Cyan
    pub muted: Color,   // Gray/Dim
}

impl Default for ColorPalette {
    fn default() -> Self {
        Self {
            success: Color::Ansi(AnsiColor::Green),
            error: Color::Ansi(AnsiColor::Red),
            warning: Color::Ansi(AnsiColor::Red),
            info: Color::Ansi(AnsiColor::Cyan),
            accent: Color::Ansi(AnsiColor::Magenta),
            primary: Color::Ansi(AnsiColor::Cyan),
            muted: Color::Ansi(AnsiColor::BrightBlack),
        }
    }
}

/// Render text with a single color and optional effects
pub fn render_styled(text: &str, color: Color, effects: Option<String>) -> String {
    let mut style = Style::new().fg_color(Some(color));

    if let Some(effects_str) = effects {
        let mut ansi_effects = Effects::new();

        for effect in effects_str.split(',') {
            let effect = effect.trim().to_lowercase();
            match effect.as_str() {
                "bold" => ansi_effects |= Effects::BOLD,
                "dim" | "dimmed" => ansi_effects |= Effects::DIMMED,
                "italic" => ansi_effects |= Effects::ITALIC,
                "underline" => ansi_effects |= Effects::UNDERLINE,
                "blink" => ansi_effects |= Effects::BLINK,
                "invert" | "reversed" => ansi_effects |= Effects::INVERT,
                "hidden" => ansi_effects |= Effects::HIDDEN,
                "strikethrough" => ansi_effects |= Effects::STRIKETHROUGH,
                _ => {}
            }
        }

        style = style.effects(ansi_effects);
    }

    // Use static reset code
    format!("{}{}{}", style, text, "\x1b[0m")
}

/// Build style from CSS/terminal color name
pub fn style_from_color_name(name: &str) -> Style {
    let (color_name, dimmed) = if let Some(idx) = name.find(':') {
        let (color, modifier) = name.split_at(idx);
        (color, modifier.strip_prefix(':').unwrap_or(""))
    } else {
        (name, "")
    };

    let color = match color_name.to_lowercase().as_str() {
        "red" => Color::Ansi(AnsiColor::Red),
        "green" => Color::Ansi(AnsiColor::Green),
        "blue" => Color::Ansi(AnsiColor::Blue),
        "yellow" => Color::Ansi(AnsiColor::Yellow),
        "cyan" => Color::Ansi(AnsiColor::Cyan),
        "magenta" | "purple" => Color::Ansi(AnsiColor::Magenta),
        "white" => Color::Ansi(AnsiColor::White),
        "black" => Color::Ansi(AnsiColor::Black),
        _ => return Style::new(),
    };

    let mut style = Style::new().fg_color(Some(color));
    if dimmed.eq_ignore_ascii_case("dimmed") {
        style = style.dimmed();
    }
    style
}

/// Create a bold colored style from AnsiColor
pub fn bold_color(color: AnsiColor) -> Style {
    Style::new().bold().fg_color(Some(Color::Ansi(color)))
}

/// Create a dimmed colored style from AnsiColor
pub fn dimmed_color(color: AnsiColor) -> Style {
    Style::new().dimmed().fg_color(Some(Color::Ansi(color)))
}

/// Diff color palette for consistent git diff styling
/// Uses standard ANSI colors without bold for accessibility and consistency.
#[derive(Debug, Clone, Copy)]
pub struct DiffColorPalette {
    pub added_fg: Color,
    pub added_bg: Color,
    pub removed_fg: Color,
    pub removed_bg: Color,
    pub header_fg: Color,
    pub header_bg: Color,
}

impl Default for DiffColorPalette {
    fn default() -> Self {
        Self {
            added_fg: Color::Ansi(AnsiColor::Green),
            added_bg: Color::Rgb(RgbColor(10, 24, 10)),
            removed_fg: Color::Ansi(AnsiColor::Red),
            removed_bg: Color::Rgb(RgbColor(24, 10, 10)),
            header_fg: Color::Ansi(AnsiColor::Cyan),
            header_bg: Color::Rgb(RgbColor(10, 16, 20)),
        }
    }
}

impl DiffColorPalette {
    pub fn added_style(&self) -> Style {
        Style::new().fg_color(Some(self.added_fg))
    }

    pub fn removed_style(&self) -> Style {
        Style::new().fg_color(Some(self.removed_fg))
    }

    pub fn header_style(&self) -> Style {
        Style::new().fg_color(Some(self.header_fg))
    }
}

// ── Theme-aware diff rendering ─────────────────────────────────────────────
//
// Provides terminal-adaptive styling that adjusts background tints based on:
//   1. DiffTheme (Dark/Light) — detected from terminal background
//   2. DiffColorLevel (TrueColor/Ansi256/Ansi16) — from terminal capability
//
// Colors are selected for WCAG AA accessibility contrast ratios (4.5:1 minimum).

/// Terminal background theme for diff rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffTheme {
    Dark,
    Light,
}

impl DiffTheme {
    /// Detect theme from the terminal environment.
    pub fn detect() -> Self {
        // Check COLORTERM and TERM environment variables for light theme indicators
        // Default to dark theme for unknown cases (most common)
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

// Dark theme: darker backgrounds with sufficient contrast for white/light text
const DARK_TC_ADD_LINE_BG: (u8, u8, u8) = (20, 40, 20); // #142814 - Dark green
const DARK_TC_DEL_LINE_BG: (u8, u8, u8) = (48, 16, 16); // #301010 - Dark red

// Light theme: light backgrounds with sufficient contrast for dark text
// GitHub-inspired but adjusted for better contrast
const LIGHT_TC_ADD_LINE_BG: (u8, u8, u8) = (220, 245, 220); // #DCF5DC - Light green
const LIGHT_TC_DEL_LINE_BG: (u8, u8, u8) = (250, 220, 220); // #FADCD4 - Light red
const LIGHT_TC_ADD_NUM_BG: (u8, u8, u8) = (180, 230, 180); // #B4E6B4 - Gutter green
const LIGHT_TC_DEL_NUM_BG: (u8, u8, u8) = (245, 190, 190); // #F5BEBE - Gutter red
const LIGHT_TC_GUTTER_FG: (u8, u8, u8) = (30, 30, 30); // #1E1E1E - Near-black for contrast

// ── 256-color palette ──────────────────────────────────────────────────────

const DARK_256_ADD_LINE_BG: u8 = 22; // DarkGreen
const DARK_256_DEL_LINE_BG: u8 = 52; // DarkRed

const LIGHT_256_ADD_LINE_BG: u8 = 194; // LightGreen
const LIGHT_256_DEL_LINE_BG: u8 = 224; // LightRed/Pink
const LIGHT_256_ADD_NUM_BG: u8 = 157; // SeaGreen
const LIGHT_256_DEL_NUM_BG: u8 = 217; // LightPink
const LIGHT_256_GUTTER_FG: u8 = 236; // DarkGray

// ── Background color helpers ───────────────────────────────────────────────

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
