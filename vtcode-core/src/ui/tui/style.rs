use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, Style as AnsiStyle};
use ratatui::style::Color;
use serde::de::value::{Error as DeValueError, StrDeserializer};

use crate::ui::theme;

use super::types::{RatatuiTextStyle, RatatuiTheme};

fn convert_ansi_color(color: AnsiColorEnum) -> Option<Color> {
    match color {
        AnsiColorEnum::Ansi(ansi) => Some(match ansi {
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
        }),
        AnsiColorEnum::Ansi256(value) => Some(Color::Indexed(value.0)),
        AnsiColorEnum::Rgb(rgb) => Some(Color::Rgb(rgb.0, rgb.1, rgb.2)),
    }
}

fn convert_style_color(style: &AnsiStyle) -> Option<Color> {
    style.get_fg_color().and_then(convert_ansi_color)
}

pub fn convert_style(style: AnsiStyle) -> RatatuiTextStyle {
    let mut converted = RatatuiTextStyle {
        color: convert_style_color(&style),
        ..Default::default()
    };
    let effects = style.get_effects();
    converted.bold = effects.contains(Effects::BOLD);
    converted.italic = effects.contains(Effects::ITALIC);
    converted
}

pub fn parse_tui_color(input: &str) -> Option<Color> {
    let deserializer = StrDeserializer::<DeValueError>::new(input);
    color_to_tui::deserialize(deserializer).ok()
}

pub fn theme_from_styles(styles: &theme::ThemeStyles) -> RatatuiTheme {
    RatatuiTheme {
        background: convert_ansi_color(styles.background),
        foreground: convert_ansi_color(styles.foreground),
        primary: convert_style_color(&styles.primary),
        secondary: convert_style_color(&styles.secondary),
    }
}
