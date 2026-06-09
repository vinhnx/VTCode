//! Unified color conversion between `anstyle` and `ratatui`.
//!
//! This module provides the single correct mapping from `anstyle::Color` to
//! `ratatui::style::Color`. Previous implementations in `vtcode-commons` and
//! `vtcode-tui` had bugs mapping `Magenta` and bright variants to incorrect
//! ratatui colors.

use anstyle::{AnsiColor, Color as AnstyleColor, RgbColor};

/// Convert an `anstyle::Color` to a `ratatui::style::Color`.
///
/// This is the canonical, correct mapping. It properly handles:
/// - All 16 standard ANSI colors (including bright variants as `Light*`)
/// - 256-color palette via `Indexed`
/// - True color via `Rgb`
///
/// # Bug fixes over prior implementations
///
/// Prior implementations in `vtcode-commons::anstyle_utils` and
/// `vtcode-tui::core_tui::style` incorrectly mapped:
/// - `Magenta` to `DarkGray` (now correctly `Magenta`)
/// - `BrightMagenta` to `DarkGray` (now correctly `LightMagenta`)
/// - `BrightRed/Green/Yellow/Blue/Cyan` to non-bright variants
/// - `Ansi256` colors to `Reset` instead of `Indexed`
pub fn anstyle_to_ratatui_color(color: AnstyleColor) -> ratatui::style::Color {
    match color {
        AnstyleColor::Ansi(ansi) => ansi_to_ratatui(ansi),
        AnstyleColor::Ansi256(c) => ratatui::style::Color::Indexed(c.0),
        AnstyleColor::Rgb(RgbColor(r, g, b)) => ratatui::style::Color::Rgb(r, g, b),
    }
}

/// Map a standard ANSI color (0-15) to its ratatui equivalent.
fn ansi_to_ratatui(color: AnsiColor) -> ratatui::style::Color {
    match color {
        AnsiColor::Black => ratatui::style::Color::Black,
        AnsiColor::Red => ratatui::style::Color::Red,
        AnsiColor::Green => ratatui::style::Color::Green,
        AnsiColor::Yellow => ratatui::style::Color::Yellow,
        AnsiColor::Blue => ratatui::style::Color::Blue,
        AnsiColor::Magenta => ratatui::style::Color::Magenta,
        AnsiColor::Cyan => ratatui::style::Color::Cyan,
        AnsiColor::White => ratatui::style::Color::White,
        AnsiColor::BrightBlack => ratatui::style::Color::DarkGray,
        AnsiColor::BrightRed => ratatui::style::Color::LightRed,
        AnsiColor::BrightGreen => ratatui::style::Color::LightGreen,
        AnsiColor::BrightYellow => ratatui::style::Color::LightYellow,
        AnsiColor::BrightBlue => ratatui::style::Color::LightBlue,
        AnsiColor::BrightMagenta => ratatui::style::Color::LightMagenta,
        AnsiColor::BrightCyan => ratatui::style::Color::LightCyan,
        AnsiColor::BrightWhite => ratatui::style::Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_ansi_colors() {
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::Black)),
            ratatui::style::Color::Black
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::Red)),
            ratatui::style::Color::Red
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::Green)),
            ratatui::style::Color::Green
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::Yellow)),
            ratatui::style::Color::Yellow
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::Blue)),
            ratatui::style::Color::Blue
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::Cyan)),
            ratatui::style::Color::Cyan
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::White)),
            ratatui::style::Color::White
        );
    }

    #[test]
    fn magenta_maps_to_magenta_not_dark_gray() {
        // Regression test: previous implementations incorrectly mapped
        // Magenta to DarkGray.
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::Magenta)),
            ratatui::style::Color::Magenta
        );
    }

    #[test]
    fn bright_ansi_colors() {
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightBlack)),
            ratatui::style::Color::DarkGray
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightRed)),
            ratatui::style::Color::LightRed
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightGreen)),
            ratatui::style::Color::LightGreen
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightYellow)),
            ratatui::style::Color::LightYellow
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightBlue)),
            ratatui::style::Color::LightBlue
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightMagenta)),
            ratatui::style::Color::LightMagenta
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightCyan)),
            ratatui::style::Color::LightCyan
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightWhite)),
            ratatui::style::Color::White
        );
    }

    #[test]
    fn bright_magenta_maps_to_light_magenta() {
        // Regression test: previous implementations incorrectly mapped
        // BrightMagenta to DarkGray.
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi(AnsiColor::BrightMagenta)),
            ratatui::style::Color::LightMagenta
        );
    }

    #[test]
    fn ansi256_color() {
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi256(anstyle::Ansi256Color(42))),
            ratatui::style::Color::Indexed(42)
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi256(anstyle::Ansi256Color(0))),
            ratatui::style::Color::Indexed(0)
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Ansi256(anstyle::Ansi256Color(255))),
            ratatui::style::Color::Indexed(255)
        );
    }

    #[test]
    fn rgb_color() {
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Rgb(RgbColor(255, 128, 0))),
            ratatui::style::Color::Rgb(255, 128, 0)
        );
        assert_eq!(
            anstyle_to_ratatui_color(AnstyleColor::Rgb(RgbColor(0, 0, 0))),
            ratatui::style::Color::Rgb(0, 0, 0)
        );
    }

    #[test]
    fn all_16_ansi_colors_covered() {
        // Ensure every ANSI color maps to something other than Reset/Black
        // for non-Black colors.
        let colors = [
            (AnsiColor::Black, ratatui::style::Color::Black),
            (AnsiColor::Red, ratatui::style::Color::Red),
            (AnsiColor::Green, ratatui::style::Color::Green),
            (AnsiColor::Yellow, ratatui::style::Color::Yellow),
            (AnsiColor::Blue, ratatui::style::Color::Blue),
            (AnsiColor::Magenta, ratatui::style::Color::Magenta),
            (AnsiColor::Cyan, ratatui::style::Color::Cyan),
            (AnsiColor::White, ratatui::style::Color::White),
            (AnsiColor::BrightBlack, ratatui::style::Color::DarkGray),
            (AnsiColor::BrightRed, ratatui::style::Color::LightRed),
            (AnsiColor::BrightGreen, ratatui::style::Color::LightGreen),
            (AnsiColor::BrightYellow, ratatui::style::Color::LightYellow),
            (AnsiColor::BrightBlue, ratatui::style::Color::LightBlue),
            (
                AnsiColor::BrightMagenta,
                ratatui::style::Color::LightMagenta,
            ),
            (AnsiColor::BrightCyan, ratatui::style::Color::LightCyan),
            (AnsiColor::BrightWhite, ratatui::style::Color::White),
        ];
        for (input, expected) in colors {
            assert_eq!(
                anstyle_to_ratatui_color(AnstyleColor::Ansi(input)),
                expected,
                "mismatch for {input:?}"
            );
        }
    }
}
