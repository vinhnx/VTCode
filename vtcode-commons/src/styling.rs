//! Unified message styles and their logical mappings

use anstyle::{AnsiColor, Color, Effects, RgbColor, Style};

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
        let mut ansi_effects = Effects::new();

        for effect in effects_str.split(',') {
            let effect = effect.trim().to_lowercase();
            match effect.as_str() {
                "bold" => ansi_effects |= Effects::BOLD,
                "dim" | "dimmed" => ansi_effects |= Effects::DIMMED,
                "italic" => ansi_effects |= Effects::ITALIC,
                "underline" => ansi_effects |= Effects::UNDERLINE,
                "blink" => ansi_effects |= Effects::BLINK,
                "invert" | "reversed" => ansi_effects |= Effects::INVERT,
                "hidden" => ansi_effects |= Effects::HIDDEN,
                "strikethrough" => ansi_effects |= Effects::STRIKETHROUGH,
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
/// Uses standard ANSI colors without bold for accessibility and consistency.
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
            added_bg: Color::Rgb(RgbColor(10, 24, 10)),
            removed_fg: Color::Ansi(AnsiColor::Red),
            removed_bg: Color::Rgb(RgbColor(24, 10, 10)),
            header_fg: Color::Ansi(AnsiColor::Cyan),
            header_bg: Color::Rgb(RgbColor(10, 16, 20)),
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

// Re-export diff theme configuration
pub use crate::diff_theme::{
    DiffColorLevel, DiffTheme, diff_add_bg, diff_del_bg, diff_gutter_bg_add_light,
    diff_gutter_bg_del_light, diff_gutter_fg_light,
};
