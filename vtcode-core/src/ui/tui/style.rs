use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, RgbColor, Style as AnsiStyle};
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::ui::theme;

use super::types::{InlineTextStyle, InlineTheme};

fn convert_ansi_color(color: AnsiColorEnum) -> Option<AnsiColorEnum> {
    Some(match color {
        AnsiColorEnum::Ansi(ansi) => AnsiColorEnum::Ansi(ansi),
        AnsiColorEnum::Ansi256(value) => AnsiColorEnum::Ansi256(value),
        AnsiColorEnum::Rgb(rgb) => AnsiColorEnum::Rgb(rgb),
    })
}

fn convert_style_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_fg_color().and_then(convert_ansi_color)
}

fn convert_style_bg_color(style: &AnsiStyle) -> Option<AnsiColorEnum> {
    style.get_bg_color().and_then(convert_ansi_color)
}

pub fn convert_style(style: AnsiStyle) -> InlineTextStyle {
    InlineTextStyle {
        color: convert_style_color(&style),
        bg_color: convert_style_bg_color(&style),
        effects: style.get_effects(),
    }
}

pub fn theme_from_styles(styles: &theme::ThemeStyles) -> InlineTheme {
    InlineTheme {
        foreground: convert_ansi_color(styles.foreground),
        primary: convert_style_color(&styles.primary),
        secondary: convert_style_color(&styles.secondary),
        tool_accent: convert_style_color(&styles.tool),
        tool_body: convert_style_color(&styles.tool_detail),
    }
}

pub fn measure_text_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
}

pub fn ratatui_color_from_ansi(color: AnsiColorEnum) -> Color {
    match color {
        AnsiColorEnum::Ansi(base) => match base {
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
            AnsiColor::BrightWhite => Color::Gray,
        },
        AnsiColorEnum::Ansi256(value) => Color::Indexed(value.index()),
        AnsiColorEnum::Rgb(RgbColor(red, green, blue)) => Color::Rgb(red, green, blue),
    }
}

pub fn ratatui_style_from_inline(
    style: &InlineTextStyle,
    fallback: Option<AnsiColorEnum>,
) -> Style {
    let mut resolved = Style::default();
    
    // Foreground color
    if let Some(color) = style.color.or(fallback) {
        resolved = resolved.fg(ratatui_color_from_ansi(color));
    }
    
    // Background color
    if let Some(color) = style.bg_color {
        resolved = resolved.bg(ratatui_color_from_ansi(color));
    }
    
    // Effects bitmask
    if style.effects.contains(Effects::BOLD) {
        resolved = resolved.add_modifier(Modifier::BOLD);
    }
    if style.effects.contains(Effects::ITALIC) {
        resolved = resolved.add_modifier(Modifier::ITALIC);
    }
    if style.effects.contains(Effects::UNDERLINE) {
        resolved = resolved.add_modifier(Modifier::UNDERLINED);
    }
    if style.effects.contains(Effects::DIMMED) {
        resolved = resolved.add_modifier(Modifier::DIM);
    }
    
    resolved
}
