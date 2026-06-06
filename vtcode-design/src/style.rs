//! Unified style bridging between `anstyle` and `ratatui`.
//!
//! Converts `anstyle::Style` and `InlineTextStyle` to `ratatui::style::Style`,
//! using the correct color mapping from [`crate::color`].

use anstyle::{Color as AnstyleColor, Effects, Style as AnstyleStyle};
use ratatui::style::{Modifier, Style};

use crate::color::anstyle_to_ratatui_color;

/// Convert an `anstyle::Style` directly to a `ratatui::style::Style`.
///
/// Uses the correct color mapping from [`crate::color::anstyle_to_ratatui_color`].
pub fn anstyle_to_ratatui_style(style: AnstyleStyle) -> Style {
    let mut ratatui_style = Style::default();

    if let Some(fg) = style.get_fg_color() {
        ratatui_style = ratatui_style.fg(anstyle_to_ratatui_color(fg));
    }

    if let Some(bg) = style.get_bg_color() {
        ratatui_style = ratatui_style.bg(anstyle_to_ratatui_color(bg));
    }

    ratatui_style = ratatui_style.add_modifier(effects_to_modifiers(style.get_effects()));
    ratatui_style
}

/// Convert an `InlineTextStyle` to a `ratatui::style::Style`.
///
/// If the style has no foreground color, the `fallback` is used.
///
/// `InlineTextStyle` is from `vtcode_commons::ui_protocol::InlineTextStyle`.
/// We accept its fields individually here to avoid a direct dependency on the
/// type, keeping the coupling loose.
pub fn inline_text_style_to_ratatui(
    color: Option<AnstyleColor>,
    bg_color: Option<AnstyleColor>,
    effects: Effects,
    fallback: Option<AnstyleColor>,
) -> Style {
    let mut resolved = Style::default();

    if let Some(c) = color.or(fallback) {
        resolved = resolved.fg(anstyle_to_ratatui_color(c));
    }

    if let Some(c) = bg_color {
        resolved = resolved.bg(anstyle_to_ratatui_color(c));
    }

    resolved = resolved.add_modifier(effects_to_modifiers(effects));
    resolved
}

/// Convert `anstyle::Effects` to `ratatui::style::Modifier`.
pub fn effects_to_modifiers(effects: Effects) -> Modifier {
    let mut modifier = Modifier::empty();

    if effects.contains(Effects::BOLD) {
        modifier.insert(Modifier::BOLD);
    }
    if effects.contains(Effects::DIMMED) {
        modifier.insert(Modifier::DIM);
    }
    if effects.contains(Effects::ITALIC) {
        modifier.insert(Modifier::ITALIC);
    }
    if effects.contains(Effects::UNDERLINE) {
        modifier.insert(Modifier::UNDERLINED);
    }
    if effects.contains(Effects::BLINK) {
        modifier.insert(Modifier::SLOW_BLINK);
    }
    if effects.contains(Effects::INVERT) {
        modifier.insert(Modifier::REVERSED);
    }
    if effects.contains(Effects::STRIKETHROUGH) {
        modifier.insert(Modifier::CROSSED_OUT);
    }

    modifier
}

/// Create a `ratatui::Style` with a foreground color.
pub fn fg_style(color: AnstyleColor) -> Style {
    Style::default().fg(anstyle_to_ratatui_color(color))
}

/// Create a `ratatui::Style` with a background color.
pub fn bg_style(color: AnstyleColor) -> Style {
    Style::default().bg(anstyle_to_ratatui_color(color))
}

/// Create a `ratatui::Style` with foreground and background colors.
pub fn fg_bg_style(fg: AnstyleColor, bg: AnstyleColor) -> Style {
    Style::default()
        .fg(anstyle_to_ratatui_color(fg))
        .bg(anstyle_to_ratatui_color(bg))
}

/// Create a `ratatui::Style` with effects/modifiers.
pub fn with_effects(effects: Effects) -> Style {
    Style::default().add_modifier(effects_to_modifiers(effects))
}

/// Create a `ratatui::Style` with foreground color and effects.
pub fn colored_with_effects(color: AnstyleColor, effects: Effects) -> Style {
    Style::default()
        .fg(anstyle_to_ratatui_color(color))
        .add_modifier(effects_to_modifiers(effects))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anstyle_style_with_fg() {
        let input =
            AnstyleStyle::new().fg_color(Some(AnstyleColor::Ansi(anstyle::AnsiColor::Green)));
        let result = anstyle_to_ratatui_style(input);
        assert_eq!(result.fg, Some(ratatui::style::Color::Green));
    }

    #[test]
    fn anstyle_style_with_magenta_fg() {
        // Regression test: Magenta should map to Magenta, not DarkGray.
        let input =
            AnstyleStyle::new().fg_color(Some(AnstyleColor::Ansi(anstyle::AnsiColor::Magenta)));
        let result = anstyle_to_ratatui_style(input);
        assert_eq!(result.fg, Some(ratatui::style::Color::Magenta));
    }

    #[test]
    fn anstyle_style_with_bg() {
        let input = AnstyleStyle::new().bg_color(Some(AnstyleColor::Ansi(anstyle::AnsiColor::Red)));
        let result = anstyle_to_ratatui_style(input);
        assert_eq!(result.bg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn anstyle_style_with_effects() {
        let input = AnstyleStyle::new().bold().italic();
        let result = anstyle_to_ratatui_style(input);
        assert!(result.add_modifier.contains(Modifier::BOLD));
        assert!(result.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn inline_text_style_with_fallback() {
        let result = inline_text_style_to_ratatui(
            None,
            None,
            Effects::BOLD,
            Some(AnstyleColor::Ansi(anstyle::AnsiColor::Cyan)),
        );
        assert_eq!(result.fg, Some(ratatui::style::Color::Cyan));
        assert!(result.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn inline_text_style_color_overrides_fallback() {
        let result = inline_text_style_to_ratatui(
            Some(AnstyleColor::Ansi(anstyle::AnsiColor::Red)),
            None,
            Effects::new(),
            Some(AnstyleColor::Ansi(anstyle::AnsiColor::Cyan)),
        );
        assert_eq!(result.fg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn effects_to_modifiers_all() {
        let effects = Effects::BOLD
            | Effects::DIMMED
            | Effects::ITALIC
            | Effects::UNDERLINE
            | Effects::BLINK
            | Effects::INVERT
            | Effects::STRIKETHROUGH;
        let modifier = effects_to_modifiers(effects);
        assert!(modifier.contains(Modifier::BOLD));
        assert!(modifier.contains(Modifier::DIM));
        assert!(modifier.contains(Modifier::ITALIC));
        assert!(modifier.contains(Modifier::UNDERLINED));
        assert!(modifier.contains(Modifier::SLOW_BLINK));
        assert!(modifier.contains(Modifier::REVERSED));
        assert!(modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn convenience_fg_style() {
        let s = fg_style(AnstyleColor::Ansi(anstyle::AnsiColor::Blue));
        assert_eq!(s.fg, Some(ratatui::style::Color::Blue));
    }

    #[test]
    fn convenience_bg_style() {
        let s = bg_style(AnstyleColor::Ansi(anstyle::AnsiColor::Red));
        assert_eq!(s.bg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn convenience_fg_bg_style() {
        let s = fg_bg_style(
            AnstyleColor::Ansi(anstyle::AnsiColor::Green),
            AnstyleColor::Ansi(anstyle::AnsiColor::Black),
        );
        assert_eq!(s.fg, Some(ratatui::style::Color::Green));
        assert_eq!(s.bg, Some(ratatui::style::Color::Black));
    }

    #[test]
    fn convenience_colored_with_effects() {
        let s = colored_with_effects(
            AnstyleColor::Ansi(anstyle::AnsiColor::Yellow),
            Effects::BOLD | Effects::ITALIC,
        );
        assert_eq!(s.fg, Some(ratatui::style::Color::Yellow));
        assert!(s.add_modifier.contains(Modifier::BOLD));
        assert!(s.add_modifier.contains(Modifier::ITALIC));
    }
}
