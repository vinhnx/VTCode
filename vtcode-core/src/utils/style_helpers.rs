// vtcode-core/src/utils/style_helpers.rs
//! Centralized styling helpers to reduce hardcoded colors and repeated patterns

use anstyle::{AnsiColor, Color, Style, Effects};

/// Standard color palette with semantic names
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub success: Color,      // Green
    pub error: Color,        // Red
    pub warning: Color,      // Yellow
    pub info: Color,         // Cyan
    pub accent: Color,       // Blue
    pub muted: Color,        // Gray/Dim
}

impl ColorPalette {
    pub fn default() -> Self {
        Self {
            success: Color::Ansi(AnsiColor::Green),
            error: Color::Ansi(AnsiColor::Red),
            warning: Color::Ansi(AnsiColor::Yellow),
            info: Color::Ansi(AnsiColor::Cyan),
            accent: Color::Ansi(AnsiColor::Blue),
            muted: Color::Ansi(AnsiColor::White), // Will be dimmed
        }
    }
}

/// Render text with a single color
pub fn render_styled(text: &str, color: Color, effects: Option<Effects>) -> String {
    let mut style = Style::new().fg_color(Some(color));
    if let Some(e) = effects {
        style = style.effects(e);
    }
    format!("{style}{text}{}", style.render_reset())
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

/// Create a bold colored style
pub fn bold_color(color: AnsiColor) -> Style {
    Style::new()
        .bold()
        .fg_color(Some(color.into()))
}

/// Create a dimmed style
pub fn dimmed_color(color: AnsiColor) -> Style {
    Style::new()
        .dimmed()
        .fg_color(Some(color.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_palette_defaults() {
        let palette = ColorPalette::default();
        assert!(matches!(palette.success, Color::Ansi(AnsiColor::Green)));
        assert!(matches!(palette.error, Color::Ansi(AnsiColor::Red)));
    }

    #[test]
    fn test_style_from_color_name() {
        let style = style_from_color_name("red");
        assert!(!style.to_string().is_empty());
    }

    #[test]
    fn test_render_styled_contains_reset() {
        let result = render_styled("test", Color::Ansi(AnsiColor::Green), None);
        assert!(result.contains("\x1b"));
        assert!(result.contains("test"));
    }

    #[test]
    fn test_style_from_color_name_case_insensitive() {
        let style1 = style_from_color_name("RED");
        let style2 = style_from_color_name("red");
        assert_eq!(style1.to_string(), style2.to_string());
    }

    #[test]
    fn test_bold_color_contains_bold() {
        let style = bold_color(AnsiColor::Green);
        assert!(style.to_string().contains("\x1b"));
    }
}
