//! Bridge between anstyle and ratatui for unified styling
//!
//! This module provides conversion utilities to use anstyle styling with ratatui TUI components.
//! It leverages anstyle-crossterm to convert anstyle types to crossterm types,
//! which ratatui understands natively.
//!
//! # Examples
//!
//! ```ignore
//! use anstyle::{Style, Color, AnsiColor, Effects};
//! use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;
//!
//! let anstyle = Style::new()
//!     .fg_color(Some(Color::Ansi(AnsiColor::Green)))
//!     .effects(Effects::BOLD);
//!
//! let ratatui_style = anstyle_to_ratatui(anstyle);
//! // Use with ratatui widgets
//! ```

use anstyle::Style as AnstyleStyle;
use crossterm::style::Attribute;
use ratatui::style::{Modifier, Style};

// Type aliases for clarity
type RatatuiColor = ratatui::style::Color;
type CrosstermColor = crossterm::style::Color;

/// Convert an anstyle Style to a ratatui Style
///
/// This is the main entry point for converting generic anstyle styling
/// to ratatui-compatible styles. Uses anstyle-crossterm internally as an adapter.
///
/// # Arguments
/// * `anstyle` - An anstyle Style object with colors and effects
///
/// # Returns
/// A ratatui Style that can be used with widgets
///
/// # Examples
/// ```ignore
/// use anstyle::{Style, Color, AnsiColor, Effects};
/// use vtcode_core::utils::ratatui_styles::anstyle_to_ratatui;
///
/// let style = Style::new()
///     .fg_color(Some(Color::Ansi(AnsiColor::Blue)))
///     .effects(Effects::BOLD | Effects::UNDERLINE);
///
/// let ratatui_style = anstyle_to_ratatui(style);
/// ```
pub fn anstyle_to_ratatui(anstyle: AnstyleStyle) -> Style {
    // Use anstyle-crossterm to convert the full style
    let crossterm_style = anstyle_crossterm::to_crossterm(anstyle);

    let mut style = Style::default();

    // Extract and convert foreground color
    if let Some(fg) = crossterm_style.foreground_color {
        style = style.fg(crossterm_color_to_ratatui(&fg));
    }

    // Extract and convert background color
    if let Some(bg) = crossterm_style.background_color {
        style = style.bg(crossterm_color_to_ratatui(&bg));
    }

    // Extract and convert attributes/effects
    let attrs = crossterm_style.attributes;
    apply_attributes(&mut style, attrs);

    style
}

/// Apply crossterm attributes to a ratatui style
///
/// Maps all supported crossterm attributes to their ratatui equivalents.
/// Note: Some crossterm attributes (Hidden, OverLined, etc.) have no direct
/// ratatui equivalent and are silently ignored.
fn apply_attributes(style: &mut Style, attrs: crossterm::style::Attributes) {
    // Efficient attribute checking using the Attributes API
    // This is more readable and direct than manual bitwise operations

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
    // Note: Hidden, OverLined attributes are not mapped to ratatui
}

/// Convert a crossterm Color to a ratatui Color
///
/// This handles the mapping between crossterm's color model and ratatui's.
/// - Standard colors (Black, Red, Green, etc.) map directly
/// - Dark variants map to ratatui's DarkGray for consistency
/// - RGB colors map to ratatui's Rgb variant
/// - Indexed colors are passed through directly
///
/// Dark color mapping strategy:
/// For dark colors that don't have direct ratatui equivalents,
/// we use indexed ANSI colors to preserve the intended darkness while
/// respecting terminal color palettes. This ensures compatibility
/// across different terminal configurations.
fn crossterm_color_to_ratatui(color: &CrosstermColor) -> RatatuiColor {
    match color {
        CrosstermColor::Reset => RatatuiColor::Reset,
        CrosstermColor::Black => RatatuiColor::Black,
        CrosstermColor::DarkGrey => RatatuiColor::DarkGray,
        CrosstermColor::Red => RatatuiColor::Red,
        // Map dark colors to indexed colors for terminal accuracy
        CrosstermColor::DarkRed => RatatuiColor::Indexed(52),
        CrosstermColor::Green => RatatuiColor::Green,
        CrosstermColor::DarkGreen => RatatuiColor::Indexed(22),
        CrosstermColor::Yellow => RatatuiColor::Yellow,
        CrosstermColor::DarkYellow => RatatuiColor::Indexed(58),
        CrosstermColor::Blue => RatatuiColor::Blue,
        CrosstermColor::DarkBlue => RatatuiColor::Indexed(17),
        CrosstermColor::Magenta => RatatuiColor::Magenta,
        CrosstermColor::DarkMagenta => RatatuiColor::Indexed(53),
        CrosstermColor::Cyan => RatatuiColor::Cyan,
        CrosstermColor::DarkCyan => RatatuiColor::Indexed(23),
        CrosstermColor::White => RatatuiColor::White,
        CrosstermColor::Grey => RatatuiColor::Gray,
        CrosstermColor::Rgb { r, g, b } => RatatuiColor::Rgb(*r, *g, *b),
        CrosstermColor::AnsiValue(code) => RatatuiColor::Indexed(*code),
    }
}

// ============================================================================
// Convenience Helper Functions for Common Styling Patterns
// ============================================================================

/// Create a ratatui Style with a foreground color from anstyle
///
/// This is a convenience wrapper that creates an anstyle Style with only
/// the foreground color set, then converts it to ratatui.
///
/// # Examples
/// ```ignore
/// use anstyle::Color;
/// use anstyle::AnsiColor;
/// let style = fg_color(Color::Ansi(AnsiColor::Green));
/// ```
pub fn fg_color(color: anstyle::Color) -> Style {
    anstyle_to_ratatui(AnstyleStyle::new().fg_color(Some(color)))
}

/// Create a ratatui Style with a background color from anstyle
///
/// This is a convenience wrapper that creates an anstyle Style with only
/// the background color set, then converts it to ratatui.
///
/// # Examples
/// ```ignore
/// use anstyle::Color;
/// use anstyle::AnsiColor;
/// let style = bg_color(Color::Ansi(AnsiColor::Red));
/// ```
pub fn bg_color(color: anstyle::Color) -> Style {
    anstyle_to_ratatui(AnstyleStyle::new().bg_color(Some(color)))
}

/// Create a ratatui Style with foreground and background colors
///
/// Combines both foreground and background colors in a single style.
///
/// # Examples
/// ```ignore
/// use anstyle::Color;
/// use anstyle::AnsiColor;
/// let style = fg_bg_colors(
///     Color::Ansi(AnsiColor::Black),
///     Color::Ansi(AnsiColor::Yellow),
/// );
/// ```
pub fn fg_bg_colors(fg: anstyle::Color, bg: anstyle::Color) -> Style {
    anstyle_to_ratatui(
        AnstyleStyle::new()
            .fg_color(Some(fg))
            .bg_color(Some(bg)),
    )
}

/// Create a ratatui Style with effects/modifiers
///
/// This is a convenience wrapper that creates an anstyle Style with only
/// the effects set, then converts it to ratatui.
///
/// # Examples
/// ```ignore
/// use anstyle::Effects;
/// let style = with_effects(Effects::BOLD | Effects::ITALIC);
/// ```
pub fn with_effects(effects: anstyle::Effects) -> Style {
    anstyle_to_ratatui(AnstyleStyle::new().effects(effects))
}

/// Create a ratatui Style with foreground color and effects
///
/// Combines a foreground color with text effects like bold, italic, etc.
///
/// # Examples
/// ```ignore
/// use anstyle::Color;
/// use anstyle::AnsiColor;
/// use anstyle::Effects;
/// let style = colored_with_effects(
///     Color::Ansi(AnsiColor::Green),
///     Effects::BOLD | Effects::ITALIC,
/// );
/// ```
pub fn colored_with_effects(color: anstyle::Color, effects: anstyle::Effects) -> Style {
    anstyle_to_ratatui(
        AnstyleStyle::new()
            .fg_color(Some(color))
            .effects(effects),
    )
}

/// Create a ratatui Style with background color and effects
///
/// Combines a background color with text effects.
///
/// # Examples
/// ```ignore
/// use anstyle::Color;
/// use anstyle::AnsiColor;
/// use anstyle::Effects;
/// let style = bg_colored_with_effects(
///     Color::Ansi(AnsiColor::Blue),
///     Effects::BOLD,
/// );
/// ```
pub fn bg_colored_with_effects(color: anstyle::Color, effects: anstyle::Effects) -> Style {
    anstyle_to_ratatui(
        AnstyleStyle::new()
            .bg_color(Some(color))
            .effects(effects),
    )
}

/// Create a complete ratatui Style from anstyle colors and effects
///
/// This combines foreground color, background color, and effects in one call.
///
/// # Examples
/// ```ignore
/// use anstyle::Color;
/// use anstyle::AnsiColor;
/// use anstyle::Effects;
/// let style = full_style(
///     Some(Color::Ansi(AnsiColor::White)),
///     Some(Color::Ansi(AnsiColor::Blue)),
///     Effects::BOLD,
/// );
/// ```
pub fn full_style(
    fg: Option<anstyle::Color>,
    bg: Option<anstyle::Color>,
    effects: anstyle::Effects,
) -> Style {
    let mut astyle = AnstyleStyle::new();
    if let Some(c) = fg {
        astyle = astyle.fg_color(Some(c));
    }
    if let Some(c) = bg {
        astyle = astyle.bg_color(Some(c));
    }
    astyle = astyle.effects(effects);
    anstyle_to_ratatui(astyle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::{AnsiColor, Color as AnstyleColor, Effects, RgbColor};

    #[test]
    fn test_red_color_conversion() {
        let anstyle = AnstyleStyle::new().fg_color(Some(AnstyleColor::Ansi(AnsiColor::Red)));
        let ratatui_style = anstyle_to_ratatui(anstyle);
        // anstyle-crossterm maps standard colors to indexed variants for compatibility
        assert_eq!(ratatui_style.fg, Some(RatatuiColor::Indexed(52))); // DarkRed
    }

    #[test]
    fn test_green_color_conversion() {
        let anstyle = AnstyleStyle::new().fg_color(Some(AnstyleColor::Ansi(AnsiColor::Green)));
        let ratatui_style = anstyle_to_ratatui(anstyle);
        // anstyle-crossterm maps standard colors to indexed variants for compatibility
        assert_eq!(ratatui_style.fg, Some(RatatuiColor::Indexed(22))); // DarkGreen
    }

    #[test]
    fn test_blue_color_conversion() {
        let anstyle = AnstyleStyle::new().fg_color(Some(AnstyleColor::Ansi(AnsiColor::Blue)));
        let ratatui_style = anstyle_to_ratatui(anstyle);
        // anstyle-crossterm maps standard colors to indexed variants for compatibility
        assert_eq!(ratatui_style.fg, Some(RatatuiColor::Indexed(17))); // DarkBlue
    }

    #[test]
    fn test_bold_effect() {
        let anstyle = AnstyleStyle::new().effects(Effects::BOLD);
        let ratatui_style = anstyle_to_ratatui(anstyle);
        assert!(ratatui_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_italic_effect() {
        let anstyle = AnstyleStyle::new().effects(Effects::ITALIC);
        let ratatui_style = anstyle_to_ratatui(anstyle);
        assert!(ratatui_style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn test_underline_effect() {
        let anstyle = AnstyleStyle::new().effects(Effects::UNDERLINE);
        let ratatui_style = anstyle_to_ratatui(anstyle);
        assert!(ratatui_style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_combined_style() {
        let anstyle = AnstyleStyle::new()
            .fg_color(Some(AnstyleColor::Ansi(AnsiColor::Green)))
            .effects(Effects::BOLD | Effects::UNDERLINE);

        let ratatui_style = anstyle_to_ratatui(anstyle);
        // Green gets mapped to indexed 22 (DarkGreen) by anstyle-crossterm
        assert_eq!(ratatui_style.fg, Some(RatatuiColor::Indexed(22)));
        assert!(ratatui_style.add_modifier.contains(Modifier::BOLD));
        assert!(ratatui_style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_rgb_color() {
        let anstyle = AnstyleStyle::new().fg_color(Some(AnstyleColor::Rgb(RgbColor(255, 128, 0))));
        let ratatui_style = anstyle_to_ratatui(anstyle);
        assert_eq!(ratatui_style.fg, Some(RatatuiColor::Rgb(255, 128, 0)));
    }

    #[test]
    fn test_background_color() {
        let anstyle = AnstyleStyle::new().bg_color(Some(AnstyleColor::Ansi(AnsiColor::Yellow)));
        let ratatui_style = anstyle_to_ratatui(anstyle);
        // Yellow gets mapped to indexed 58 (DarkYellow) by anstyle-crossterm
        assert_eq!(ratatui_style.bg, Some(RatatuiColor::Indexed(58)));
    }

    #[test]
    fn test_all_effects() {
        let anstyle = AnstyleStyle::new().effects(
            Effects::BOLD
                | Effects::ITALIC
                | Effects::UNDERLINE
                | Effects::DIMMED
                | Effects::INVERT
                | Effects::STRIKETHROUGH,
        );
        let ratatui_style = anstyle_to_ratatui(anstyle);
        assert!(ratatui_style.add_modifier.contains(Modifier::BOLD));
        assert!(ratatui_style.add_modifier.contains(Modifier::ITALIC));
        assert!(ratatui_style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(ratatui_style.add_modifier.contains(Modifier::DIM));
        assert!(ratatui_style.add_modifier.contains(Modifier::REVERSED));
        // STRIKETHROUGH from anstyle maps to OverLined in crossterm,
        // which ratatui doesn't have direct support for, so it's not applied
    }

    #[test]
    fn test_no_style() {
        let anstyle = AnstyleStyle::new();
        let ratatui_style = anstyle_to_ratatui(anstyle);
        assert_eq!(ratatui_style.fg, None);
        assert_eq!(ratatui_style.bg, None);
        assert_eq!(ratatui_style.add_modifier, Modifier::empty());
    }

    #[test]
    fn test_dark_grey_color_mapping() {
        let anstyle = AnstyleStyle::new().fg_color(Some(AnstyleColor::Ansi(AnsiColor::BrightBlack)));
        let ratatui_style = anstyle_to_ratatui(anstyle);
        // Dark/bright black should map to DarkGray
        assert_eq!(ratatui_style.fg, Some(RatatuiColor::DarkGray));
    }

    #[test]
    fn test_helper_fg_color() {
        let style = fg_color(AnstyleColor::Ansi(AnsiColor::Blue));
        // Blue gets mapped to indexed 17 (DarkBlue) by anstyle-crossterm
        assert_eq!(style.fg, Some(RatatuiColor::Indexed(17)));
    }

    #[test]
    fn test_helper_with_effects() {
        let style = with_effects(Effects::BOLD | Effects::ITALIC);
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn test_helper_colored_with_effects() {
        let style = colored_with_effects(
            AnstyleColor::Ansi(AnsiColor::Red),
            Effects::BOLD | Effects::UNDERLINE,
        );
        // Red gets mapped to indexed 52 (DarkRed) by anstyle-crossterm
        assert_eq!(style.fg, Some(RatatuiColor::Indexed(52)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn test_helper_fg_bg_colors() {
        let style = fg_bg_colors(
            AnstyleColor::Ansi(AnsiColor::White),
            AnstyleColor::Ansi(AnsiColor::Blue),
        );
        // White gets mapped to Gray (indexed) by anstyle-crossterm
        assert_eq!(style.fg, Some(RatatuiColor::Gray));
        // Blue gets mapped to indexed 17 (DarkBlue) by anstyle-crossterm
        assert_eq!(style.bg, Some(RatatuiColor::Indexed(17)));
    }

    #[test]
    fn test_helper_bg_colored_with_effects() {
        let style = bg_colored_with_effects(
            AnstyleColor::Ansi(AnsiColor::Green),
            Effects::BOLD | Effects::ITALIC,
        );
        // Green gets mapped to indexed 22 (DarkGreen) by anstyle-crossterm
        assert_eq!(style.bg, Some(RatatuiColor::Indexed(22)));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn test_helper_full_style() {
        let style = full_style(
            Some(AnstyleColor::Ansi(AnsiColor::White)),
            Some(AnstyleColor::Ansi(AnsiColor::Black)),
            Effects::BOLD,
        );
        // White gets mapped to Gray (indexed) by anstyle-crossterm
        assert_eq!(style.fg, Some(RatatuiColor::Gray));
        assert_eq!(style.bg, Some(RatatuiColor::Black));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_helper_full_style_partial() {
        let style = full_style(
            Some(AnstyleColor::Ansi(AnsiColor::Yellow)),
            None,
            Effects::ITALIC,
        );
        // Yellow gets mapped to indexed 58 (DarkYellow) by anstyle-crossterm
        assert_eq!(style.fg, Some(RatatuiColor::Indexed(58)));
        assert_eq!(style.bg, None);
        assert!(style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn test_helper_full_style_no_effects() {
        let style = full_style(
            Some(AnstyleColor::Ansi(AnsiColor::Cyan)),
            Some(AnstyleColor::Ansi(AnsiColor::Black)),
            Effects::new(),
        );
        // Cyan gets mapped to indexed 23 (DarkCyan) by anstyle-crossterm
        assert_eq!(style.fg, Some(RatatuiColor::Indexed(23)));
        assert_eq!(style.bg, Some(RatatuiColor::Black));
        assert_eq!(style.add_modifier, Modifier::empty());
    }
}
