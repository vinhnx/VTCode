//! ANSI terminal capabilities detection and feature support
//!
//! This module provides utilities to detect terminal capabilities such as color depth,
//! unicode support, advanced formatting features, and color scheme (light/dark mode).
//!
//! Standards implemented:
//! - NO_COLOR: https://no-color.org/
//! - CLICOLOR/CLICOLOR_FORCE: De-facto standard
//! - COLORTERM: True-color detection
//! - COLORFGBG: Terminal foreground/background detection

use anstyle_query::{clicolor, clicolor_force, no_color, term_supports_color};
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU8, Ordering};

/// Color depth support level detected for the terminal
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorDepth {
    /// No color support
    None = 0,
    /// 16 colors (basic ANSI)
    Basic16 = 1,
    /// 256 colors
    Color256 = 2,
    /// True color (24-bit RGB)
    TrueColor = 3,
}

impl ColorDepth {
    /// Get a human-readable name for this color depth
    pub fn name(self) -> &'static str {
        match self {
            ColorDepth::None => "none",
            ColorDepth::Basic16 => "16-color",
            ColorDepth::Color256 => "256-color",
            ColorDepth::TrueColor => "true-color",
        }
    }

    /// Check if this depth supports color
    pub fn supports_color(self) -> bool {
        self != ColorDepth::None
    }

    /// Check if this depth is at least 256 colors
    pub fn supports_256(self) -> bool {
        self >= ColorDepth::Color256
    }

    /// Check if this depth supports true color
    pub fn supports_true_color(self) -> bool {
        self == ColorDepth::TrueColor
    }
}

/// ANSI terminal feature capabilities
#[derive(Clone, Copy, Debug)]
pub struct AnsiCapabilities {
    /// Detected color depth
    pub color_depth: ColorDepth,
    /// Whether unicode is supported
    pub unicode_support: bool,
    /// Whether to force color output
    pub force_color: bool,
    /// Whether color is explicitly disabled
    pub no_color: bool,
}

impl AnsiCapabilities {
    /// Detect terminal capabilities
    pub fn detect() -> Self {
        Self {
            color_depth: detect_color_depth(),
            unicode_support: detect_unicode_support(),
            force_color: clicolor_force(),
            no_color: no_color(),
        }
    }

    /// Check if color output is supported
    pub fn supports_color(&self) -> bool {
        !self.no_color && (self.force_color || self.color_depth.supports_color())
    }

    /// Check if 256-color output is supported
    pub fn supports_256_colors(&self) -> bool {
        self.supports_color() && self.color_depth.supports_256()
    }

    /// Check if true color (24-bit) is supported
    pub fn supports_true_color(&self) -> bool {
        self.supports_color() && self.color_depth.supports_true_color()
    }

    /// Check if advanced formatting (tables, boxes) should use unicode
    pub fn should_use_unicode_boxes(&self) -> bool {
        self.unicode_support && self.supports_color()
    }
}

/// Detected terminal color scheme (light or dark background)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// Light background (dark text preferred)
    Light,
    /// Dark background (light text preferred)
    #[default]
    Dark,
    /// Unable to detect, assume dark
    Unknown,
}

impl ColorScheme {
    /// Check if this is a light color scheme
    pub fn is_light(self) -> bool {
        matches!(self, ColorScheme::Light)
    }

    /// Check if this is a dark color scheme
    pub fn is_dark(self) -> bool {
        matches!(self, ColorScheme::Dark | ColorScheme::Unknown)
    }

    /// Get a human-readable name
    pub fn name(self) -> &'static str {
        match self {
            ColorScheme::Light => "light",
            ColorScheme::Dark => "dark",
            ColorScheme::Unknown => "unknown",
        }
    }
}

/// Detect terminal color scheme from environment.
///
/// Detection methods (in order of priority):
/// 1. COLORFGBG environment variable (format: "fg;bg" or "fg;ignored;bg")
/// 2. Terminal-specific environment variables
/// 3. Default to dark (most common terminal configuration)
///
/// Based on: https://no-color.org/ and terminal color research
pub fn detect_color_scheme() -> ColorScheme {
    // Check cached value first
    static CACHED: Lazy<ColorScheme> = Lazy::new(detect_color_scheme_uncached);
    *CACHED
}

fn detect_color_scheme_uncached() -> ColorScheme {
    // Method 1: COLORFGBG (used by rxvt, xterm, and many others)
    // Format: "foreground;background" or "foreground;ignored;background"
    if let Ok(colorfgbg) = std::env::var("COLORFGBG") {
        let parts: Vec<&str> = colorfgbg.split(';').collect();
        // Background color is the last component
        if let Some(bg_str) = parts.last() {
            if let Ok(bg) = bg_str.parse::<u8>() {
                // ANSI colors 0-7: 0=black, 7=white (light), 15=bright white
                // If background is white or near-white, it's a light theme
                // Colors 7 (white), 15 (bright white) indicate light background
                return if bg == 7 || bg == 15 {
                    ColorScheme::Light
                } else if bg == 0 || bg == 8 {
                    // 0=black, 8=bright black (gray)
                    ColorScheme::Dark
                } else if bg > 230 {
                    // 256-color: 230+ are light grays/white
                    ColorScheme::Light
                } else {
                    ColorScheme::Dark
                };
            }
        }
    }

    // Method 2: Check for known light terminal indicators
    // TERM_PROGRAM can hint at default color scheme
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        let term_lower = term_program.to_lowercase();
        // These typically default to dark
        if term_lower.contains("iterm")
            || term_lower.contains("ghostty")
            || term_lower.contains("warp")
            || term_lower.contains("alacritty")
        {
            // Modern terminals typically default to dark, but don't override COLORFGBG
            return ColorScheme::Dark;
        }
    }

    // Method 3: macOS Terminal.app often uses light background by default
    if cfg!(target_os = "macos") {
        if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
            if term_program == "Apple_Terminal" {
                // Apple Terminal defaults to light, but user may have changed it
                // Without more info, assume light for Apple_Terminal
                return ColorScheme::Light;
            }
        }
    }

    // Default to dark (most common for development terminals)
    ColorScheme::Unknown
}

// Cache detection results to avoid repeated system calls
static COLOR_DEPTH_CACHE: AtomicU8 = AtomicU8::new(255); // 255 = not cached yet

/// Detect the terminal's color depth
fn detect_color_depth() -> ColorDepth {
    // Check cache first
    let cached = COLOR_DEPTH_CACHE.load(Ordering::Relaxed);
    if cached != 255 {
        return match cached {
            0 => ColorDepth::None,
            1 => ColorDepth::Basic16,
            2 => ColorDepth::Color256,
            3 => ColorDepth::TrueColor,
            _ => ColorDepth::None,
        };
    }

    let depth = if no_color() {
        ColorDepth::None
    } else if clicolor_force() {
        ColorDepth::TrueColor
    } else if !clicolor().unwrap_or_else(term_supports_color) {
        ColorDepth::None
    } else {
        // Check COLORTERM for truecolor support
        std::env::var("COLORTERM")
            .ok()
            .and_then(|val| {
                let lower = val.to_lowercase();
                if lower.contains("truecolor") || lower.contains("24bit") {
                    Some(ColorDepth::TrueColor)
                } else {
                    None
                }
            })
            .unwrap_or(ColorDepth::Color256)
    };

    // Cache the result
    COLOR_DEPTH_CACHE.store(
        match depth {
            ColorDepth::None => 0,
            ColorDepth::Basic16 => 1,
            ColorDepth::Color256 => 2,
            ColorDepth::TrueColor => 3,
        },
        Ordering::Relaxed,
    );

    depth
}

/// Detect if unicode is supported by the terminal
fn detect_unicode_support() -> bool {
    std::env::var("LANG")
        .ok()
        .map(|lang| lang.to_lowercase().contains("utf"))
        .or_else(|| {
            std::env::var("LC_ALL")
                .ok()
                .map(|lc| lc.to_lowercase().contains("utf"))
        })
        .unwrap_or(true) // Default to true for modern systems
}

/// Global capabilities instance (cached)
pub static CAPABILITIES: Lazy<AnsiCapabilities> = Lazy::new(AnsiCapabilities::detect);

/// Check if NO_COLOR environment variable is set (re-exported for convenience)
pub fn is_no_color() -> bool {
    no_color()
}

/// Check if CLICOLOR_FORCE is set (re-exported for convenience)
pub fn is_clicolor_force() -> bool {
    clicolor_force()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_depth_ordering() {
        assert!(ColorDepth::None < ColorDepth::Basic16);
        assert!(ColorDepth::Basic16 < ColorDepth::Color256);
        assert!(ColorDepth::Color256 < ColorDepth::TrueColor);
    }

    #[test]
    fn test_color_depth_names() {
        assert_eq!(ColorDepth::None.name(), "none");
        assert_eq!(ColorDepth::Basic16.name(), "16-color");
        assert_eq!(ColorDepth::Color256.name(), "256-color");
        assert_eq!(ColorDepth::TrueColor.name(), "true-color");
    }

    #[test]
    fn test_capabilities_detect() {
        let caps = AnsiCapabilities::detect();
        assert!(!caps.no_color || !caps.supports_color());
    }

    #[test]
    fn test_color_depth_methods() {
        assert!(!ColorDepth::None.supports_color());
        assert!(ColorDepth::Color256.supports_256());
        assert!(!ColorDepth::Color256.supports_true_color());
        assert!(ColorDepth::TrueColor.supports_true_color());
    }

    #[test]
    fn test_color_scheme_methods() {
        assert!(ColorScheme::Light.is_light());
        assert!(!ColorScheme::Light.is_dark());
        assert!(ColorScheme::Dark.is_dark());
        assert!(!ColorScheme::Dark.is_light());
        assert!(ColorScheme::Unknown.is_dark()); // Unknown defaults to dark
        assert_eq!(ColorScheme::Light.name(), "light");
        assert_eq!(ColorScheme::Dark.name(), "dark");
    }
}
