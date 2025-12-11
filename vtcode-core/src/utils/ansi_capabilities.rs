//! ANSI terminal capabilities detection and feature support
//!
//! This module provides utilities to detect terminal capabilities such as color depth,
//! unicode support, and advanced formatting features.

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
}
