//! Unified color conversion between `anstyle` and `ratatui`.
//!
//! This module provides the single correct mapping from `anstyle::Color` to
//! `ratatui::style::Color`. Previous implementations in `vtcode-commons` and
//! `vtcode-ui` had bugs mapping `Magenta` and bright variants to incorrect
//! ratatui colors.

use anstyle::{AnsiColor, Color as AnstyleColor, RgbColor};
use ratatui::style::Color;

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
/// `vtcode-ui::tui::core_tui::style` incorrectly mapped:
/// - `Magenta` to `DarkGray` (now correctly `Magenta`)
/// - `BrightMagenta` to `DarkGray` (now correctly `LightMagenta`)
/// - `BrightRed/Green/Yellow/Blue/Cyan` to non-bright variants
/// - `Ansi256` colors to `Reset` instead of `Indexed`
pub fn anstyle_to_ratatui_color(color: AnstyleColor) -> Color {
    match color {
        AnstyleColor::Ansi(ansi) => ansi_to_ratatui(ansi),
        AnstyleColor::Ansi256(c) => Color::Indexed(c.0),
        AnstyleColor::Rgb(RgbColor(r, g, b)) => Color::Rgb(r, g, b),
    }
}

/// Map a standard ANSI color (0-15) to its ratatui equivalent.
fn ansi_to_ratatui(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Black => Color::Black,
        AnsiColor::Red => Color::Red,
        AnsiColor::Green => Color::Green,
        AnsiColor::Yellow => Color::Yellow,
        AnsiColor::Blue => Color::Blue,
        AnsiColor::Magenta => Color::Magenta,
        AnsiColor::Cyan => Color::Cyan,
        AnsiColor::White => Color::White,
        AnsiColor::BrightBlack => Color::DarkGray,
        AnsiColor::BrightRed => Color::LightRed,
        AnsiColor::BrightGreen => Color::LightGreen,
        AnsiColor::BrightYellow => Color::LightYellow,
        AnsiColor::BrightBlue => Color::LightBlue,
        AnsiColor::BrightMagenta => Color::LightMagenta,
        AnsiColor::BrightCyan => Color::LightCyan,
        AnsiColor::BrightWhite => Color::White,
    }
}

/// Map a standard ANSI hue name to its `(dark_background, light_background)`
/// `ratatui` color variants.
///
/// This is the design-system's portable way to keep agent/mode badges readable
/// in BOTH terminal appearances: the brighter `Light*` variant is used on dark
/// backgrounds, the base variant on light backgrounds. Names are kept in sync
/// with `AGENT_HUE_NAMES` in `vtcode-config`.
fn ansi_hue_variant(hue: &str, light: bool) -> Option<Color> {
    let (dark, lit) = match hue {
        "red" => (Color::LightRed, Color::Red),
        "green" => (Color::LightGreen, Color::Green),
        "blue" => (Color::LightBlue, Color::Blue),
        "magenta" => (Color::LightMagenta, Color::Magenta),
        "yellow" => (Color::LightYellow, Color::Yellow),
        "cyan" => (Color::LightCyan, Color::Cyan),
        _ => return None,
    };
    Some(if light { lit } else { dark })
}

/// Parse a hex color string (e.g. `"#D99A4E"`) to a `ratatui` `Color`.
/// Returns `None` if the string is not a valid `#rrggbb` value.
pub fn hex_to_ratatui_color(hex: &str) -> Option<Color> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

/// Resolve a mode/agent color token to a `ratatui` color.
///
/// Tries, in order:
/// 1. A known primary-agent mode name (e.g. `"build"`) — mapped via the
///    `vtcode-config` canonical table to a standard ANSI hue.
/// 2. A raw standard ANSI hue name (e.g. `"green"`) — used directly (this is
///    what the plan-approval overlay emits).
/// 3. A `#rrggbb` hex string — retained for back-compat with custom agents.
///
/// Hue/mode tokens are resolved to the variant matching `light`, so a single
/// token reads well on both dark and light terminals. Falls back to
/// `fallback` when the token is unknown or unparseable.
pub fn resolve_agent_color(token: &str, fallback: Color, light: bool) -> Color {
    use vtcode_config::constants::ui::agent_mode_hue;

    agent_mode_hue(token)
        .and_then(|h| ansi_hue_variant(h, light))
        .or_else(|| ansi_hue_variant(token, light))
        .or_else(|| hex_to_ratatui_color(token))
        .unwrap_or(fallback)
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
