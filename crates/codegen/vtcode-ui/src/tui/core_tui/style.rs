use anstyle::{Color as AnsiColorEnum, Style as AnsiStyle};
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::tui::ui::theme;

// Re-export from commons so existing consumers don't break.
pub use vtcode_commons::ui_protocol::{convert_style, theme_from_color_fields};

use super::types::{InlineTextStyle, InlineTheme};

pub fn theme_from_styles(styles: &theme::ThemeStyles) -> InlineTheme {
    theme_from_color_fields(
        styles.foreground,
        styles.background,
        styles.primary,
        styles.secondary,
        styles.tool,
        styles.tool_detail,
        styles.pty_output,
    )
}

pub fn measure_text_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
}

/// Convert anstyle Color to ratatui Color.
///
/// Delegates to `crate::design::color::anstyle_to_ratatui_color` which
/// provides the correct mapping (fixing the Magenta bug).
pub fn ratatui_color_from_ansi(color: AnsiColorEnum) -> Color {
    crate::design::color::anstyle_to_ratatui_color(color)
}

/// Parse a hex color string (e.g., "#D99A4E") to a ratatui Color.
/// Returns None if the string is invalid or cannot be parsed.
pub use crate::design::color::hex_to_ratatui_color;

/// Get the agent color style from an optional color token.
///
/// The token may be a primary-agent mode name (`"build"`), a standard ANSI hue
/// name (`"green"`), or a `#rrggbb` hex string. It is resolved theme-aware via
/// the design system so the badge stays legible on both dark and light
/// terminals, with `fallback_color` used when the token is empty or unknown.
pub fn agent_color_style(color: Option<&str>, fallback_color: Color) -> Style {
    let light = matches!(
        vtcode_commons::ansi_capabilities::detect_color_scheme(),
        vtcode_commons::ansi_capabilities::ColorScheme::Light
    );
    let color = color
        .map(|c| crate::design::color::resolve_agent_color(c, fallback_color, light))
        .unwrap_or(fallback_color);
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub fn ratatui_style_from_inline(
    style: &InlineTextStyle,
    fallback: Option<AnsiColorEnum>,
) -> Style {
    crate::design::style::inline_text_style_to_ratatui(
        style.color,
        style.bg_color,
        style.effects,
        fallback,
    )
}

/// PTY output style helper: keep configured color, suppress bold, enforce dimmed output.
pub fn ratatui_pty_style_from_inline(
    style: &InlineTextStyle,
    fallback: Option<AnsiColorEnum>,
) -> Style {
    ratatui_style_from_inline(style, fallback)
        .remove_modifier(Modifier::BOLD)
        .add_modifier(Modifier::DIM)
}

/// Convert an `anstyle::Style` directly to a `ratatui::style::Style`.
pub fn ratatui_style_from_ansi(style: AnsiStyle) -> Style {
    crate::design::style::anstyle_to_ratatui_style(style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::color::resolve_agent_color;
    use vtcode_config::constants::ui::{
        AGENT_COLOR_AUTO, AGENT_COLOR_BUILD, AGENT_COLOR_DUCK, AGENT_COLOR_PLAN,
    };

    #[test]
    fn agent_color_style_applies_mode_color_with_bold() {
        let fallback = Color::LightMagenta;
        let style = agent_color_style(Some(AGENT_COLOR_BUILD), fallback);
        // The exact variant depends on the detected terminal scheme, but it must
        // be a concrete (non-fallback) standard color and always bold.
        assert_ne!(style.fg, Some(fallback));
        assert!(style.add_modifier.contains(Modifier::BOLD));

        for hue in [
            AGENT_COLOR_BUILD,
            AGENT_COLOR_AUTO,
            AGENT_COLOR_PLAN,
            AGENT_COLOR_DUCK,
        ] {
            let s = agent_color_style(Some(hue), fallback);
            assert!(s.add_modifier.contains(Modifier::BOLD));
            assert!(s.fg.is_some());
        }
    }

    #[test]
    fn agent_color_style_is_theme_aware_and_distinct_per_mode() {
        let fallback = Color::LightMagenta;
        // On a dark terminal each mode resolves to its bright variant.
        let dark = [
            resolve_agent_color(AGENT_COLOR_BUILD, fallback, false),
            resolve_agent_color(AGENT_COLOR_AUTO, fallback, false),
            resolve_agent_color(AGENT_COLOR_PLAN, fallback, false),
            resolve_agent_color(AGENT_COLOR_DUCK, fallback, false),
        ];
        assert_eq!(
            dark,
            [
                Color::LightRed,
                Color::LightGreen,
                Color::LightBlue,
                Color::LightMagenta
            ]
        );
        // On a light terminal each mode resolves to its base variant.
        let light = [
            resolve_agent_color(AGENT_COLOR_BUILD, fallback, true),
            resolve_agent_color(AGENT_COLOR_AUTO, fallback, true),
            resolve_agent_color(AGENT_COLOR_PLAN, fallback, true),
            resolve_agent_color(AGENT_COLOR_DUCK, fallback, true),
        ];
        assert_eq!(
            light,
            [Color::Red, Color::Green, Color::Blue, Color::Magenta]
        );
        // The four modes must remain visually distinct in both appearances.
        assert_eq!(
            dark.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
        assert_eq!(
            light.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
    }

    #[test]
    fn agent_color_style_accepts_raw_hue_names_and_hex() {
        let fallback = Color::LightMagenta;
        // Raw standard ANSI hue name (as emitted by the plan-approval overlay).
        assert_eq!(
            resolve_agent_color("green", fallback, false),
            Color::LightGreen
        );
        assert_eq!(resolve_agent_color("blue", fallback, true), Color::Blue);
        // Legacy hex still resolves.
        assert_eq!(
            resolve_agent_color("#FF0000", fallback, false),
            Color::Rgb(255, 0, 0)
        );
    }

    #[test]
    fn agent_color_style_falls_back_when_missing_or_invalid() {
        let fallback = Color::LightMagenta;
        let missing = agent_color_style(None, fallback);
        assert_eq!(missing.fg, Some(fallback));
        assert!(missing.add_modifier.contains(Modifier::BOLD));

        let invalid = agent_color_style(Some("not-a-color"), fallback);
        assert_eq!(invalid.fg, Some(fallback));
    }
}
