//! Advanced color utilities for anstyle
//!
//! This module provides utilities for working with anstyle colors including
//! blending, conversion, and manipulation.

use anstyle::{Color, Effects, RgbColor, Style as AnsiStyle};

/// Create an RGB color from hex string
pub fn color_from_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

    Some(Color::Rgb(RgbColor(r, g, b)))
}

/// Blend two RGB colors
pub fn blend_colors(color1: &Color, color2: &Color, ratio: f32) -> Option<Color> {
    let rgb1 = color_to_rgb(color1)?;
    let rgb2 = color_to_rgb(color2)?;
    
    let r = (rgb1.r() as f32 * (1.0 - ratio) + rgb2.r() as f32 * ratio) as u8;
    let g = (rgb1.g() as f32 * (1.0 - ratio) + rgb2.g() as f32 * ratio) as u8;
    let b = (rgb1.b() as f32 * (1.0 - ratio) + rgb2.b() as f32 * ratio) as u8;

    Some(Color::Rgb(RgbColor(r, g, b)))
}

/// Convert an ANSI color to RGB, if possible
fn color_to_rgb(color: &Color) -> Option<RgbColor> {
    match color {
        Color::Rgb(rgb) => Some(*rgb),
        Color::Ansi(ansi_color) => ansi_to_rgb(*ansi_color),
        Color::Ansi256(ansi256_color) => ansi256_to_rgb(*ansi256_color), 
    }
}

/// Convert an ANSI color to RGB approximation
fn ansi_to_rgb(ansi_color: anstyle::AnsiColor) -> Option<RgbColor> {
    match ansi_color {
        anstyle::AnsiColor::Black => Some(RgbColor(0, 0, 0)),
        anstyle::AnsiColor::Red => Some(RgbColor(170, 0, 0)),
        anstyle::AnsiColor::Green => Some(RgbColor(0, 170, 0)),
        anstyle::AnsiColor::Yellow => Some(RgbColor(170, 85, 0)),
        anstyle::AnsiColor::Blue => Some(RgbColor(0, 0, 170)),
        anstyle::AnsiColor::Magenta => Some(RgbColor(170, 0, 170)),
        anstyle::AnsiColor::Cyan => Some(RgbColor(0, 170, 170)),
        anstyle::AnsiColor::White => Some(RgbColor(170, 170, 170)),
        anstyle::AnsiColor::BrightBlack => Some(RgbColor(85, 85, 85)),
        anstyle::AnsiColor::BrightRed => Some(RgbColor(255, 85, 85)),
        anstyle::AnsiColor::BrightGreen => Some(RgbColor(85, 255, 85)),
        anstyle::AnsiColor::BrightYellow => Some(RgbColor(255, 255, 85)),
        anstyle::AnsiColor::BrightBlue => Some(RgbColor(85, 85, 255)),
        anstyle::AnsiColor::BrightMagenta => Some(RgbColor(255, 85, 255)),
        anstyle::AnsiColor::BrightCyan => Some(RgbColor(85, 255, 255)),
        anstyle::AnsiColor::BrightWhite => Some(RgbColor(255, 255, 255)),
    }
}

/// Convert an ANSI256 color to RGB approximation
fn ansi256_to_rgb(ansi256_color: anstyle::Ansi256Color) -> Option<RgbColor> {
    // Standard ANSI256 color conversion - return the same code as RGB
    // For full conversion, we'd need a proper lookup table, but for
    // simplicity, we'll return the first 16 colors as approximations
    let code = ansi256_color.0;
    match code {
        0 => Some(RgbColor(0, 0, 0)),
        1 => Some(RgbColor(170, 0, 0)),
        2 => Some(RgbColor(0, 170, 0)),
        3 => Some(RgbColor(170, 85, 0)),
        4 => Some(RgbColor(0, 0, 170)),
        5 => Some(RgbColor(170, 0, 170)),
        6 => Some(RgbColor(0, 170, 170)),
        7 => Some(RgbColor(170, 170, 170)),
        8 => Some(RgbColor(85, 85, 85)),
        9 => Some(RgbColor(255, 85, 85)),
        10 => Some(RgbColor(85, 255, 85)),
        11 => Some(RgbColor(255, 255, 85)),
        12 => Some(RgbColor(85, 85, 255)),
        13 => Some(RgbColor(255, 85, 255)),
        14 => Some(RgbColor(85, 255, 255)),
        15 => Some(RgbColor(255, 255, 255)),
        _ => {
            // For other ANSI256 codes, we'd typically use a lookup table
            // For now, just return the RGB equivalent if it exists in the 0-255 range
            // This is a simplified conversion
            Some(RgbColor(code, code, code)) // Just as a default fallback
        }
    }
}

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
    enhanced_style(
        fg,
        None,
        Effects::BOLD | Effects::UNDERLINE,
    )
}

/// Create a dim italic style for secondary text
pub fn dim_italic(fg: Option<Color>) -> AnsiStyle {
    enhanced_style(
        fg,
        None,
        Effects::DIMMED | Effects::ITALIC,
    )
}

/// Create a style with inverted colors
pub fn inverted(fg: Option<Color>, bg: Option<Color>) -> AnsiStyle {
    enhanced_style(
        bg, // Swap fg and bg
        fg,
        Effects::INVERT,
    )
}

/// Determine if a color is light (for contrast calculations)
pub fn is_light_color(color: &Color) -> bool {
    let rgb = color_to_rgb(color);
    if let Some(RgbColor(r, g, b)) = rgb {
        // Perceived luminance calculation
        let luminance = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0;
        luminance > 0.5
    } else {
        false // Default to dark for non-RGB colors
    }
}

/// Get a contrasting color (black or white) for better readability
pub fn contrasting_color(color: &Color) -> Color {
    if is_light_color(color) {
        Color::Ansi(anstyle::AnsiColor::Black)
    } else {
        Color::Ansi(anstyle::AnsiColor::White)
    }
}

/// Create a desaturated version of a color
pub fn desaturate_color(color: &Color, amount: f32) -> Option<Color> {
    let rgb = color_to_rgb(color)?;
    let r = rgb.r() as f32;
    let g = rgb.g() as f32;
    let b = rgb.b() as f32;
    
    // Calculate grayscale using luminance
    let gray = 0.299 * r + 0.587 * g + 0.114 * b;
    
    // Blend original with grayscale based on amount (0.0 = original, 1.0 = grayscale)
    let r_new = r * (1.0 - amount) + gray * amount;
    let g_new = g * (1.0 - amount) + gray * amount;
    let b_new = b * (1.0 - amount) + gray * amount;
    
    Some(Color::Rgb(RgbColor(
        r_new as u8,
        g_new as u8,
        b_new as u8,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anstyle::{AnsiColor, RgbColor};

    #[test]
    fn test_hex_to_color() {
        let color = color_from_hex("#FF0000").unwrap();
        let rgb = color_to_rgb(&color).unwrap();
        assert_eq!(rgb, RgbColor(255, 0, 0));
    }

    #[test]
    fn test_hex_without_pound() {
        let color = color_from_hex("00FF00").unwrap();
        let rgb = color_to_rgb(&color).unwrap();
        assert_eq!(rgb, RgbColor(0, 255, 0));
    }

    #[test]
    fn test_invalid_hex() {
        assert!(color_from_hex("invalid").is_none());
        assert!(color_from_hex("#FF00").is_none()); // Too short
        assert!(color_from_hex("#FF00000").is_none()); // Too long
    }

    #[test]
    fn test_blend_colors() {
        let red = Color::Rgb(RgbColor(255, 0, 0));
        let blue = Color::Rgb(RgbColor(0, 0, 255));
        let blended = blend_colors(&red, &blue, 0.5).unwrap();
        let rgb = color_to_rgb(&blended).unwrap();
        
        // Should be roughly purple (127, 0, 127) after blending
        assert!((rgb.r() as i32 - 127).abs() < 2);
        assert!((rgb.g() as i32 - 0).abs() < 2);
        assert!((rgb.b() as i32 - 127).abs() < 2);
    }

    #[test]
    fn test_ansi_to_rgb() {
        let red = Color::Ansi(anstyle::AnsiColor::Red);
        let rgb = color_to_rgb(&red).unwrap();
        assert_eq!(rgb, RgbColor(170, 0, 0));
    }

    #[test]
    fn test_is_light_color() {
        let light = Color::Rgb(RgbColor(255, 255, 255));
        let dark = Color::Rgb(RgbColor(0, 0, 0));
        
        assert!(is_light_color(&light));
        assert!(!is_light_color(&dark));
    }

    #[test]
    fn test_contrasting_color() {
        let light = Color::Rgb(RgbColor(255, 255, 255));
        let dark = Color::Rgb(RgbColor(0, 0, 0));
        
        // For light color, contrasting should be dark
        assert_eq!(contrasting_color(&light), Color::Ansi(anstyle::AnsiColor::Black));
        // For dark color, contrasting should be light  
        assert_eq!(contrasting_color(&dark), Color::Ansi(anstyle::AnsiColor::White));
    }

    #[test]
    fn test_desaturate_color() {
        let red = Color::Rgb(RgbColor(255, 0, 0));
        let desaturated = desaturate_color(&red, 1.0).unwrap(); // Fully desaturated
        let rgb = color_to_rgb(&desaturated).unwrap();
        
        // Should be some shade of gray
        assert_eq!(rgb.r(), rgb.g());
        assert_eq!(rgb.g(), rgb.b());
    }

    #[test]
    fn test_enhanced_style() {
        let style = enhanced_style(
            Some(Color::Ansi(anstyle::AnsiColor::Green)),
            Some(Color::Ansi(anstyle::AnsiColor::Blue)),
            Effects::BOLD | Effects::ITALIC
        );
        
        assert!(style.get_fg_color().is_some());
        assert!(style.get_bg_color().is_some());
        assert!(style.get_effects().contains(Effects::BOLD));
        assert!(style.get_effects().contains(Effects::ITALIC));
    }

    #[test]
    fn test_bold_underline() {
        let style = bold_underline(Some(Color::Ansi(anstyle::AnsiColor::Red)));
        assert!(style.get_effects().contains(Effects::BOLD));
        assert!(style.get_effects().contains(Effects::UNDERLINE));
    }

    #[test]
    fn test_dim_italic() {
        let style = dim_italic(Some(Color::Ansi(anstyle::AnsiColor::Blue)));
        assert!(style.get_effects().contains(Effects::DIMMED));
        assert!(style.get_effects().contains(Effects::ITALIC));
    }

    #[test]
    fn test_inverted() {
        let fg = Color::Rgb(RgbColor(255, 0, 0)); // Red
        let bg = Color::Rgb(RgbColor(0, 0, 255)); // Blue
        let inverted_style = inverted(Some(fg), Some(bg));
        
        // Inverted should swap fg and bg
        let inverted_fg = inverted_style.get_fg_color().unwrap();
        let inverted_bg = inverted_style.get_bg_color().unwrap();
        
        let fg_rgb = color_to_rgb(&inverted_fg).unwrap();
        let bg_rgb = color_to_rgb(&inverted_bg).unwrap();
        
        assert_eq!(fg_rgb, RgbColor(0, 0, 255)); // Original bg
        assert_eq!(bg_rgb, RgbColor(255, 0, 0)); // Original fg
    }
}