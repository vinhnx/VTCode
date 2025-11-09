// Central styling helpers to avoid hardcoded ANSI codes and repeated patterns

use anstyle::{AnsiColor, Color, Effects, Style};

/// Standard color palette with semantic names
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub success: Color,      // Green
    pub error: Color,        // Red
    pub warning: Color,      // Yellow
    pub info: Color,         // Cyan
    pub accent: Color,       // Blue
    pub primary: Color,      // White
    pub muted: Color,        // Gray
}

impl ColorPalette {
    pub fn default() -> Self {
        Self {
            success: Color::Ansi(AnsiColor::Green),
            error: Color::Ansi(AnsiColor::Red),
            warning: Color::Ansi(AnsiColor::Yellow),
            info: Color::Ansi(AnsiColor::Cyan),
            accent: Color::Ansi(AnsiColor::Blue),
            primary: Color::Ansi(AnsiColor::White),
            muted: Color::Ansi(AnsiColor::White),
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
        assert!(matches!(palette.info, Color::Ansi(AnsiColor::Cyan)));
        assert!(matches!(palette.primary, Color::Ansi(AnsiColor::White)));
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
    fn test_color_name_case_insensitive() {
        let style1 = style_from_color_name("RED");
        let style2 = style_from_color_name("red");
        assert_eq!(style1.to_string(), style2.to_string());
    }

    #[test]
    fn test_unknown_color_returns_empty_style() {
        let style = style_from_color_name("nonexistent");
        assert_eq!(style.to_string(), Style::new().to_string());
    }
}
