//! Centralized styling helpers for consistent color and effect management.
//!
//! This module provides a unified interface for creating styled text using anstyle,
//! avoiding hardcoded ANSI codes and repeated color definitions.

use anstyle::{AnsiColor, Color, Effects, Style};

/// Standard color palette with semantic names
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub success: Color,  // Green
    pub error: Color,    // Red
    pub warning: Color,  // Yellow
    pub info: Color,     // Cyan
    pub accent: Color,   // Blue
    pub muted: Color,    // White/Gray (often dimmed)
}

impl ColorPalette {
    /// Default color palette
    pub fn default() -> Self {
        Self {
            success: Color::Ansi(AnsiColor::Green),
            error: Color::Ansi(AnsiColor::Red),
            warning: Color::Ansi(AnsiColor::Yellow),
            info: Color::Ansi(AnsiColor::Cyan),
            accent: Color::Ansi(AnsiColor::Blue),
            muted: Color::Ansi(AnsiColor::White),
        }
    }
}

/// Render text with a single color and optional effects
///
/// # Examples
///
/// ```ignore
/// let styled = render_styled("Success!", Color::Ansi(AnsiColor::Green), None);
/// println!("{}", styled);
///
/// let bold = render_styled("Bold text", Color::Ansi(AnsiColor::Blue), Some(Effects::BOLD));
/// println!("{}", bold);
/// ```
pub fn render_styled(text: &str, color: Color, effects: Option<Effects>) -> String {
    let mut style = Style::new().fg_color(Some(color));
    if let Some(e) = effects {
        style = style.effects(e);
    }
    format!("{style}{text}{}", style.render_reset())
}

/// Build a Style from a CSS/terminal color name
///
/// Supports: red, green, blue, yellow, cyan, magenta, purple, white, black
pub fn style_from_color_name(name: &str) -> Style {
    let color = match name.to_lowercase().as_str() {
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

    Style::new().fg_color(Some(color))
}

/// Create a bold colored style from AnsiColor
pub fn bold_color(color: AnsiColor) -> Style {
    Style::new()
        .bold()
        .fg_color(Some(color.into()))
}

/// Create a dimmed colored style from AnsiColor
pub fn dimmed_color(color: AnsiColor) -> Style {
    Style::new()
        .dimmed()
        .fg_color(Some(color.into()))
}

/// Create a style with foreground color (convenience function)
pub fn fg_color(color: AnsiColor) -> Style {
    Style::new().fg_color(Some(color.into()))
}

/// Create a style with effects only (no color)
pub fn with_effects(effects: Effects) -> Style {
    Style::new().effects(effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_palette_defaults() {
        let palette = ColorPalette::default();
        assert!(matches!(palette.success, Color::Ansi(AnsiColor::Green)));
        assert!(matches!(palette.error, Color::Ansi(AnsiColor::Red)));
        assert!(matches!(palette.warning, Color::Ansi(AnsiColor::Yellow)));
        assert!(matches!(palette.info, Color::Ansi(AnsiColor::Cyan)));
        assert!(matches!(palette.accent, Color::Ansi(AnsiColor::Blue)));
    }

    #[test]
    fn test_style_from_color_name_primary_colors() {
        let red = style_from_color_name("red");
        let green = style_from_color_name("green");
        let blue = style_from_color_name("blue");

        // Just verify they create non-empty styles
        assert!(!red.to_string().is_empty());
        assert!(!green.to_string().is_empty());
        assert!(!blue.to_string().is_empty());
    }

    #[test]
    fn test_style_from_color_name_purple_alias() {
        let magenta = style_from_color_name("magenta");
        let purple = style_from_color_name("purple");

        // Both should produce equivalent styles
        assert!(!magenta.to_string().is_empty());
        assert!(!purple.to_string().is_empty());
    }

    #[test]
    fn test_style_from_color_name_invalid() {
        let invalid = style_from_color_name("notacolor");
        // Should return a plain style
        assert!(invalid.get_fg_color().is_none());
    }

    #[test]
    fn test_render_styled_contains_reset() {
        let result = render_styled("test", Color::Ansi(AnsiColor::Green), None);
        assert!(result.contains("test"));
        // Should contain ANSI codes
        assert!(result.len() > "test".len());
    }

    #[test]
    fn test_render_styled_with_effects() {
        let result = render_styled(
            "bold",
            Color::Ansi(AnsiColor::Red),
            Some(Effects::BOLD),
        );
        assert!(result.contains("bold"));
    }

    #[test]
    fn test_bold_color() {
        let style = bold_color(AnsiColor::Blue);
        // Verify bold is set
        assert!(style.get_effects().contains(Effects::BOLD));
    }

    #[test]
    fn test_dimmed_color() {
        let style = dimmed_color(AnsiColor::Yellow);
        // Verify dimmed is set
        assert!(style.get_effects().contains(Effects::DIMMED));
    }

    #[test]
    fn test_fg_color() {
        let style = fg_color(AnsiColor::Cyan);
        assert!(style.get_fg_color().is_some());
    }

    #[test]
    fn test_with_effects() {
        let style = with_effects(Effects::UNDERLINE);
        assert!(style.get_effects().contains(Effects::UNDERLINE));
    }
}
