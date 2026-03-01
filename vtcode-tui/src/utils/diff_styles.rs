//! Unified diff styles for TUI rendering
//!
//! Re-exports diff theme from vtcode-commons and provides
//! ratatui-specific style helpers for diff rendering.

// Re-export diff theme from vtcode-commons
pub use vtcode_commons::diff_theme::{
    DiffColorLevel, DiffTheme, diff_add_bg, diff_del_bg, diff_gutter_bg_add_light,
    diff_gutter_bg_del_light, diff_gutter_fg_light,
};
pub use vtcode_commons::styling::DiffColorPalette;

use ratatui::style::{Color as RatatuiColor, Modifier, Style as RatatuiStyle};

// ── Conversion helpers ─────────────────────────────────────────────────────

/// Convert anstyle Color to ratatui Color
fn ratatui_color_from_anstyle(color: anstyle::Color) -> RatatuiColor {
    match color {
        anstyle::Color::Ansi(c) => match c {
            anstyle::AnsiColor::Black => RatatuiColor::Black,
            anstyle::AnsiColor::Red => RatatuiColor::Red,
            anstyle::AnsiColor::Green => RatatuiColor::Green,
            anstyle::AnsiColor::Yellow => RatatuiColor::Yellow,
            anstyle::AnsiColor::Blue => RatatuiColor::Blue,
            anstyle::AnsiColor::Magenta => RatatuiColor::Magenta,
            anstyle::AnsiColor::Cyan => RatatuiColor::Cyan,
            anstyle::AnsiColor::White => RatatuiColor::White,
            anstyle::AnsiColor::BrightBlack => RatatuiColor::DarkGray,
            anstyle::AnsiColor::BrightRed => RatatuiColor::LightRed,
            anstyle::AnsiColor::BrightGreen => RatatuiColor::LightGreen,
            anstyle::AnsiColor::BrightYellow => RatatuiColor::LightYellow,
            anstyle::AnsiColor::BrightBlue => RatatuiColor::LightBlue,
            anstyle::AnsiColor::BrightMagenta => RatatuiColor::LightMagenta,
            anstyle::AnsiColor::BrightCyan => RatatuiColor::LightCyan,
            anstyle::AnsiColor::BrightWhite => RatatuiColor::White,
        },
        anstyle::Color::Ansi256(c) => RatatuiColor::Indexed(c.0),
        anstyle::Color::Rgb(c) => RatatuiColor::Rgb(c.0, c.1, c.2),
    }
}

// ── TUI-specific diff line styling ─────────────────────────────────────────

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
        DiffLineType::Insert => {
            RatatuiStyle::default().bg(ratatui_color_from_anstyle(diff_add_bg(theme, level)))
        }
        DiffLineType::Delete => {
            RatatuiStyle::default().bg(ratatui_color_from_anstyle(diff_del_bg(theme, level)))
        }
        DiffLineType::Context => RatatuiStyle::default(),
    }
}

/// Gutter (line number) style.
///
/// Keep gutter signs/numbers dim and on standard ANSI red/green without bold.
pub fn style_gutter(kind: DiffLineType, theme: DiffTheme, level: DiffColorLevel) -> RatatuiStyle {
    let _ = (theme, level);
    match kind {
        DiffLineType::Insert => RatatuiStyle::default()
            .fg(RatatuiColor::Green)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Delete => RatatuiStyle::default()
            .fg(RatatuiColor::Red)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Context => RatatuiStyle::default().add_modifier(Modifier::DIM),
    }
}

/// Sign character (`+`/`-`) style.
/// Uses standard ANSI red/green without bold for consistency.
pub fn style_sign(kind: DiffLineType, _theme: DiffTheme, _level: DiffColorLevel) -> RatatuiStyle {
    match kind {
        DiffLineType::Insert => RatatuiStyle::default()
            .fg(RatatuiColor::Green)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Delete => RatatuiStyle::default()
            .fg(RatatuiColor::Red)
            .add_modifier(Modifier::DIM)
            .remove_modifier(Modifier::BOLD),
        DiffLineType::Context => RatatuiStyle::default(),
    }
}

/// Content style for plain (non-syntax-highlighted) diff lines.
///
/// Dark + ANSI16: black fg on colored bg for contrast.
/// Light: bg only, no fg override.
/// Dark + TrueColor/256: colored fg + tinted bg.
pub fn style_content(kind: DiffLineType, theme: DiffTheme, level: DiffColorLevel) -> RatatuiStyle {
    match (kind, theme, level) {
        // Dark + ANSI16: force Black fg on colored bg for contrast
        (DiffLineType::Insert, DiffTheme::Dark, DiffColorLevel::Ansi16) => RatatuiStyle::default()
            .fg(RatatuiColor::Black)
            .bg(ratatui_color_from_anstyle(diff_add_bg(theme, level))),
        (DiffLineType::Delete, DiffTheme::Dark, DiffColorLevel::Ansi16) => RatatuiStyle::default()
            .fg(RatatuiColor::Black)
            .bg(ratatui_color_from_anstyle(diff_del_bg(theme, level))),
        // Light: bg only, no fg override
        (DiffLineType::Insert, DiffTheme::Light, _) => {
            RatatuiStyle::default().bg(ratatui_color_from_anstyle(diff_add_bg(theme, level)))
        }
        (DiffLineType::Delete, DiffTheme::Light, _) => {
            RatatuiStyle::default().bg(ratatui_color_from_anstyle(diff_del_bg(theme, level)))
        }
        // Dark + TrueColor/256: colored fg + tinted bg
        (DiffLineType::Insert, DiffTheme::Dark, _) => RatatuiStyle::default()
            .fg(RatatuiColor::Green)
            .bg(ratatui_color_from_anstyle(diff_add_bg(theme, level))),
        (DiffLineType::Delete, DiffTheme::Dark, _) => RatatuiStyle::default()
            .fg(RatatuiColor::Red)
            .bg(ratatui_color_from_anstyle(diff_del_bg(theme, level))),
        // Context: terminal default
        (DiffLineType::Context, _, _) => RatatuiStyle::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_truecolor_add_bg_is_rgb() {
        let bg = diff_add_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(25, 45, 35))
        ));
    }

    #[test]
    fn dark_truecolor_del_bg_is_rgb() {
        let bg = diff_del_bg(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(90, 40, 40))
        ));
    }

    #[test]
    fn light_truecolor_add_bg_is_accessible() {
        let bg = diff_add_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(215, 240, 215))
        ));
    }

    #[test]
    fn light_truecolor_del_bg_is_accessible() {
        let bg = diff_del_bg(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert!(matches!(
            bg,
            anstyle::Color::Rgb(anstyle::RgbColor(255, 235, 235))
        ));
    }

    #[test]
    fn dark_256_uses_indexed_colors() {
        let add = diff_add_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        let del = diff_del_bg(DiffTheme::Dark, DiffColorLevel::Ansi256);
        assert!(matches!(
            add,
            anstyle::Color::Ansi256(anstyle::Ansi256Color(22))
        ));
        assert!(matches!(
            del,
            anstyle::Color::Ansi256(anstyle::Ansi256Color(52))
        ));
    }

    #[test]
    fn dark_ansi16_uses_named_colors() {
        let add = diff_add_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        let del = diff_del_bg(DiffTheme::Dark, DiffColorLevel::Ansi16);
        assert_eq!(add, anstyle::Color::Ansi(anstyle::AnsiColor::Green));
        assert_eq!(del, anstyle::Color::Ansi(anstyle::AnsiColor::Red));
    }

    #[test]
    fn context_line_bg_is_default() {
        let style = style_line_bg(
            DiffLineType::Context,
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
        );
        assert_eq!(style, RatatuiStyle::default());
    }

    #[test]
    fn dark_gutter_is_dim() {
        let style = style_gutter(
            DiffLineType::Context,
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
        );
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn insert_gutter_is_dim_standard_green_no_bold() {
        let style = style_gutter(
            DiffLineType::Insert,
            DiffTheme::Light,
            DiffColorLevel::TrueColor,
        );
        assert_eq!(style.fg, Some(RatatuiColor::Green));
        assert!(style.add_modifier.contains(Modifier::DIM));
        assert!(style.sub_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn dark_ansi16_content_forces_black_fg() {
        let style = style_content(
            DiffLineType::Insert,
            DiffTheme::Dark,
            DiffColorLevel::Ansi16,
        );
        assert_eq!(style.fg, Some(RatatuiColor::Black));
    }

    #[test]
    fn sign_style_always_uses_standard_colors() {
        let add_sign = style_sign(
            DiffLineType::Insert,
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
        );
        let del_sign = style_sign(
            DiffLineType::Delete,
            DiffTheme::Light,
            DiffColorLevel::Ansi16,
        );
        assert_eq!(add_sign.fg, Some(RatatuiColor::Green));
        assert_eq!(del_sign.fg, Some(RatatuiColor::Red));
        assert!(add_sign.add_modifier.contains(Modifier::DIM));
        assert!(del_sign.add_modifier.contains(Modifier::DIM));
        assert!(add_sign.sub_modifier.contains(Modifier::BOLD));
        assert!(del_sign.sub_modifier.contains(Modifier::BOLD));
    }
}
