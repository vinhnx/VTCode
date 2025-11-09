// Central style factory and helpers
// Provides semantic color palettes and safe style construction

use anstyle::{AnsiColor, Color, Style};

/// Standard color palette with semantic names
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub success: Color,  // Green
    pub error: Color,    // Red
    pub warning: Color,  // Yellow
    pub info: Color,     // Cyan
    pub accent: Color,   // Blue
    pub primary: Color,  // Bright white
    pub muted: Color,    // Gray/Dim
}

impl ColorPalette {
    pub fn default() -> Self {
        Self {
            success: Color::Ansi(AnsiColor::Green),
            error: Color::Ansi(AnsiColor::Red),
            warning: Color::Ansi(AnsiColor::Yellow),
            info: Color::Ansi(AnsiColor::Cyan),
            accent: Color::Ansi(AnsiColor::Blue),
            primary: Color::Ansi(AnsiColor::BrightWhite),
            muted: Color::Ansi(AnsiColor::White),
        }
    }
}

/// Render text with a single color and optional effects
pub fn render_styled(text: &str, color: Color, effects: Option<String>) -> String {
    let style = Style::new().fg_color(Some(color));

    // Apply effects if provided (e.g., bold, dimmed)
    // For now, we accept effects as a parameter but don't use it
    // This allows future extension without changing the signature
    let _ = effects;

    format!("{}{}{}", style, text, style.render_reset())
}

/// Build style from CSS/terminal color name
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
        .fg_color(Some(Color::Ansi(color)))
}

/// Create a dimmed colored style from AnsiColor
pub fn dimmed_color(color: AnsiColor) -> Style {
    Style::new()
        .dimmed()
        .fg_color(Some(Color::Ansi(color)))
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
    fn test_style_from_color_name_valid() {
        let style_red = style_from_color_name("red");
        let style_green = style_from_color_name("green");
        let style_blue = style_from_color_name("blue");
        let style_cyan = style_from_color_name("cyan");

        // Just verify they're not empty/default
        assert!(!style_red.to_string().is_empty());
        assert!(!style_green.to_string().is_empty());
        assert!(!style_blue.to_string().is_empty());
        assert!(!style_cyan.to_string().is_empty());
    }

    #[test]
    fn test_style_from_color_name_case_insensitive() {
        let style_lower = style_from_color_name("red");
        let style_upper = style_from_color_name("RED");
        let style_mixed = style_from_color_name("ReD");

        // All should produce valid styles
        assert!(!style_lower.to_string().is_empty());
        assert!(!style_upper.to_string().is_empty());
        assert!(!style_mixed.to_string().is_empty());
    }

    #[test]
    fn test_style_from_color_name_invalid() {
        let style = style_from_color_name("invalid");
        assert_eq!(style.to_string(), "");
    }

    #[test]
    fn test_style_from_color_name_purple_alias() {
        let style_magenta = style_from_color_name("magenta");
        let style_purple = style_from_color_name("purple");

        assert!(!style_magenta.to_string().is_empty());
        assert!(!style_purple.to_string().is_empty());
    }

    #[test]
    fn test_render_styled_contains_reset() {
        let result = render_styled("test", Color::Ansi(AnsiColor::Green), None);
        assert!(result.contains("test"));
        // Verify it has ANSI codes
        assert!(result.contains("\x1b["));
    }

    #[test]
    fn test_render_styled_different_colors() {
        let green = render_styled("ok", Color::Ansi(AnsiColor::Green), None);
        let red = render_styled("error", Color::Ansi(AnsiColor::Red), None);

        assert!(green.contains("ok"));
        assert!(red.contains("error"));
        assert_ne!(green, red);
    }

    #[test]
    fn test_bold_color() {
        let style = bold_color(AnsiColor::Blue);
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_dimmed_color() {
        let style = dimmed_color(AnsiColor::Yellow);
        assert!(!style.to_string().is_empty());
    }
}
