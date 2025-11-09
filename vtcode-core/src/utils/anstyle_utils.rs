//! Utilities for working with anstyle types
//!
//! This module provides utilities for converting anstyle types directly to ratatui types
//! without going through the crossterm bridge, giving more predictable color mapping.

use anstyle::{AnsiColor, Color as AnsiColorType, Effects, Style as AnsiStyle};
use ratatui::style::{Color, Modifier, Style};

/// Convert anstyle Color to ratatui Color directly
pub fn ansi_color_to_ratatui_color(color: &AnsiColorType) -> Color {
    match color {
        AnsiColorType::Ansi(ansi_color) => match ansi_color {
            AnsiColor::Black => Color::Black,
            AnsiColor::Red => Color::Red,
            AnsiColor::Green => Color::Green,
            AnsiColor::Yellow => Color::Yellow,
            AnsiColor::Blue => Color::Blue,
            AnsiColor::Magenta => Color::Magenta,
            AnsiColor::Cyan => Color::Cyan,
            AnsiColor::White => Color::White,
            AnsiColor::BrightBlack => Color::Gray,
            AnsiColor::BrightRed => Color::LightRed,
            AnsiColor::BrightGreen => Color::LightGreen,
            AnsiColor::BrightYellow => Color::LightYellow,
            AnsiColor::BrightBlue => Color::LightBlue,
            AnsiColor::BrightMagenta => Color::LightMagenta,
            AnsiColor::BrightCyan => Color::LightCyan,
            AnsiColor::BrightWhite => Color::White, // or maybe a custom RGB white
        },
        AnsiColorType::Rgb(rgb_color) => Color::Rgb(rgb_color.r(), rgb_color.g(), rgb_color.b()),
        // For custom color handling, you can add more mappings
        _ => Color::Reset,
    }
}

/// Convert anstyle Effects to ratatui Modifiers
pub fn ansi_effects_to_ratatui_modifiers(effects: Effects) -> Modifier {
    let mut modifier = Modifier::empty();
    
    if effects.contains(Effects::BOLD) {
        modifier.insert(Modifier::BOLD);
    }
    if effects.contains(Effects::DIMMED) {
        modifier.insert(Modifier::DIM);
    }
    if effects.contains(Effects::ITALIC) {
        modifier.insert(Modifier::ITALIC);
    }
    if effects.contains(Effects::UNDERLINE) {
        modifier.insert(Modifier::UNDERLINED);
    }
    if effects.contains(Effects::BLINK) {
        modifier.insert(Modifier::SLOW_BLINK);
    }
    if effects.contains(Effects::INVERT) {
        modifier.insert(Modifier::REVERSED);
    }
    if effects.contains(Effects::HIDDEN) {
        // No direct ratatui equivalent, but could be handled if needed
    }
    if effects.contains(Effects::STRIKETHROUGH) {
        modifier.insert(Modifier::CROSSED_OUT);
    }
    
    modifier
}

/// Convert anstyle Style directly to ratatui Style
/// This is a more direct conversion than using the crossterm bridge
pub fn ansi_style_to_ratatui_style(style: AnsiStyle) -> Style {
    let mut ratatui_style = Style::default();

    // Apply foreground color
    if let Some(fg_color) = style.get_fg_color() {
        ratatui_style = ratatui_style.fg(ansi_color_to_ratatui_color(&fg_color));
    }

    // Apply background color
    if let Some(bg_color) = style.get_bg_color() {
        ratatui_style = ratatui_style.bg(ansi_color_to_ratatui_color(&bg_color));
    }

    // Apply effects
    let modifiers = ansi_effects_to_ratatui_modifiers(style.get_effects());
    ratatui_style = ratatui_style.add_modifier(modifiers);

    ratatui_style
}

/// A convenience function that combines color, background, and effects into a single ratatui Style
pub fn build_ratatui_style(
    fg_color: Option<AnsiColorType>,
    bg_color: Option<AnsiColorType>,
    effects: Effects,
) -> Style {
    let mut style = Style::default();

    if let Some(fg) = fg_color {
        style = style.fg(ansi_color_to_ratatui_color(&fg));
    }

    if let Some(bg) = bg_color {
        style = style.bg(ansi_color_to_ratatui_color(&bg));
    }

    let modifiers = ansi_effects_to_ratatui_modifiers(effects);
    style = style.add_modifier(modifiers);

    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, RgbColor};

    #[test]
    fn test_ansi_color_conversion() {
        assert_eq!(ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::Red)), Color::Red);
        assert_eq!(ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::Green)), Color::Green);
        assert_eq!(ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::Blue)), Color::Blue);
        assert_eq!(ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::White)), Color::White);
    }

    #[test]
    fn test_rgb_color_conversion() {
        let rgb = RgbColor(255, 128, 0);
        let ansi_color = AnsiColorEnum::Rgb(rgb);
        let result = ansi_color_to_ratatui_color(&ansi_color);
        assert_eq!(result, Color::Rgb(255, 128, 0));
    }

    #[test]
    fn test_effects_conversion() {
        let effects = Effects::BOLD | Effects::ITALIC | Effects::UNDERLINE;
        let modifiers = ansi_effects_to_ratatui_modifiers(effects);
        
        assert!(modifiers.contains(Modifier::BOLD));
        assert!(modifiers.contains(Modifier::ITALIC));
        assert!(modifiers.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_ansi_style_to_ratatui_style() {
        let ansi_style = anstyle::Style::new()
            .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Green)))
            .bg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Blue)))
            .effects(Effects::BOLD | Effects::ITALIC);

        let ratatui_style = ansi_style_to_ratatui_style(ansi_style);

        assert_eq!(ratatui_style.fg, Some(Color::Green));
        assert_eq!(ratatui_style.bg, Some(Color::Blue));
        assert!(ratatui_style.add_modifier.contains(Modifier::BOLD));
        assert!(ratatui_style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn test_build_ratatui_style() {
        let style = build_ratatui_style(
            Some(AnsiColorEnum::Ansi(AnsiColor::Yellow)),
            Some(AnsiColorEnum::Ansi(AnsiColor::Black)),
            Effects::BOLD,
        );

        assert_eq!(style.fg, Some(Color::Yellow));
        assert_eq!(style.bg, Some(Color::Black));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_bright_colors() {
        assert_eq!(ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::BrightRed)), Color::LightRed);
        assert_eq!(ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::BrightGreen)), Color::LightGreen);
        assert_eq!(ansi_color_to_ratatui_color(&AnsiColorEnum::Ansi(AnsiColor::BrightBlue)), Color::LightBlue);
    }
}