//! Utilities for working with anstyle types and bridging to ratatui

use anstyle::{AnsiColor, Color as AnsiColorType, Effects, Style as AnsiStyle};
use ratatui::crossterm::style::Attribute;
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
        AnsiColorType::Rgb(rgb_color) => Color::Rgb(rgb_color.r(), rgb_color.g(), rgb_color.b()),
        _ => Color::Reset,
    }
}

/// Convert an anstyle Style to a ratatui Style using anstyle-crossterm as a bridge
pub fn anstyle_to_ratatui(anstyle: AnsiStyle) -> Style {
    let crossterm_style = anstyle_crossterm::to_crossterm(anstyle);
    let mut style = Style::default();

    if let Some(fg) = crossterm_style.foreground_color {
        style = style.fg(crossterm_color_to_ratatui(&fg));
    }

    if let Some(bg) = crossterm_style.background_color {
        style = style.bg(crossterm_color_to_ratatui(&bg));
    }

    let attrs = crossterm_style.attributes;
    apply_attributes(&mut style, attrs);

    style
}

fn apply_attributes(style: &mut Style, attrs: ratatui::crossterm::style::Attributes) {
    if attrs.has(Attribute::Bold) {
        *style = style.add_modifier(Modifier::BOLD);
    }
    if attrs.has(Attribute::Italic) {
        *style = style.add_modifier(Modifier::ITALIC);
    }
    if attrs.has(Attribute::Underlined) {
        *style = style.add_modifier(Modifier::UNDERLINED);
    }
    if attrs.has(Attribute::Dim) {
        *style = style.add_modifier(Modifier::DIM);
    }
    if attrs.has(Attribute::Reverse) {
        *style = style.add_modifier(Modifier::REVERSED);
    }
    if attrs.has(Attribute::SlowBlink) || attrs.has(Attribute::RapidBlink) {
        *style = style.add_modifier(Modifier::SLOW_BLINK);
    }
    if attrs.has(Attribute::CrossedOut) {
        *style = style.add_modifier(Modifier::CROSSED_OUT);
    }
}

fn crossterm_color_to_ratatui(color: &ratatui::crossterm::style::Color) -> Color {
    use ratatui::crossterm::style::Color as CColor;
    match color {
        CColor::Reset => Color::Reset,
        CColor::Black => Color::Black,
        CColor::DarkGrey => Color::DarkGray,
        CColor::Red => Color::Red,
        CColor::DarkRed => Color::Indexed(52),
        CColor::Green => Color::Green,
        CColor::DarkGreen => Color::Indexed(22),
        CColor::Yellow => Color::Yellow,
        CColor::DarkYellow => Color::Indexed(58),
        CColor::Blue => Color::Blue,
        CColor::DarkBlue => Color::Indexed(17),
        CColor::Magenta => Color::DarkGray,
        CColor::DarkMagenta => Color::DarkGray,
        CColor::Cyan => Color::Cyan,
        CColor::DarkCyan => Color::Indexed(23),
        CColor::White => Color::White,
        CColor::Grey => Color::Gray,
        CColor::Rgb { r, g, b } => Color::Rgb(*r, *g, *b),
        CColor::AnsiValue(code) => Color::Indexed(*code),
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
    if effects.contains(Effects::STRIKETHROUGH) {
        modifier.insert(Modifier::CROSSED_OUT);
    }

    modifier
}

/// Convert an style directly to a ratatui Style using manual mapping
pub fn ansi_style_to_ratatui_style(style: AnsiStyle) -> Style {
    let mut ratatui_style = Style::default();

    if let Some(fg_color) = style.get_fg_color() {
        ratatui_style = ratatui_style.fg(ansi_color_to_ratatui_color(&fg_color));
    }

    if let Some(bg_color) = style.get_bg_color() {
        ratatui_style = ratatui_style.bg(ansi_color_to_ratatui_color(&bg_color));
    }

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

/// Create a ratatui Style with a foreground color from anstyle
pub fn fg_color(color: anstyle::Color) -> Style {
    anstyle_to_ratatui(AnsiStyle::new().fg_color(Some(color)))
}

/// Create a ratatui Style with a background color from anstyle
pub fn bg_color(color: anstyle::Color) -> Style {
    anstyle_to_ratatui(AnsiStyle::new().bg_color(Some(color)))
}

/// Create a ratatui Style with foreground and background colors
pub fn fg_bg_colors(fg: anstyle::Color, bg: anstyle::Color) -> Style {
    anstyle_to_ratatui(AnsiStyle::new().fg_color(Some(fg)).bg_color(Some(bg)))
}

/// Create a ratatui Style with effects/modifiers
pub fn with_effects(effects: anstyle::Effects) -> Style {
    anstyle_to_ratatui(AnsiStyle::new().effects(effects))
}

/// Create a ratatui Style with foreground color and effects
pub fn colored_with_effects(color: anstyle::Color, effects: anstyle::Effects) -> Style {
    anstyle_to_ratatui(AnsiStyle::new().fg_color(Some(color)).effects(effects))
}

/// Create a ratatui Style with background color and effects
pub fn bg_colored_with_effects(color: anstyle::Color, effects: anstyle::Effects) -> Style {
    anstyle_to_ratatui(AnsiStyle::new().bg_color(Some(color)).effects(effects))
}

/// Create a complete ratatui Style from anstyle colors and effects
pub fn full_style(
    fg: Option<anstyle::Color>,
    bg: Option<anstyle::Color>,
    effects: anstyle::Effects,
) -> Style {
    let mut astyle = AnsiStyle::new();
    if let Some(c) = fg {
        astyle = astyle.fg_color(Some(c));
    }
    if let Some(c) = bg {
        astyle = astyle.bg_color(Some(c));
    }
    astyle = astyle.effects(effects);
    anstyle_to_ratatui(astyle)
}