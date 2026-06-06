use anstyle::{Color as AnsiColorEnum, Style as AnsiStyle};
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

/// Convert anstyle Color to ratatui Color.
///
/// Delegates to `vtcode_design::color::anstyle_to_ratatui_color` which
/// provides the correct mapping (fixing the Magenta bug).
pub fn ratatui_color_from_ansi(color: AnsiColorEnum) -> Color {
    vtcode_design::color::anstyle_to_ratatui_color(color)
}

pub fn ratatui_style_from_inline(
    style: &InlineTextStyle,
    fallback: Option<AnsiColorEnum>,
) -> Style {
    vtcode_design::style::inline_text_style_to_ratatui(
        style.color,
        style.bg_color,
        style.effects,
        fallback,
    )
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
    vtcode_design::style::anstyle_to_ratatui_style(style)
}
