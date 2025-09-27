use anstyle::{Color as AnsiColorEnum, Effects, Style as AnsiStyle};

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

pub fn convert_style(style: AnsiStyle) -> InlineTextStyle {
    let mut converted = InlineTextStyle {
        color: convert_style_color(&style),
        ..InlineTextStyle::default()
    };
    let effects = style.get_effects();
    converted.bold = effects.contains(Effects::BOLD);
    converted.italic = effects.contains(Effects::ITALIC);
    converted
}

pub fn theme_from_styles(styles: &theme::ThemeStyles) -> InlineTheme {
    InlineTheme {
        background: convert_ansi_color(styles.background),
        foreground: convert_ansi_color(styles.foreground),
        primary: convert_style_color(&styles.primary),
        secondary: convert_style_color(&styles.secondary),
    }
}
