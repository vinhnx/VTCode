//! Unified message styles and their logical mappings

use anstyle::{AnsiColor, Color, Style};

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
        let mut ansi_effects = anstyle::Effects::new();

        for effect in effects_str.split(',') {
            let effect = effect.trim().to_lowercase();
            match effect.as_str() {
                "bold" => ansi_effects |= anstyle::Effects::BOLD,
                "dim" | "dimmed" => ansi_effects |= anstyle::Effects::DIMMED,
                "italic" => ansi_effects |= anstyle::Effects::ITALIC,
                "underline" => ansi_effects |= anstyle::Effects::UNDERLINE,
                "blink" => ansi_effects |= anstyle::Effects::BLINK,
                "invert" | "reversed" => ansi_effects |= anstyle::Effects::INVERT,
                "hidden" => ansi_effects |= anstyle::Effects::HIDDEN,
                "strikethrough" => ansi_effects |= anstyle::Effects::STRIKETHROUGH,
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
            added_bg: Color::Rgb(anstyle::RgbColor(10, 24, 10)),
            removed_fg: Color::Ansi(AnsiColor::Red),
            removed_bg: Color::Rgb(anstyle::RgbColor(24, 10, 10)),
            header_fg: Color::Ansi(AnsiColor::Cyan),
            header_bg: Color::Rgb(anstyle::RgbColor(10, 16, 20)),
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
// Extends the base DiffColorPalette with terminal-adaptive styling that
// adjusts background tints and gutter styling based on:
//   1. DiffTheme (Dark/Light) — detected from terminal background
//   2. DiffColorLevel (TrueColor/Ansi256/Ansi16) — from terminal capability
//
// Inspired by github.com/openai/codex PR #12581.

use ratatui::style::{Color as RatatuiColor, Modifier, Style as RatatuiStyle};

use super::ansi_capabilities::{ColorDepth, ColorScheme, detect_color_scheme, CAPABILITIES};

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
        match CAPABILITIES.color_depth {
            ColorDepth::TrueColor => Self::TrueColor,
            ColorDepth::Color256 => Self::Ansi256,
            ColorDepth::Basic16 | ColorDepth::None => Self::Ansi16,
        }
    }
}

// ── Truecolor palette ──────────────────────────────────────────────────────

const DARK_TC_ADD_LINE_BG: (u8, u8, u8) = (33, 58, 43);      // #213A2B
const DARK_TC_DEL_LINE_BG: (u8, u8, u8) = (74, 34, 29);      // #4A221D

const LIGHT_TC_ADD_LINE_BG: (u8, u8, u8) = (218, 251, 225);  // #DAFBE1 (GitHub-style)
const LIGHT_TC_DEL_LINE_BG: (u8, u8, u8) = (255, 235, 233);  // #FFEBE9 (GitHub-style)
const LIGHT_TC_ADD_NUM_BG: (u8, u8, u8) = (172, 238, 187);   // #ACEEBB (gutter, saturated)
const LIGHT_TC_DEL_NUM_BG: (u8, u8, u8) = (255, 206, 203);   // #FFCECB (gutter, saturated)
const LIGHT_TC_GUTTER_FG: (u8, u8, u8) = (31, 35, 40);       // #1F2328 (near-black)

// ── 256-color palette ──────────────────────────────────────────────────────

const DARK_256_ADD_LINE_BG: u8 = 22;   // DarkGreen
const DARK_256_DEL_LINE_BG: u8 = 52;   // DarkRed

const LIGHT_256_ADD_LINE_BG: u8 = 194; // Honeydew2
const LIGHT_256_DEL_LINE_BG: u8 = 224; // MistyRose1
const LIGHT_256_ADD_NUM_BG: u8 = 157;  // DarkSeaGreen2
const LIGHT_256_DEL_NUM_BG: u8 = 217;  // LightPink1
const LIGHT_256_GUTTER_FG: u8 = 236;   // Grey19

// ── Background color selectors ─────────────────────────────────────────────

fn rgb(t: (u8, u8, u8)) -> RatatuiColor {
    RatatuiColor::Rgb(t.0, t.1, t.2)
}

fn indexed(i: u8) -> RatatuiColor {
    RatatuiColor::Indexed(i)
}

/// Background color for addition lines.
pub fn add_line_bg(theme: DiffTheme, level: DiffColorLevel) -> RatatuiColor {
    match (theme, level) {
        (DiffTheme::Dark, DiffColorLevel::TrueColor) => rgb(DARK_TC_ADD_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi256) => indexed(DARK_256_ADD_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi16) => RatatuiColor::Green,
        (DiffTheme::Light, DiffColorLevel::TrueColor) => rgb(LIGHT_TC_ADD_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi256) => indexed(LIGHT_256_ADD_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi16) => RatatuiColor::LightGreen,
    }
}

/// Background color for deletion lines.
pub fn del_line_bg(theme: DiffTheme, level: DiffColorLevel) -> RatatuiColor {
    match (theme, level) {
        (DiffTheme::Dark, DiffColorLevel::TrueColor) => rgb(DARK_TC_DEL_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi256) => indexed(DARK_256_DEL_LINE_BG),
        (DiffTheme::Dark, DiffColorLevel::Ansi16) => RatatuiColor::Red,
        (DiffTheme::Light, DiffColorLevel::TrueColor) => rgb(LIGHT_TC_DEL_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi256) => indexed(LIGHT_256_DEL_LINE_BG),
        (DiffTheme::Light, DiffColorLevel::Ansi16) => RatatuiColor::LightRed,
    }
}

// ── Gutter helpers (light theme) ───────────────────────────────────────────

fn light_gutter_fg(level: DiffColorLevel) -> RatatuiColor {
    match level {
        DiffColorLevel::TrueColor => rgb(LIGHT_TC_GUTTER_FG),
        DiffColorLevel::Ansi256 => indexed(LIGHT_256_GUTTER_FG),
        DiffColorLevel::Ansi16 => RatatuiColor::Black,
    }
}

fn light_add_num_bg(level: DiffColorLevel) -> RatatuiColor {
    match level {
        DiffColorLevel::TrueColor => rgb(LIGHT_TC_ADD_NUM_BG),
        DiffColorLevel::Ansi256 => indexed(LIGHT_256_ADD_NUM_BG),
        DiffColorLevel::Ansi16 => RatatuiColor::Green,
    }
}

fn light_del_num_bg(level: DiffColorLevel) -> RatatuiColor {
    match level {
        DiffColorLevel::TrueColor => rgb(LIGHT_TC_DEL_NUM_BG),
        DiffColorLevel::Ansi256 => indexed(LIGHT_256_DEL_NUM_BG),
        DiffColorLevel::Ansi16 => RatatuiColor::Red,
    }
}

// ── Composed style builders ────────────────────────────────────────────────

/// Diff line type for style selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineType {
    Insert,
    Delete,
    Context,
}

/// Full-width line background style. Context lines use terminal default.
pub fn style_line_bg(kind: DiffLineType, theme: DiffTheme, level: DiffColorLevel) -> RatatuiStyle {
    match kind {
        DiffLineType::Insert => RatatuiStyle::default().bg(add_line_bg(theme, level)),
        DiffLineType::Delete => RatatuiStyle::default().bg(del_line_bg(theme, level)),
        DiffLineType::Context => RatatuiStyle::default(),
    }
}

/// Gutter (line number) style.
///
/// Light: opaque tinted bg + near-black fg for readability.
/// Dark: simple DIM modifier.
pub fn style_gutter(kind: DiffLineType, theme: DiffTheme, level: DiffColorLevel) -> RatatuiStyle {
    match (theme, kind) {
        (DiffTheme::Light, DiffLineType::Insert) => RatatuiStyle::default()
            .fg(light_gutter_fg(level))
            .bg(light_add_num_bg(level)),
        (DiffTheme::Light, DiffLineType::Delete) => RatatuiStyle::default()
            .fg(light_gutter_fg(level))
            .bg(light_del_num_bg(level)),
        _ => RatatuiStyle::default().add_modifier(Modifier::DIM),
    }
}

/// Sign character (`+`/`-`) style.
pub fn style_sign(kind: DiffLineType, theme: DiffTheme, level: DiffColorLevel) -> RatatuiStyle {
    match kind {
        DiffLineType::Insert => match theme {
            DiffTheme::Light => RatatuiStyle::default().fg(RatatuiColor::Green),
            DiffTheme::Dark => style_content(kind, theme, level),
        },
        DiffLineType::Delete => match theme {
            DiffTheme::Light => RatatuiStyle::default().fg(RatatuiColor::Red),
            DiffTheme::Dark => style_content(kind, theme, level),
        },
        DiffLineType::Context => RatatuiStyle::default(),
    }
}

/// Content style for plain (non-syntax-highlighted) diff lines.
pub fn style_content(kind: DiffLineType, theme: DiffTheme, level: DiffColorLevel) -> RatatuiStyle {
    match (kind, theme, level) {
        // Dark + ANSI16: force Black fg on colored bg for contrast
        (DiffLineType::Insert, DiffTheme::Dark, DiffColorLevel::Ansi16) => RatatuiStyle::default()
            .fg(RatatuiColor::Black)
            .bg(add_line_bg(theme, level)),
        (DiffLineType::Delete, DiffTheme::Dark, DiffColorLevel::Ansi16) => RatatuiStyle::default()
            .fg(RatatuiColor::Black)
            .bg(del_line_bg(theme, level)),
        // Light: bg only, no fg override
        (DiffLineType::Insert, DiffTheme::Light, _) => {
            RatatuiStyle::default().bg(add_line_bg(theme, level))
        }
        (DiffLineType::Delete, DiffTheme::Light, _) => {
            RatatuiStyle::default().bg(del_line_bg(theme, level))
        }
        // Dark + TrueColor/256: colored fg + tinted bg
        (DiffLineType::Insert, DiffTheme::Dark, _) => RatatuiStyle::default()
            .fg(RatatuiColor::Green)
            .bg(add_line_bg(theme, level)),
        (DiffLineType::Delete, DiffTheme::Dark, _) => RatatuiStyle::default()
            .fg(RatatuiColor::Red)
            .bg(del_line_bg(theme, level)),
        // Context: terminal default
        (DiffLineType::Context, _, _) => RatatuiStyle::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_truecolor_add_bg_is_rgb() {
        let bg = add_line_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(bg, RatatuiColor::Rgb(33, 58, 43)));
    }

    #[test]
    fn dark_truecolor_del_bg_is_rgb() {
        let bg = del_line_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(bg, RatatuiColor::Rgb(74, 34, 29)));
    }

    #[test]
    fn light_truecolor_add_bg_is_github_style() {
        let bg = add_line_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(bg, RatatuiColor::Rgb(218, 251, 225)));
    }

    #[test]
    fn light_truecolor_del_bg_is_github_style() {
        let bg = del_line_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(bg, RatatuiColor::Rgb(255, 235, 233)));
    }

    #[test]
    fn dark_256_uses_indexed_colors() {
        let add = add_line_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        let del = del_line_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        assert!(matches!(add, RatatuiColor::Indexed(22)));
        assert!(matches!(del, RatatuiColor::Indexed(52)));
    }

    #[test]
    fn dark_ansi16_uses_named_colors() {
        let add = add_line_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        let del = del_line_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        assert_eq!(add, RatatuiColor::Green);
        assert_eq!(del, RatatuiColor::Red);
    }

    #[test]
    fn context_line_bg_is_default() {
        let style = style_line_bg(DiffLineType::Context, DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert_eq!(style, RatatuiStyle::default());
    }

    #[test]
    fn dark_gutter_is_dim() {
        let style = style_gutter(DiffLineType::Context, DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn light_gutter_has_opaque_bg() {
        let style = style_gutter(DiffLineType::Insert, DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(style.bg.is_some());
        assert!(style.fg.is_some());
    }

    #[test]
    fn dark_ansi16_content_forces_black_fg() {
        let style = style_content(DiffLineType::Insert, DiffTheme::Dark, DiffColorLevel::Ansi16);
        assert_eq!(style.fg, Some(RatatuiColor::Black));
    }
}


