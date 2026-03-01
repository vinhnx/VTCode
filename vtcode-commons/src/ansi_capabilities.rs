//! ANSI terminal capabilities detection and feature support

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

const COLOR_SCHEME_LIGHT: u8 = 0;
const COLOR_SCHEME_DARK: u8 = 1;
const COLOR_SCHEME_UNKNOWN: u8 = 2;
const COLOR_SCHEME_UNSET: u8 = 255;

static COLOR_SCHEME_RUNTIME_OVERRIDE: AtomicU8 = AtomicU8::new(COLOR_SCHEME_UNSET);

/// Detect terminal color scheme from environment.
pub fn detect_color_scheme() -> ColorScheme {
    if let Some(override_scheme) = color_scheme_runtime_override() {
        return override_scheme;
    }

    // Check cached value first
    static CACHED: Lazy<ColorScheme> = Lazy::new(detect_color_scheme_uncached);
    *CACHED
}

fn color_scheme_runtime_override() -> Option<ColorScheme> {
    match COLOR_SCHEME_RUNTIME_OVERRIDE.load(Ordering::Relaxed) {
        COLOR_SCHEME_LIGHT => Some(ColorScheme::Light),
        COLOR_SCHEME_DARK => Some(ColorScheme::Dark),
        COLOR_SCHEME_UNKNOWN => Some(ColorScheme::Unknown),
        _ => None,
    }
}

/// Store a runtime color scheme override.
///
/// This is intended to be populated once at startup by terminal OSC probing.
/// Set `None` to clear the runtime override.
pub fn set_color_scheme_override(value: Option<ColorScheme>) {
    let encoded = match value {
        Some(ColorScheme::Light) => COLOR_SCHEME_LIGHT,
        Some(ColorScheme::Dark) => COLOR_SCHEME_DARK,
        Some(ColorScheme::Unknown) => COLOR_SCHEME_UNKNOWN,
        None => COLOR_SCHEME_UNSET,
    };
    COLOR_SCHEME_RUNTIME_OVERRIDE.store(encoded, Ordering::Relaxed);
}

fn detect_color_scheme_uncached() -> ColorScheme {
    if let Ok(colorfgbg) = std::env::var("COLORFGBG") {
        let parts: Vec<&str> = colorfgbg.split(';').collect();
        if let Some(bg_str) = parts.last()
            && let Ok(bg) = bg_str.parse::<u8>()
        {
            return if bg == 7 || bg == 15 {
                ColorScheme::Light
            } else if bg == 0 || bg == 8 {
                ColorScheme::Dark
            } else if bg > 230 {
                ColorScheme::Light
            } else {
                ColorScheme::Dark
            };
        }
    }

    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        let term_lower = term_program.to_lowercase();
        if term_lower.contains("iterm")
            || term_lower.contains("ghostty")
            || term_lower.contains("warp")
            || term_lower.contains("alacritty")
        {
            return ColorScheme::Dark;
        }
    }

    if cfg!(target_os = "macos")
        && let Ok(term_program) = std::env::var("TERM_PROGRAM")
        && term_program == "Apple_Terminal"
    {
        return ColorScheme::Light;
    }

    ColorScheme::Unknown
}

// Cache detection results to avoid repeated system calls
static COLOR_DEPTH_CACHE: AtomicU8 = AtomicU8::new(255); // 255 = not cached yet

/// Detect the terminal's color depth
fn detect_color_depth() -> ColorDepth {
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
        .unwrap_or(true)
}

/// Global capabilities instance (cached)
pub static CAPABILITIES: Lazy<AnsiCapabilities> = Lazy::new(AnsiCapabilities::detect);

/// Check if NO_COLOR environment variable is set
pub fn is_no_color() -> bool {
    no_color()
}

/// Check if CLICOLOR_FORCE is set
pub fn is_clicolor_force() -> bool {
    clicolor_force()
}
