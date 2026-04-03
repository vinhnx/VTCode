use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, RgbColor, Style as AnsiStyle};
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::ui::theme;

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

pub fn ratatui_color_from_ansi(color: AnsiColorEnum) -> Color {
    match color {
        AnsiColorEnum::Ansi(base) => match base {
            AnsiColor::Black => Color::Black,
            AnsiColor::Red => Color::Red,
            AnsiColor::Green => Color::Green,
            AnsiColor::Yellow => Color::Yellow,
            AnsiColor::Blue => Color::Blue,
            AnsiColor::Magenta => Color::DarkGray,
            AnsiColor::Cyan => Color::Cyan,
            AnsiColor::White => Color::White,
            AnsiColor::BrightBlack => Color::DarkGray,
            AnsiColor::BrightRed => Color::Red,
            AnsiColor::BrightGreen => Color::Green,
            AnsiColor::BrightYellow => Color::Yellow,
            AnsiColor::BrightBlue => Color::Blue,
            AnsiColor::BrightMagenta => Color::DarkGray,
            AnsiColor::BrightCyan => Color::Cyan,
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

    if let Some(color) = style.color.or(fallback) {
        resolved = resolved.fg(ratatui_color_from_ansi(color));
    }

    if let Some(color) = style.bg_color {
        resolved = resolved.bg(ratatui_color_from_ansi(color));
    }

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
    let inline = convert_style(style);
    ratatui_style_from_inline(&inline, None)
}
