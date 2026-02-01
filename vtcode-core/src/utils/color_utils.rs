//! Advanced color utilities for anstyle
//!
//! Re-exports from vtcode-commons for backward compatibility.

pub use vtcode_commons::colors::*;
use anstyle::{Color, Effects, Style as AnsiStyle};

/// Create a style with enhanced effects
pub fn enhanced_style(fg: Option<Color>, bg: Option<Color>, effects: Effects) -> AnsiStyle {
    let mut style = AnsiStyle::new();

    if let Some(fg_color) = fg {
        style = style.fg_color(Some(fg_color));
    }

    if let Some(bg_color) = bg {
        style = style.bg_color(Some(bg_color));
    }

    style = style.effects(effects);

    style
}

/// Create a bold underline style for highlighting
pub fn bold_underline(fg: Option<Color>) -> AnsiStyle {
    enhanced_style(fg, None, Effects::BOLD | Effects::UNDERLINE)
}

/// Create a dim italic style for secondary text
pub fn dim_italic(fg: Option<Color>) -> AnsiStyle {
    enhanced_style(fg, None, Effects::DIMMED | Effects::ITALIC)
}

/// Create a style with inverted colors
pub fn inverted(fg: Option<Color>, bg: Option<Color>) -> AnsiStyle {
    enhanced_style(
        bg, // Swap fg and bg
        fg,
        Effects::INVERT,
    )
}