use anstyle::{Color, Effects, RgbColor, Style};
use anyhow::{Context, Result, anyhow};
use catppuccin::PALETTE;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;

use crate::config::constants::{defaults, ui};

/// Identifier for the default theme.
pub const DEFAULT_THEME_ID: &str = defaults::DEFAULT_THEME;

/// Default minimum contrast ratio (WCAG AA standard)
const DEFAULT_MIN_CONTRAST: f64 = ui::THEME_MIN_CONTRAST_RATIO;
const MAX_DARK_BG_TEXT_LUMINANCE: f64 = 0.92;
const MIN_DARK_BG_TEXT_LUMINANCE: f64 = 0.20;
const MAX_LIGHT_BG_TEXT_LUMINANCE: f64 = 0.68;

/// Runtime configuration for color accessibility settings.
/// These can be updated from vtcode.toml [ui] section.
static COLOR_CONFIG: Lazy<RwLock<ColorAccessibilityConfig>> =
    Lazy::new(|| RwLock::new(ColorAccessibilityConfig::default()));

/// Color accessibility configuration loaded from vtcode.toml
#[derive(Clone, Debug)]
pub struct ColorAccessibilityConfig {
    /// Minimum contrast ratio for text (WCAG standard)
    pub minimum_contrast: f64,
    /// Whether to treat bold as bright (legacy terminal compat)
    pub bold_is_bright: bool,
    /// Whether to restrict to safe ANSI colors only
    pub safe_colors_only: bool,
}

impl Default for ColorAccessibilityConfig {
    fn default() -> Self {
        Self {
            minimum_contrast: DEFAULT_MIN_CONTRAST,
            bold_is_bright: false,
            safe_colors_only: false,
        }
    }
}

/// Update the global color accessibility configuration.
/// Call this after loading vtcode.toml to apply user preferences.
pub fn set_color_accessibility_config(config: ColorAccessibilityConfig) {
    *COLOR_CONFIG.write() = config;
}

/// Get the current minimum contrast ratio setting.
pub fn get_minimum_contrast() -> f64 {
    COLOR_CONFIG.read().minimum_contrast
}

/// Check if bold-is-bright compatibility mode is enabled.
pub fn is_bold_bright_mode() -> bool {
    COLOR_CONFIG.read().bold_is_bright
}

/// Check if safe colors only mode is enabled.
pub fn is_safe_colors_only() -> bool {
    COLOR_CONFIG.read().safe_colors_only
}

/// Palette describing UI colors for the terminal experience.
#[derive(Clone, Debug)]
pub struct ThemePalette {
    pub primary_accent: RgbColor,
    pub background: RgbColor,
    pub foreground: RgbColor,
    pub secondary_accent: RgbColor,
    pub alert: RgbColor,
    pub logo_accent: RgbColor,
}

impl ThemePalette {
    /// Create a style with foreground color, respecting bold_is_bright setting.
    /// When bold_is_bright is enabled and bold is requested, we skip bold
    /// to prevent unintended bright color mapping in legacy terminals.
    fn style_from(color: RgbColor, bold: bool) -> Style {
        let mut style = Style::new().fg_color(Some(Color::Rgb(color)));
        // Only apply bold if not in bold_is_bright compatibility mode
        if bold && !is_bold_bright_mode() {
            style = style.bold();
        }
        style
    }

    fn build_styles(&self) -> ThemeStyles {
        self.build_styles_with_contrast(get_minimum_contrast())
    }

    /// Build styles with a specific minimum contrast ratio.
    /// This allows runtime configuration of contrast requirements.
    fn build_styles_with_contrast(&self, min_contrast: f64) -> ThemeStyles {
        let primary = self.primary_accent;
        let background = self.background;
        let secondary = self.secondary_accent;
        let logo_accent = self.logo_accent;

        let fallback_light = RgbColor(
            ui::THEME_COLOR_WHITE_RED,
            ui::THEME_COLOR_WHITE_GREEN,
            ui::THEME_COLOR_WHITE_BLUE,
        );

        let text_color = ensure_contrast(
            self.foreground,
            background,
            min_contrast,
            &[
                lighten(self.foreground, ui::THEME_FOREGROUND_LIGHTEN_RATIO),
                lighten(secondary, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                fallback_light,
            ],
        );
        let text_color = balance_text_luminance(text_color, background, min_contrast);

        let info_color = ensure_contrast(
            secondary,
            background,
            min_contrast,
            &[
                lighten(secondary, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                text_color,
                fallback_light,
            ],
        );
        let info_color = balance_text_luminance(info_color, background, min_contrast);

        // Light gray for tool output derived from theme colors
        let light_tool_color = lighten(text_color, ui::THEME_MIX_RATIO); // Lighter version of the text color
        let tool_color = ensure_contrast(
            light_tool_color,
            background,
            min_contrast,
            &[
                lighten(light_tool_color, ui::THEME_TOOL_BODY_LIGHTEN_RATIO),
                info_color,
                text_color,
            ],
        );
        let tool_body_candidate = mix(light_tool_color, text_color, ui::THEME_TOOL_BODY_MIX_RATIO);
        let tool_body_color = ensure_contrast(
            tool_body_candidate,
            background,
            min_contrast,
            &[
                lighten(light_tool_color, ui::THEME_TOOL_BODY_LIGHTEN_RATIO),
                text_color,
                fallback_light,
            ],
        );
        let tool_style = Style::new().fg_color(Some(Color::Rgb(tool_color)));
        let tool_detail_style = Style::new().fg_color(Some(Color::Rgb(tool_body_color)));
        let response_color = ensure_contrast(
            text_color,
            background,
            min_contrast,
            &[
                lighten(text_color, ui::THEME_RESPONSE_COLOR_LIGHTEN_RATIO),
                fallback_light,
            ],
        );
        let response_color = balance_text_luminance(response_color, background, min_contrast);

        // Reasoning color: Use text color with dimmed effect for placeholder-like appearance
        let reasoning_color = ensure_contrast(
            lighten(text_color, 0.25), // Lighter for placeholder-like appearance
            background,
            min_contrast,
            &[lighten(text_color, 0.15), text_color, fallback_light],
        );
        let reasoning_color = balance_text_luminance(reasoning_color, background, min_contrast);
        // Reasoning style: Dimmed and italic for placeholder-like thinking output
        let reasoning_style =
            Self::style_from(reasoning_color, false).effects(Effects::DIMMED | Effects::ITALIC);
        // Make user messages more distinct using secondary accent color

        let user_color = ensure_contrast(
            lighten(secondary, ui::THEME_USER_COLOR_LIGHTEN_RATIO),
            background,
            min_contrast,
            &[
                lighten(secondary, ui::THEME_SECONDARY_USER_COLOR_LIGHTEN_RATIO),
                info_color,
                text_color,
            ],
        );
        let user_color = balance_text_luminance(user_color, background, min_contrast);

        let alert_color = ensure_contrast(
            self.alert,
            background,
            min_contrast,
            &[
                lighten(self.alert, ui::THEME_LUMINANCE_LIGHTEN_RATIO),
                fallback_light,
                text_color,
            ],
        );

        let alert_color = balance_text_luminance(alert_color, background, min_contrast);

        // Tool output style: use default terminal styling (no color/bold/dim effects)
        let tool_output_style = Style::new();

        // PTY output style: subdued foreground for terminal output that's readable
        // but visually distinct from agent/user text — avoids terminal DIM modifier
        // which can be too faint on many terminals
        let pty_output_candidate = lighten(tool_body_color, ui::THEME_PTY_OUTPUT_LIGHTEN_RATIO);
        let pty_output_color = ensure_contrast(
            pty_output_candidate,
            background,
            min_contrast,
            &[
                lighten(text_color, ui::THEME_PTY_OUTPUT_LIGHTEN_RATIO),
                tool_body_color,
                text_color,
            ],
        );
        let pty_output_style = Style::new().fg_color(Some(Color::Rgb(pty_output_color)));

        let primary_style_color = balance_text_luminance(
            ensure_contrast(primary, background, min_contrast, &[text_color]),
            background,
            min_contrast,
        );
        let secondary_style_color = balance_text_luminance(
            ensure_contrast(
                secondary,
                background,
                min_contrast,
                &[info_color, text_color],
            ),
            background,
            min_contrast,
        );
        let logo_style_color = balance_text_luminance(
            ensure_contrast(
                logo_accent,
                background,
                min_contrast,
                &[secondary_style_color, text_color],
            ),
            background,
            min_contrast,
        );

        ThemeStyles {
            info: Self::style_from(info_color, true),
            error: Self::style_from(alert_color, true),
            output: Self::style_from(text_color, false),
            response: Self::style_from(response_color, false),
            reasoning: reasoning_style,
            tool: tool_style,
            tool_detail: tool_detail_style,
            tool_output: tool_output_style,
            pty_output: pty_output_style,
            status: Self::style_from(
                ensure_contrast(
                    lighten(primary_style_color, ui::THEME_PRIMARY_STATUS_LIGHTEN_RATIO),
                    background,
                    min_contrast,
                    &[
                        lighten(
                            primary_style_color,
                            ui::THEME_PRIMARY_STATUS_SECONDARY_LIGHTEN_RATIO,
                        ),
                        info_color,
                        text_color,
                    ],
                ),
                true,
            ),
            mcp: Self::style_from(
                ensure_contrast(
                    lighten(logo_style_color, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                    background,
                    min_contrast,
                    &[
                        lighten(logo_style_color, ui::THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO),
                        info_color,
                        fallback_light,
                    ],
                ),
                true,
            ),
            user: Self::style_from(user_color, false),
            primary: Self::style_from(primary_style_color, false),
            secondary: Self::style_from(secondary_style_color, false),
            background: Color::Rgb(background),
            foreground: Color::Rgb(text_color),
        }
    }
}

/// Styles computed from palette colors.
#[derive(Clone, Debug)]
pub struct ThemeStyles {
    pub info: Style,
    pub error: Style,
    pub output: Style,
    pub response: Style,
    pub reasoning: Style,
    pub tool: Style,
    pub tool_detail: Style,
    pub tool_output: Style,
    pub pty_output: Style,
    pub status: Style,
    pub mcp: Style,
    pub user: Style,
    pub primary: Style,
    pub secondary: Style,
    pub background: Color,
    pub foreground: Color,
}

#[derive(Clone, Debug)]
pub struct ThemeDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub palette: ThemePalette,
}

#[derive(Clone, Debug)]
struct ActiveTheme {
    id: String,
    label: String,
    palette: ThemePalette,
    styles: ThemeStyles,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum CatppuccinFlavorKind {
    Latte,
    Frappe,
    Macchiato,
    Mocha,
}

impl CatppuccinFlavorKind {
    const fn id(self) -> &'static str {
        match self {
            CatppuccinFlavorKind::Latte => "catppuccin-latte",
            CatppuccinFlavorKind::Frappe => "catppuccin-frappe",
            CatppuccinFlavorKind::Macchiato => "catppuccin-macchiato",
            CatppuccinFlavorKind::Mocha => "catppuccin-mocha",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            CatppuccinFlavorKind::Latte => "Catppuccin Latte",
            CatppuccinFlavorKind::Frappe => "Catppuccin Frappé",
            CatppuccinFlavorKind::Macchiato => "Catppuccin Macchiato",
            CatppuccinFlavorKind::Mocha => "Catppuccin Mocha",
        }
    }

    fn flavor(self) -> catppuccin::Flavor {
        match self {
            CatppuccinFlavorKind::Latte => PALETTE.latte,
            CatppuccinFlavorKind::Frappe => PALETTE.frappe,
            CatppuccinFlavorKind::Macchiato => PALETTE.macchiato,
            CatppuccinFlavorKind::Mocha => PALETTE.mocha,
        }
    }
}

static CATPPUCCIN_FLAVORS: &[CatppuccinFlavorKind] = &[
    CatppuccinFlavorKind::Latte,
    CatppuccinFlavorKind::Frappe,
    CatppuccinFlavorKind::Macchiato,
    CatppuccinFlavorKind::Mocha,
];

static REGISTRY: Lazy<HashMap<&'static str, ThemeDefinition>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "ciapre-dark",
        ThemeDefinition {
            id: "ciapre-dark",
            label: "Ciapre Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xBF, 0xB3, 0x8F),
                background: RgbColor(0x26, 0x26, 0x26),
                foreground: RgbColor(0xBF, 0xB3, 0x8F),
                secondary_accent: RgbColor(0xD9, 0x9A, 0x4E),
                alert: RgbColor(0xFF, 0x8A, 0x8A),
                logo_accent: RgbColor(0xD9, 0x9A, 0x4E),
            },
        },
    );
    map.insert(
        "ciapre-blue",
        ThemeDefinition {
            id: "ciapre-blue",
            label: "Ciapre Blue",
            palette: ThemePalette {
                primary_accent: RgbColor(0xBF, 0xB3, 0x8F),
                background: RgbColor(0x17, 0x1C, 0x26),
                foreground: RgbColor(0xBF, 0xB3, 0x8F),
                secondary_accent: RgbColor(0xBF, 0xB3, 0x8F),
                alert: RgbColor(0xFF, 0x8A, 0x8A),
                logo_accent: RgbColor(0xD9, 0x9A, 0x4E),
            },
        },
    );
    map.insert(
        "ciapre",
        ThemeDefinition {
            id: "ciapre",
            label: "Ciapre",
            palette: ThemePalette {
                primary_accent: RgbColor(0xAE, 0xA4, 0x7F), // White (#AEA47F)
                background: RgbColor(0x18, 0x18, 0x18),     // Black (#181818)
                foreground: RgbColor(0xAE, 0xA4, 0x7F),     // White (#AEA47F)
                secondary_accent: RgbColor(0xCC, 0x8A, 0x3E), // Yellow (#CC8A3E)
                alert: RgbColor(0xAC, 0x38, 0x35),          // Bright Red (#AC3835)
                logo_accent: RgbColor(0xCC, 0x8A, 0x3E),    // Yellow (#CC8A3E)
            },
        },
    );
    map.insert(
        "solarized-dark",
        ThemeDefinition {
            id: "solarized-dark",
            label: "Solarized Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0x83, 0x94, 0x96), // Base 0 (#839496)
                background: RgbColor(0x00, 0x2B, 0x36),     // Base 03 (#002b36)
                foreground: RgbColor(0x83, 0x94, 0x96),     // Base 0 (#839496)
                secondary_accent: RgbColor(0x26, 0x8B, 0xD2), // Blue (#268bd2)
                alert: RgbColor(0xDC, 0x32, 0x2F),          // Red (#dc322f)
                logo_accent: RgbColor(0xB5, 0x89, 0x00),    // Yellow (#b58900)
            },
        },
    );
    map.insert(
        "solarized-light",
        ThemeDefinition {
            id: "solarized-light",
            label: "Solarized Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x58, 0x6E, 0x75), // Base 00 (#586e75)
                background: RgbColor(0xFD, 0xF6, 0xE3),     // Base 3 (#fdf6e3)
                foreground: RgbColor(0x58, 0x6E, 0x75),     // Base 00 (#586e75)
                secondary_accent: RgbColor(0x26, 0x8B, 0xD2), // Blue (#268bd2)
                alert: RgbColor(0xDC, 0x32, 0x2F),          // Red (#dc322f)
                logo_accent: RgbColor(0xB5, 0x89, 0x00),    // Yellow (#b58900)
            },
        },
    );
    map.insert(
        "solarized-dark-hc",
        ThemeDefinition {
            id: "solarized-dark-hc",
            label: "Solarized Dark Higher Contrast",
            palette: ThemePalette {
                primary_accent: RgbColor(0x83, 0x94, 0x96), // Base 0 (#839496)
                background: RgbColor(0x00, 0x28, 0x31),     // Base 03 (#002831)
                foreground: RgbColor(0xE9, 0xE3, 0xCC),     // Base 1 (#e9e3cc)
                secondary_accent: RgbColor(0x20, 0x76, 0xC7), // Blue (#2076c7)
                alert: RgbColor(0xD1, 0x1C, 0x24),          // Red (#d11c24)
                logo_accent: RgbColor(0xA5, 0x77, 0x06),    // Yellow (#a57706)
            },
        },
    );

    // Gruvbox themes
    map.insert(
        "gruvbox-dark",
        ThemeDefinition {
            id: "gruvbox-dark",
            label: "Gruvbox Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xA8, 0x99, 0x84), // Light gray (#a89984)
                background: RgbColor(0x28, 0x28, 0x28),     // Dark background (#282828)
                foreground: RgbColor(0xA8, 0x99, 0x84),     // Light gray (#a89984)
                secondary_accent: RgbColor(0x45, 0x85, 0x88), // Cyan (#458588)
                alert: RgbColor(0xCC, 0x24, 0x1D),          // Red (#cc241d)
                logo_accent: RgbColor(0xD7, 0x99, 0x21),    // Yellow (#d79921)
            },
        },
    );
    map.insert(
        "gruvbox-dark-hard",
        ThemeDefinition {
            id: "gruvbox-dark-hard",
            label: "Gruvbox Dark Hard",
            palette: ThemePalette {
                primary_accent: RgbColor(0xA8, 0x99, 0x84), // Light gray (#a89984)
                background: RgbColor(0x1D, 0x20, 0x21),     // Hard dark (#1d2021)
                foreground: RgbColor(0xA8, 0x99, 0x84),     // Light gray (#a89984)
                secondary_accent: RgbColor(0x45, 0x85, 0x88), // Cyan (#458588)
                alert: RgbColor(0xCC, 0x24, 0x1D),          // Red (#cc241d)
                logo_accent: RgbColor(0xD7, 0x99, 0x21),    // Yellow (#d79921)
            },
        },
    );
    map.insert(
        "gruvbox-light",
        ThemeDefinition {
            id: "gruvbox-light",
            label: "Gruvbox Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x7C, 0x6F, 0x64), // Dark gray (#7c6f64)
                background: RgbColor(0xFB, 0xF4, 0xE8),     // Light background (#fbf4e8)
                foreground: RgbColor(0x7C, 0x6F, 0x64),     // Dark gray (#7c6f64)
                secondary_accent: RgbColor(0x45, 0x85, 0x88), // Cyan (#458588)
                alert: RgbColor(0xCC, 0x24, 0x1D),          // Red (#cc241d)
                logo_accent: RgbColor(0xD7, 0x99, 0x21),    // Yellow (#d79921)
            },
        },
    );
    map.insert(
        "gruvbox-light-hard",
        ThemeDefinition {
            id: "gruvbox-light-hard",
            label: "Gruvbox Light Hard",
            palette: ThemePalette {
                primary_accent: RgbColor(0x7C, 0x6F, 0x64), // Dark gray (#7c6f64)
                background: RgbColor(0xF9, 0xF5, 0xD7),     // Hard light (#f9f5d7)
                foreground: RgbColor(0x7C, 0x6F, 0x64),     // Dark gray (#7c6f64)
                secondary_accent: RgbColor(0x45, 0x85, 0x88), // Cyan (#458588)
                alert: RgbColor(0xCC, 0x24, 0x1D),          // Red (#cc241d)
                logo_accent: RgbColor(0xD7, 0x99, 0x21),    // Yellow (#d79921)
            },
        },
    );
    map.insert(
        "gruvbox-material",
        ThemeDefinition {
            id: "gruvbox-material",
            label: "Gruvbox Material",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF), // White (#ffffff)
                background: RgbColor(0x14, 0x16, 0x17),     // Dark background (#141617)
                foreground: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                secondary_accent: RgbColor(0x6D, 0xA3, 0xED), // Blue (#6da3ed)
                alert: RgbColor(0xEA, 0x69, 0x26),          // Orange (#ea6926)
                logo_accent: RgbColor(0xEE, 0xCE, 0x5B),    // Yellow (#eece5b)
            },
        },
    );
    map.insert(
        "gruvbox-material-dark",
        ThemeDefinition {
            id: "gruvbox-material-dark",
            label: "Gruvbox Material Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD4, 0xBE, 0x98), // Light tan (#d4be98)
                background: RgbColor(0x28, 0x28, 0x28),     // Dark background (#282828)
                foreground: RgbColor(0xD4, 0xBE, 0x98),     // Light tan (#d4be98)
                secondary_accent: RgbColor(0x7D, 0xAE, 0xA3), // Cyan (#7daea3)
                alert: RgbColor(0xEA, 0x69, 0x62),          // Red (#ea6962)
                logo_accent: RgbColor(0xD8, 0xA6, 0x57),    // Yellow (#d8a657)
            },
        },
    );
    map.insert(
        "gruvbox-material-light",
        ThemeDefinition {
            id: "gruvbox-material-light",
            label: "Gruvbox Material Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x65, 0x47, 0x35), // Dark brown (#654735)
                background: RgbColor(0xFB, 0xF1, 0xC7),     // Light background (#fbf1c7)
                foreground: RgbColor(0x65, 0x47, 0x35),     // Dark brown (#654735)
                secondary_accent: RgbColor(0x45, 0x70, 0x7A), // Cyan (#45707a)
                alert: RgbColor(0xC1, 0x4A, 0x4A),          // Red (#c14a4a)
                logo_accent: RgbColor(0xB4, 0x71, 0x09),    // Yellow (#b47109)
            },
        },
    );

    // Zenburn theme
    map.insert(
        "zenburn",
        ThemeDefinition {
            id: "zenburn",
            label: "Zenburn",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDC, 0xDC, 0xCC), // White (#dcdccc)
                background: RgbColor(0x4D, 0x4D, 0x4D),     // Dark background (#4d4d4d)
                foreground: RgbColor(0xDC, 0xDC, 0xCC),     // White (#dcdccc)
                secondary_accent: RgbColor(0x8C, 0xD0, 0xD3), // Cyan (#8cd0d3)
                alert: RgbColor(0x70, 0x50, 0x50),          // Red (#705050)
                logo_accent: RgbColor(0xF0, 0xDF, 0xAF),    // Yellow (#f0dfaf)
            },
        },
    );

    // Tomorrow themes
    map.insert(
        "tomorrow",
        ThemeDefinition {
            id: "tomorrow",
            label: "Tomorrow",
            palette: ThemePalette {
                primary_accent: RgbColor(0x4D, 0x4D, 0x4D), // Dark gray (#4d4d4d)
                background: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                foreground: RgbColor(0x4D, 0x4D, 0x4D),     // Dark gray (#4d4d4d)
                secondary_accent: RgbColor(0x42, 0x71, 0xAE), // Blue (#4271ae)
                alert: RgbColor(0xC8, 0x28, 0x29),          // Red (#c82829)
                logo_accent: RgbColor(0xEA, 0xB7, 0x00),    // Yellow (#eab700)
            },
        },
    );
    map.insert(
        "tomorrow-night",
        ThemeDefinition {
            id: "tomorrow-night",
            label: "Tomorrow Night",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDE, 0xDE, 0xDE), // Light gray (#dedede)
                background: RgbColor(0x1D, 0x1F, 0x21),     // Dark (#1d1f21)
                foreground: RgbColor(0xDE, 0xDE, 0xDE),     // Light gray (#dedede)
                secondary_accent: RgbColor(0x81, 0xA2, 0xBE), // Blue (#81a2be)
                alert: RgbColor(0xCC, 0x66, 0x66),          // Red (#cc6666)
                logo_accent: RgbColor(0xF0, 0xC6, 0x74),    // Yellow (#f0c674)
            },
        },
    );
    map.insert(
        "tomorrow-night-blue",
        ThemeDefinition {
            id: "tomorrow-night-blue",
            label: "Tomorrow Night Blue",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF), // White (#ffffff)
                background: RgbColor(0x00, 0x24, 0x51),     // Dark blue (#002451)
                foreground: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                secondary_accent: RgbColor(0xBB, 0xDA, 0xFF), // Blue (#bbdaff)
                alert: RgbColor(0xFF, 0x9D, 0xA4),          // Red (#ff9da4)
                logo_accent: RgbColor(0xFF, 0xEE, 0xAD),    // Yellow (#ffeead)
            },
        },
    );
    map.insert(
        "tomorrow-night-bright",
        ThemeDefinition {
            id: "tomorrow-night-bright",
            label: "Tomorrow Night Bright",
            palette: ThemePalette {
                primary_accent: RgbColor(0xE0, 0xE0, 0xE0), // Light gray (#e0e0e0)
                background: RgbColor(0x00, 0x00, 0x00),     // Black (#000000)
                foreground: RgbColor(0xE0, 0xE0, 0xE0),     // Light gray (#e0e0e0)
                secondary_accent: RgbColor(0x7A, 0xA6, 0xDA), // Blue (#7aa6da)
                alert: RgbColor(0xD5, 0x4E, 0x53),          // Red (#d54e53)
                logo_accent: RgbColor(0xE7, 0xC5, 0x47),    // Yellow (#e7c547)
            },
        },
    );
    map.insert(
        "tomorrow-night-burns",
        ThemeDefinition {
            id: "tomorrow-night-burns",
            label: "Tomorrow Night Burns",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF5, 0xF5, 0xF5), // White (#f5f5f5)
                background: RgbColor(0x25, 0x25, 0x25),     // Dark (#252525)
                foreground: RgbColor(0xF5, 0xF5, 0xF5),     // White (#f5f5f5)
                secondary_accent: RgbColor(0xFC, 0x59, 0x5F), // Red (#fc595f)
                alert: RgbColor(0xFC, 0x59, 0x5F),          // Red (#fc595f)
                logo_accent: RgbColor(0xE0, 0x93, 0x95),    // Light red (#e09395)
            },
        },
    );
    map.insert(
        "tomorrow-night-eighties",
        ThemeDefinition {
            id: "tomorrow-night-eighties",
            label: "Tomorrow Night Eighties",
            palette: ThemePalette {
                primary_accent: RgbColor(0xCC, 0xCC, 0xCC), // Light gray (#cccccc)
                background: RgbColor(0x2D, 0x2D, 0x2D),     // Dark (#2d2d2d)
                foreground: RgbColor(0xCC, 0xCC, 0xCC),     // Light gray (#cccccc)
                secondary_accent: RgbColor(0x66, 0x99, 0xCC), // Blue (#6699cc)
                alert: RgbColor(0xF2, 0x77, 0x7A),          // Red (#f2777a)
                logo_accent: RgbColor(0xFF, 0xCC, 0x66),    // Yellow (#ffcc66)
            },
        },
    );

    // Ayu themes
    map.insert(
        "ayu",
        ThemeDefinition {
            id: "ayu",
            label: "Ayu",
            palette: ThemePalette {
                primary_accent: RgbColor(0xC7, 0xC7, 0xC7), // Gray (#c7c7c7)
                background: RgbColor(0x11, 0x15, 0x1C),     // Dark (#11151c)
                foreground: RgbColor(0xC7, 0xC7, 0xC7),     // Gray (#c7c7c7)
                secondary_accent: RgbColor(0x53, 0xBD, 0xFA), // Blue (#53bdfa)
                alert: RgbColor(0xEA, 0x6C, 0x73),          // Red (#ea6c73)
                logo_accent: RgbColor(0xF9, 0xAF, 0x4F),    // Orange (#f9af4f)
            },
        },
    );
    map.insert(
        "ayu-mirage",
        ThemeDefinition {
            id: "ayu-mirage",
            label: "Ayu Mirage",
            palette: ThemePalette {
                primary_accent: RgbColor(0xC7, 0xC7, 0xC7), // Gray (#c7c7c7)
                background: RgbColor(0x17, 0x1B, 0x24),     // Dark blue (#171b24)
                foreground: RgbColor(0xC7, 0xC7, 0xC7),     // Gray (#c7c7c7)
                secondary_accent: RgbColor(0x6D, 0xCB, 0xFA), // Blue (#6dcbfa)
                alert: RgbColor(0xED, 0x82, 0x74),          // Red (#ed8274)
                logo_accent: RgbColor(0xFA, 0xCC, 0x6E),    // Orange (#facc6e)
            },
        },
    );

    // Material themes
    map.insert(
        "material-ocean",
        ThemeDefinition {
            id: "material-ocean",
            label: "Material Ocean",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF), // White (#ffffff)
                background: RgbColor(0x0F, 0x11, 0x1A),     // Dark blue (#0f111a)
                foreground: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                secondary_accent: RgbColor(0x82, 0xAA, 0xFF), // Blue (#82aaff)
                alert: RgbColor(0xFF, 0x53, 0x70),          // Red (#ff5370)
                logo_accent: RgbColor(0xFF, 0xCB, 0x6B),    // Yellow (#ffcb6b)
            },
        },
    );
    map.insert(
        "material-dark",
        ThemeDefinition {
            id: "material-dark",
            label: "Material Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEF, 0xEF, 0xEF), // Light gray (#efefef)
                background: RgbColor(0x21, 0x21, 0x21),     // Dark (#212121)
                foreground: RgbColor(0xEF, 0xEF, 0xEF),     // Light gray (#efefef)
                secondary_accent: RgbColor(0x13, 0x4E, 0xB2), // Blue (#134eb2)
                alert: RgbColor(0xB7, 0x14, 0x1F),          // Red (#b7141f)
                logo_accent: RgbColor(0xF6, 0x98, 0x1E),    // Yellow (#f6981e)
            },
        },
    );
    map.insert(
        "material",
        ThemeDefinition {
            id: "material",
            label: "Material",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEF, 0xEF, 0xEF), // Light gray (#efefef)
                background: RgbColor(0x21, 0x21, 0x21),     // Dark (#212121)
                foreground: RgbColor(0xEF, 0xEF, 0xEF),     // Light gray (#efefef)
                secondary_accent: RgbColor(0x14, 0x4E, 0xB2), // Blue (#144eb2)
                alert: RgbColor(0xB7, 0x14, 0x1F),          // Red (#b7141f)
                logo_accent: RgbColor(0xF6, 0x98, 0x1E),    // Yellow (#f6981e)
            },
        },
    );

    // GitHub themes
    map.insert(
        "github-dark",
        ThemeDefinition {
            id: "github-dark",
            label: "GitHub Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF), // White (#ffffff)
                background: RgbColor(0x0D, 0x11, 0x17),     // Dark (#0d1117)
                foreground: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                secondary_accent: RgbColor(0x6C, 0xA4, 0xF8), // Blue (#6ca4f8)
                alert: RgbColor(0xF7, 0x81, 0x66),          // Red (#f78166)
                logo_accent: RgbColor(0xE3, 0xB3, 0x41),    // Yellow (#e3b341)
            },
        },
    );
    map.insert(
        "github",
        ThemeDefinition {
            id: "github",
            label: "GitHub",
            palette: ThemePalette {
                primary_accent: RgbColor(0x3E, 0x3E, 0x3E), // Dark gray (#3e3e3e)
                background: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                foreground: RgbColor(0x3E, 0x3E, 0x3E),     // Dark gray (#3e3e3e)
                secondary_accent: RgbColor(0x00, 0x3E, 0x8A), // Blue (#003e8a)
                alert: RgbColor(0x97, 0x0B, 0x16),          // Red (#970b16)
                logo_accent: RgbColor(0xF8, 0xEE, 0xC7),    // Yellow (#f8eec7)
            },
        },
    );

    // Dracula theme
    map.insert(
        "dracula",
        ThemeDefinition {
            id: "dracula",
            label: "Dracula",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF8, 0xF8, 0xF2), // Light gray (#f8f8f2)
                background: RgbColor(0x21, 0x22, 0x2C),     // Dark (#21222c)
                foreground: RgbColor(0xF8, 0xF8, 0xF2),     // Light gray (#f8f8f2)
                secondary_accent: RgbColor(0xBD, 0x93, 0xF9), // Purple (#bd93f9)
                alert: RgbColor(0xFF, 0x55, 0x55),          // Red (#ff5555)
                logo_accent: RgbColor(0xF1, 0xFA, 0x8C),    // Yellow (#f1fa8c)
            },
        },
    );

    // Monokai Classic theme
    map.insert(
        "monokai-classic",
        ThemeDefinition {
            id: "monokai-classic",
            label: "Monokai Classic",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF8, 0xF8, 0xF2), // Light gray (#f8f8f2)
                background: RgbColor(0x27, 0x28, 0x22),     // Dark (#272822)
                foreground: RgbColor(0xF8, 0xF8, 0xF2),     // Light gray (#f8f8f2)
                secondary_accent: RgbColor(0x66, 0xD9, 0xEF), // Cyan (#66d9ef)
                alert: RgbColor(0xF9, 0x26, 0x72),          // Red (#f92672)
                logo_accent: RgbColor(0xE6, 0xDB, 0x74),    // Yellow (#e6db74)
            },
        },
    );

    // Night Owl theme
    map.insert(
        "night-owl",
        ThemeDefinition {
            id: "night-owl",
            label: "Night Owl",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF), // White (#ffffff)
                background: RgbColor(0x00, 0x16, 0x26),     // Dark blue (#001626)
                foreground: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                secondary_accent: RgbColor(0x82, 0xAA, 0xFF), // Blue (#82aaff)
                alert: RgbColor(0xEF, 0x53, 0x50),          // Red (#ef5350)
                logo_accent: RgbColor(0xAD, 0xDB, 0x89),    // Yellow (#addd89)
            },
        },
    );

    // Spacegray themes
    map.insert(
        "spacegray",
        ThemeDefinition {
            id: "spacegray",
            label: "Spacegray",
            palette: ThemePalette {
                primary_accent: RgbColor(0xB3, 0xB8, 0xC3), // Light gray (#b3b8c3)
                background: RgbColor(0x00, 0x00, 0x00),     // Black (#000000)
                foreground: RgbColor(0xB3, 0xB8, 0xC3),     // Light gray (#b3b8c3)
                secondary_accent: RgbColor(0x7D, 0x8F, 0xA4), // Blue (#7d8fa4)
                alert: RgbColor(0xB0, 0x4B, 0x57),          // Red (#b04b57)
                logo_accent: RgbColor(0xE5, 0xC1, 0x79),    // Yellow (#e5c179)
            },
        },
    );
    map.insert(
        "spacegray-bright",
        ThemeDefinition {
            id: "spacegray-bright",
            label: "Spacegray Bright",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD8, 0xD8, 0xD8), // Light gray (#d8d8d8)
                background: RgbColor(0x08, 0x08, 0x08),     // Dark (#080808)
                foreground: RgbColor(0xD8, 0xD8, 0xD8),     // Light gray (#d8d8d8)
                secondary_accent: RgbColor(0x7B, 0xAE, 0xBC), // Blue (#7baebc)
                alert: RgbColor(0xBD, 0x55, 0x53),          // Red (#bd5553)
                logo_accent: RgbColor(0xF6, 0xC9, 0x73),    // Yellow (#f6c973)
            },
        },
    );
    map.insert(
        "spacegray-eighties",
        ThemeDefinition {
            id: "spacegray-eighties",
            label: "Spacegray Eighties",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEF, 0xEC, 0xE7), // Light gray (#efec e7)
                background: RgbColor(0x15, 0x17, 0x1D),     // Dark (#15171d)
                foreground: RgbColor(0xEF, 0xEC, 0xE7),     // Light gray (#efec e7)
                secondary_accent: RgbColor(0x54, 0x86, 0xC0), // Blue (#5486c0)
                alert: RgbColor(0xEC, 0x5F, 0x67),          // Red (#ec5f67)
                logo_accent: RgbColor(0xFE, 0xC2, 0x54),    // Yellow (#fec254)
            },
        },
    );
    map.insert(
        "spacegray-eighties-dull",
        ThemeDefinition {
            id: "spacegray-eighties-dull",
            label: "Spacegray Eighties Dull",
            palette: ThemePalette {
                primary_accent: RgbColor(0xB3, 0xB8, 0xBC), // Light gray (#b3b8bc)
                background: RgbColor(0x15, 0x17, 0x1C),     // Dark (#15171c)
                foreground: RgbColor(0xB3, 0xB8, 0xBC),     // Light gray (#b3b8bc)
                secondary_accent: RgbColor(0x7C, 0x8F, 0x9E), // Blue (#7c8f9e)
                alert: RgbColor(0xB2, 0x4A, 0x56),          // Red (#b24a56)
                logo_accent: RgbColor(0xC6, 0x73, 0x44),    // Orange (#c67344)
            },
        },
    );

    // Atom themes
    map.insert(
        "atom",
        ThemeDefinition {
            id: "atom",
            label: "Atom",
            palette: ThemePalette {
                primary_accent: RgbColor(0xE0, 0xE0, 0xE0), // Light gray (#e0e0e0)
                background: RgbColor(0x00, 0x00, 0x00),     // Black (#000000)
                foreground: RgbColor(0xE0, 0xE0, 0xE0),     // Light gray (#e0e0e0)
                secondary_accent: RgbColor(0x85, 0xBE, 0xFE), // Blue (#85befe)
                alert: RgbColor(0xFD, 0x5F, 0xF1),          // Magenta (#fd5ff1)
                logo_accent: RgbColor(0xFF, 0xD7, 0xB1),    // Yellow (#ffd7b1)
            },
        },
    );
    map.insert(
        "atom-one-dark",
        ThemeDefinition {
            id: "atom-one-dark",
            label: "Atom One Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xAB, 0xB2, 0xBF), // Light gray (#abb2bf)
                background: RgbColor(0x21, 0x25, 0x2B),     // Dark (#21252b)
                foreground: RgbColor(0xAB, 0xB2, 0xBF),     // Light gray (#abb2bf)
                secondary_accent: RgbColor(0x61, 0xAF, 0xEF), // Blue (#61afef)
                alert: RgbColor(0xE0, 0x6C, 0x75),          // Red (#e06c75)
                logo_accent: RgbColor(0xE5, 0xC0, 0x7B),    // Yellow (#e5c07b)
            },
        },
    );
    map.insert(
        "atom-one-light",
        ThemeDefinition {
            id: "atom-one-light",
            label: "Atom One Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x3E, 0x3E, 0x3E), // Dark gray (#3e3e3e)
                background: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                foreground: RgbColor(0x3E, 0x3E, 0x3E),     // Dark gray (#3e3e3e)
                secondary_accent: RgbColor(0x2F, 0x5A, 0xF3), // Blue (#2f5af3)
                alert: RgbColor(0xDE, 0x3E, 0x35),          // Red (#de3e35)
                logo_accent: RgbColor(0xD2, 0xB6, 0x7C),    // Yellow (#d2b67c)
            },
        },
    );

    // Other popular themes
    map.insert(
        "man-page",
        ThemeDefinition {
            id: "man-page",
            label: "Man Page",
            palette: ThemePalette {
                primary_accent: RgbColor(0xCC, 0xCC, 0xCC), // Light gray (#cccccc)
                background: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                foreground: RgbColor(0xCC, 0xCC, 0xCC),     // Light gray (#cccccc)
                secondary_accent: RgbColor(0x00, 0x00, 0xB2), // Blue (#0000b2)
                alert: RgbColor(0xCC, 0x00, 0x00),          // Red (#cc0000)
                logo_accent: RgbColor(0x99, 0x99, 0x00),    // Yellow (#999900)
            },
        },
    );
    map.insert(
        "jetbrains-darcula",
        ThemeDefinition {
            id: "jetbrains-darcula",
            label: "JetBrains Darcula",
            palette: ThemePalette {
                primary_accent: RgbColor(0xAD, 0xAD, 0xAD), // Gray (#adadad)
                background: RgbColor(0x1E, 0x1E, 0x1E),     // Dark (#1e1e1e)
                foreground: RgbColor(0xAD, 0xAD, 0xAD),     // Gray (#adadad)
                secondary_accent: RgbColor(0x45, 0x82, 0xEB), // Blue (#4582eb)
                alert: RgbColor(0xFB, 0x54, 0x54),          // Red (#fb5454)
                logo_accent: RgbColor(0xC2, 0xC2, 0x00),    // Yellow (#c2c200)
            },
        },
    );
    map.insert(
        "homebrew",
        ThemeDefinition {
            id: "homebrew",
            label: "Homebrew",
            palette: ThemePalette {
                primary_accent: RgbColor(0xBF, 0xBF, 0xBF), // Light gray (#bfbfbf)
                background: RgbColor(0x00, 0x00, 0x00),     // Black (#000000)
                foreground: RgbColor(0xBF, 0xBF, 0xBF),     // Light gray (#bfbfbf)
                secondary_accent: RgbColor(0x00, 0x00, 0xB2), // Blue (#0000b2)
                alert: RgbColor(0x99, 0x00, 0x00),          // Red (#990000)
                logo_accent: RgbColor(0x99, 0x99, 0x00),    // Yellow (#999900)
            },
        },
    );
    map.insert(
        "framer",
        ThemeDefinition {
            id: "framer",
            label: "Framer",
            palette: ThemePalette {
                primary_accent: RgbColor(0xCC, 0xCC, 0xCC), // Light gray (#cccccc)
                background: RgbColor(0x14, 0x14, 0x14),     // Dark (#141414)
                foreground: RgbColor(0xCC, 0xCC, 0xCC),     // Light gray (#cccccc)
                secondary_accent: RgbColor(0x00, 0xAA, 0xFF), // Blue (#00aaff)
                alert: RgbColor(0xFF, 0x55, 0x55),          // Red (#ff5555)
                logo_accent: RgbColor(0xFF, 0xCC, 0x33),    // Yellow (#ffcc33)
            },
        },
    );
    map.insert(
        "espresso",
        ThemeDefinition {
            id: "espresso",
            label: "Espresso",
            palette: ThemePalette {
                primary_accent: RgbColor(0xEE, 0xEE, 0xEF), // Light gray (#eeeeef)
                background: RgbColor(0x35, 0x35, 0x35),     // Dark (#353535)
                foreground: RgbColor(0xEE, 0xEE, 0xEF),     // Light gray (#eeeeef)
                secondary_accent: RgbColor(0x6C, 0x99, 0xBB), // Blue (#6c99bb)
                alert: RgbColor(0xD2, 0x52, 0x52),          // Red (#d25252)
                logo_accent: RgbColor(0xFF, 0xC6, 0x6D),    // Yellow (#ffc66d)
            },
        },
    );
    map.insert(
        "adventure-time",
        ThemeDefinition {
            id: "adventure-time",
            label: "Adventure Time",
            palette: ThemePalette {
                primary_accent: RgbColor(0xF8, 0xDC, 0xC0), // Light tan (#f8dcc0)
                background: RgbColor(0x05, 0x04, 0x04),     // Dark (#050404)
                foreground: RgbColor(0xF8, 0xDC, 0xC0),     // Light tan (#f8dcc0)
                secondary_accent: RgbColor(0x0E, 0x49, 0xC6), // Blue (#0e49c6)
                alert: RgbColor(0xBD, 0x00, 0x13),          // Red (#bd0013)
                logo_accent: RgbColor(0xE8, 0x74, 0x1D),    // Orange (#e8741d)
            },
        },
    );
    map.insert(
        "afterglow",
        ThemeDefinition {
            id: "afterglow",
            label: "Afterglow",
            palette: ThemePalette {
                primary_accent: RgbColor(0xD0, 0xD0, 0xD0), // Light gray (#d0d0d0)
                background: RgbColor(0x15, 0x15, 0x15),     // Dark (#151515)
                foreground: RgbColor(0xD0, 0xD0, 0xD0),     // Light gray (#d0d0d0)
                secondary_accent: RgbColor(0x6C, 0x99, 0xBB), // Blue (#6c99bb)
                alert: RgbColor(0xAC, 0x41, 0x42),          // Red (#ac4142)
                logo_accent: RgbColor(0xE5, 0xB5, 0x67),    // Yellow (#e5b567)
            },
        },
    );
    map.insert(
        "apple-classic",
        ThemeDefinition {
            id: "apple-classic",
            label: "Apple Classic",
            palette: ThemePalette {
                primary_accent: RgbColor(0xC7, 0xC7, 0xC7), // Light gray (#c7c7c7)
                background: RgbColor(0x00, 0x00, 0x00),     // Black (#000000)
                foreground: RgbColor(0xC7, 0xC7, 0xC7),     // Light gray (#c7c7c7)
                secondary_accent: RgbColor(0x01, 0x25, 0xC8), // Blue (#0125c8)
                alert: RgbColor(0xCA, 0x1B, 0x11),          // Red (#ca1b11)
                logo_accent: RgbColor(0xC7, 0xC5, 0x00),    // Yellow (#c7c500)
            },
        },
    );
    map.insert(
        "apple-system-colors",
        ThemeDefinition {
            id: "apple-system-colors",
            label: "Apple System Colors",
            palette: ThemePalette {
                primary_accent: RgbColor(0x98, 0x98, 0x9D), // Gray (#98989d)
                background: RgbColor(0x1A, 0x1A, 0x1A),     // Dark (#1a1a1a)
                foreground: RgbColor(0x98, 0x98, 0x9D),     // Gray (#98989d)
                secondary_accent: RgbColor(0x08, 0x69, 0xC9), // Blue (#0869c9)
                alert: RgbColor(0xCC, 0x37, 0x2E),          // Red (#cc372e)
                logo_accent: RgbColor(0xCD, 0xAB, 0x1E),    // Yellow (#cdab1e)
            },
        },
    );
    map.insert(
        "apple-system-colors-light",
        ThemeDefinition {
            id: "apple-system-colors-light",
            label: "Apple System Colors Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x1A, 0x1A, 0x1A), // Dark gray (#1a1a1a)
                background: RgbColor(0xFF, 0xFF, 0xFF),     // White (#ffffff)
                foreground: RgbColor(0x1A, 0x1A, 0x1A),     // Dark gray (#1a1a1a)
                secondary_accent: RgbColor(0x2E, 0x68, 0xC5), // Blue (#2e68c5)
                alert: RgbColor(0xBC, 0x44, 0x37),          // Red (#bc4437)
                logo_accent: RgbColor(0xC8, 0xAD, 0x3A),    // Yellow (#c8ad3a)
            },
        },
    );

    // Vitesse themes
    map.insert(
        "vitesse-black",
        ThemeDefinition {
            id: "vitesse-black",
            label: "Vitesse Black",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDB, 0xD7, 0xCA), // Light gray foreground
                background: RgbColor(0x00, 0x00, 0x00),     // Black
                foreground: RgbColor(0xDB, 0xD7, 0xCA),     // Light gray
                secondary_accent: RgbColor(0x4D, 0x93, 0x75), // Green (selection color)
                alert: RgbColor(0xCB, 0x76, 0x76),          // Red for errors
                logo_accent: RgbColor(0xDB, 0xD7, 0xCA),    // Light gray for logo accent
            },
        },
    );
    map.insert(
        "vitesse-dark",
        ThemeDefinition {
            id: "vitesse-dark",
            label: "Vitesse Dark",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDB, 0xD7, 0xCA), // Light gray foreground
                background: RgbColor(0x12, 0x12, 0x12),     // Very dark gray
                foreground: RgbColor(0xDB, 0xD7, 0xCA),     // Light gray
                secondary_accent: RgbColor(0x4D, 0x93, 0x75), // Green (selection color)
                alert: RgbColor(0xCB, 0x76, 0x76),          // Red for errors
                logo_accent: RgbColor(0xDB, 0xD7, 0xCA),    // Light gray for logo accent
            },
        },
    );
    map.insert(
        "vitesse-dark-soft",
        ThemeDefinition {
            id: "vitesse-dark-soft",
            label: "Vitesse Dark Soft",
            palette: ThemePalette {
                primary_accent: RgbColor(0xDB, 0xD7, 0xCA), // Light gray foreground
                background: RgbColor(0x22, 0x22, 0x22),     // Very dark gray (soft)
                foreground: RgbColor(0xDB, 0xD7, 0xCA),     // Light gray
                secondary_accent: RgbColor(0x4D, 0x93, 0x75), // Green (selection color)
                alert: RgbColor(0xCB, 0x76, 0x76),          // Red for errors
                logo_accent: RgbColor(0xDB, 0xD7, 0xCA),    // Light gray for logo accent
            },
        },
    );
    map.insert(
        "vitesse-light",
        ThemeDefinition {
            id: "vitesse-light",
            label: "Vitesse Light",
            palette: ThemePalette {
                primary_accent: RgbColor(0x39, 0x3A, 0x34), // Dark gray foreground
                background: RgbColor(0xFF, 0xFF, 0xFF),     // White
                foreground: RgbColor(0x39, 0x3A, 0x34),     // Dark gray
                secondary_accent: RgbColor(0x1C, 0x6B, 0x48), // Green (selection color)
                alert: RgbColor(0xAB, 0x59, 0x59),          // Red for errors
                logo_accent: RgbColor(0x39, 0x3A, 0x34),    // Dark gray for logo accent
            },
        },
    );
    map.insert(
        "vitesse-light-soft",
        ThemeDefinition {
            id: "vitesse-light-soft",
            label: "Vitesse Light Soft",
            palette: ThemePalette {
                primary_accent: RgbColor(0x39, 0x3A, 0x34), // Dark gray foreground
                background: RgbColor(0xF1, 0xF0, 0xE9),     // Soft cream
                foreground: RgbColor(0x39, 0x3A, 0x34),     // Dark gray
                secondary_accent: RgbColor(0x1C, 0x6B, 0x48), // Green (selection color)
                alert: RgbColor(0xAB, 0x59, 0x59),          // Red for errors
                logo_accent: RgbColor(0x39, 0x3A, 0x34),    // Dark gray for logo accent
            },
        },
    );

    map.insert(
        "mono",
        ThemeDefinition {
            id: "mono",
            label: "Mono",
            palette: ThemePalette {
                primary_accent: RgbColor(0xFF, 0xFF, 0xFF),   // Pure white
                background: RgbColor(0x00, 0x00, 0x00),       // Black
                foreground: RgbColor(0xDB, 0xD7, 0xCA), // Soft light gray (borrowed from vitesse)
                secondary_accent: RgbColor(0xBB, 0xBB, 0xBB), // Medium gray
                alert: RgbColor(0xFF, 0xFF, 0xFF),      // High contrast white for alerts
                logo_accent: RgbColor(0xFF, 0xFF, 0xFF), // White for logo
            },
        },
    );

    map.insert(
        "ansi-classic",
        ThemeDefinition {
            id: "ansi-classic",
            label: "ANSI Classic",
            palette: ThemePalette {
                // Classic ANSI-inspired palette (IBM/VT-era feel): high contrast on black.
                primary_accent: RgbColor(0xC0, 0xC0, 0xC0), // silver/white-ish
                background: RgbColor(0x00, 0x00, 0x00),     // black
                foreground: RgbColor(0xC0, 0xC0, 0xC0),     // silver
                secondary_accent: RgbColor(0x00, 0xAA, 0xAA), // cyan
                alert: RgbColor(0xAA, 0x00, 0x00),          // red
                logo_accent: RgbColor(0xAA, 0xAA, 0x00),    // yellow
            },
        },
    );

    register_catppuccin_themes(&mut map);
    map
});

fn register_catppuccin_themes(map: &mut HashMap<&'static str, ThemeDefinition>) {
    for &flavor_kind in CATPPUCCIN_FLAVORS {
        let flavor = flavor_kind.flavor();
        let theme_definition = ThemeDefinition {
            id: flavor_kind.id(),
            label: flavor_kind.label(),
            palette: catppuccin_palette(flavor),
        };
        map.insert(flavor_kind.id(), theme_definition);
    }
}

fn catppuccin_palette(flavor: catppuccin::Flavor) -> ThemePalette {
    let colors = flavor.colors;
    ThemePalette {
        primary_accent: catppuccin_rgb(colors.lavender),
        background: catppuccin_rgb(colors.base),
        foreground: catppuccin_rgb(colors.text),
        secondary_accent: catppuccin_rgb(colors.sapphire),
        alert: catppuccin_rgb(colors.red),
        logo_accent: catppuccin_rgb(colors.peach),
    }
}

fn catppuccin_rgb(color: catppuccin::Color) -> RgbColor {
    RgbColor(color.rgb.r, color.rgb.g, color.rgb.b)
}

static ACTIVE: Lazy<RwLock<ActiveTheme>> = Lazy::new(|| {
    let default = REGISTRY
        .get(DEFAULT_THEME_ID)
        .expect("default theme must exist");
    let styles = default.palette.build_styles();
    RwLock::new(ActiveTheme {
        id: default.id.to_string(),
        label: default.label.to_string(),
        palette: default.palette.clone(),
        styles,
    })
});

/// Set the active theme by identifier.
pub fn set_active_theme(theme_id: &str) -> Result<()> {
    let id_lc = theme_id.trim().to_lowercase();
    let theme = REGISTRY
        .get(id_lc.as_str())
        .ok_or_else(|| anyhow!("Unknown theme '{theme_id}'"))?;

    let styles = theme.palette.build_styles();
    let mut guard = ACTIVE.write();
    guard.id = theme.id.to_string();
    guard.label = theme.label.to_string();
    guard.palette = theme.palette.clone();
    guard.styles = styles;
    Ok(())
}

/// Get the identifier of the active theme.
pub fn active_theme_id() -> String {
    ACTIVE.read().id.clone()
}

/// Get the human-readable label of the active theme.
pub fn active_theme_label() -> String {
    ACTIVE.read().label.clone()
}

/// Get the current styles cloned from the active theme.
pub fn active_styles() -> ThemeStyles {
    ACTIVE.read().styles.clone()
}

/// Slightly adjusted accent color for banner-like copy.
pub fn banner_color() -> RgbColor {
    let guard = ACTIVE.read();
    let accent = guard.palette.logo_accent;
    let secondary = guard.palette.secondary_accent;
    let background = guard.palette.background;
    drop(guard);

    let min_contrast = get_minimum_contrast();
    let candidate = lighten(accent, ui::THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO);
    ensure_contrast(
        candidate,
        background,
        min_contrast,
        &[
            lighten(accent, ui::THEME_PRIMARY_STATUS_SECONDARY_LIGHTEN_RATIO),
            lighten(
                secondary,
                ui::THEME_LOGO_ACCENT_BANNER_SECONDARY_LIGHTEN_RATIO,
            ),
            accent,
        ],
    )
}

/// Slightly darkened accent style for banner-like copy.
pub fn banner_style() -> Style {
    let accent = banner_color();
    Style::new().fg_color(Some(Color::Rgb(accent))).bold()
}

/// Accent color for the startup banner logo.
pub fn logo_accent_color() -> RgbColor {
    ACTIVE.read().palette.logo_accent
}

/// Enumerate available theme identifiers.
pub fn available_themes() -> Vec<&'static str> {
    let mut keys: Vec<_> = REGISTRY.keys().copied().collect();
    keys.sort();
    keys
}

/// Look up a theme label for display.
pub fn theme_label(theme_id: &str) -> Option<&'static str> {
    REGISTRY.get(theme_id).map(|definition| definition.label)
}

fn relative_luminance(color: RgbColor) -> f64 {
    fn channel(value: u8) -> f64 {
        let c = (value as f64) / 255.0;
        if c <= ui::THEME_RELATIVE_LUMINANCE_CUTOFF {
            c / ui::THEME_RELATIVE_LUMINANCE_LOW_FACTOR
        } else {
            ((c + ui::THEME_RELATIVE_LUMINANCE_OFFSET)
                / (1.0 + ui::THEME_RELATIVE_LUMINANCE_OFFSET))
                .powf(ui::THEME_RELATIVE_LUMINANCE_EXPONENT)
        }
    }
    let r = channel(color.0);
    let g = channel(color.1);
    let b = channel(color.2);
    ui::THEME_RED_LUMINANCE_COEFFICIENT * r
        + ui::THEME_GREEN_LUMINANCE_COEFFICIENT * g
        + ui::THEME_BLUE_LUMINANCE_COEFFICIENT * b
}

fn contrast_ratio(foreground: RgbColor, background: RgbColor) -> f64 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    let (lighter, darker) = if fg > bg { (fg, bg) } else { (bg, fg) };
    (lighter + ui::THEME_CONTRAST_RATIO_OFFSET) / (darker + ui::THEME_CONTRAST_RATIO_OFFSET)
}

fn darken(color: RgbColor, ratio: f64) -> RgbColor {
    mix(color, RgbColor(0, 0, 0), ratio)
}

fn adjust_luminance_to_target(color: RgbColor, target: f64) -> RgbColor {
    let current = relative_luminance(color);
    if (current - target).abs() < 1e-3 {
        return color;
    }

    if current < target {
        // Raise luminance by blending toward white.
        let denom = (1.0 - current).max(1e-6);
        let ratio = ((target - current) / denom).clamp(0.0, 1.0);
        lighten(color, ratio)
    } else {
        // Lower luminance by blending toward black.
        let denom = current.max(1e-6);
        let ratio = ((current - target) / denom).clamp(0.0, 1.0);
        darken(color, ratio)
    }
}

fn balance_text_luminance(color: RgbColor, background: RgbColor, min_contrast: f64) -> RgbColor {
    let bg_luminance = relative_luminance(background);
    let mut candidate = color;
    let current = relative_luminance(candidate);
    if bg_luminance < 0.5 {
        if current < MIN_DARK_BG_TEXT_LUMINANCE {
            candidate = adjust_luminance_to_target(candidate, MIN_DARK_BG_TEXT_LUMINANCE);
        } else if current > MAX_DARK_BG_TEXT_LUMINANCE {
            candidate = adjust_luminance_to_target(candidate, MAX_DARK_BG_TEXT_LUMINANCE);
        }
    } else if current > MAX_LIGHT_BG_TEXT_LUMINANCE {
        candidate = adjust_luminance_to_target(candidate, MAX_LIGHT_BG_TEXT_LUMINANCE);
    }

    ensure_contrast(candidate, background, min_contrast, &[color])
}

fn ensure_contrast(
    candidate: RgbColor,
    background: RgbColor,
    min_ratio: f64,
    fallbacks: &[RgbColor],
) -> RgbColor {
    if contrast_ratio(candidate, background) >= min_ratio {
        return candidate;
    }
    for &fallback in fallbacks {
        if contrast_ratio(fallback, background) >= min_ratio {
            return fallback;
        }
    }

    // Final accessibility fallback: choose the higher-contrast endpoint.
    let black = RgbColor(0, 0, 0);
    let white = RgbColor(255, 255, 255);
    if contrast_ratio(black, background) >= contrast_ratio(white, background) {
        black
    } else {
        white
    }
}

pub(crate) fn mix(color: RgbColor, target: RgbColor, ratio: f64) -> RgbColor {
    let ratio = ratio.clamp(ui::THEME_MIX_RATIO_MIN, ui::THEME_MIX_RATIO_MAX);
    let blend = |c: u8, t: u8| -> u8 {
        let c = c as f64;
        let t = t as f64;
        ((c + (t - c) * ratio).round()).clamp(ui::THEME_BLEND_CLAMP_MIN, ui::THEME_BLEND_CLAMP_MAX)
            as u8
    };
    RgbColor(
        blend(color.0, target.0),
        blend(color.1, target.1),
        blend(color.2, target.2),
    )
}

fn lighten(color: RgbColor, ratio: f64) -> RgbColor {
    mix(
        color,
        RgbColor(
            ui::THEME_COLOR_WHITE_RED,
            ui::THEME_COLOR_WHITE_GREEN,
            ui::THEME_COLOR_WHITE_BLUE,
        ),
        ratio,
    )
}

/// Resolve a theme identifier from configuration or CLI input.
pub fn resolve_theme(preferred: Option<String>) -> String {
    preferred
        .and_then(|candidate| {
            let trimmed = candidate.trim().to_lowercase();
            if trimmed.is_empty() {
                None
            } else if REGISTRY.contains_key(trimmed.as_str()) {
                Some(trimmed)
            } else {
                None
            }
        })
        .unwrap_or_else(|| DEFAULT_THEME_ID.to_string())
}

/// Validate a theme and return its label for messaging.
pub fn ensure_theme(theme_id: &str) -> Result<&'static str> {
    REGISTRY
        .get(theme_id)
        .map(|definition| definition.label)
        .context("Theme not found")
}

/// Rebuild the active theme's styles with current accessibility settings.
/// Call this after updating color accessibility configuration.
pub fn rebuild_active_styles() {
    let mut guard = ACTIVE.write();
    guard.styles = guard.palette.build_styles();
}

/// Theme validation result
#[derive(Debug, Clone)]
pub struct ThemeValidationResult {
    /// Whether the theme passed validation
    pub is_valid: bool,
    /// List of warnings (non-fatal issues)
    pub warnings: Vec<String>,
    /// List of errors (fatal issues)
    pub errors: Vec<String>,
}

/// Validate a theme's color contrast ratios.
/// Returns warnings for colors that don't meet WCAG AA standards.
pub fn validate_theme_contrast(theme_id: &str) -> ThemeValidationResult {
    let mut result = ThemeValidationResult {
        is_valid: true,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    let theme = match REGISTRY.get(theme_id) {
        Some(t) => t,
        None => {
            result.is_valid = false;
            result.errors.push(format!("Unknown theme: {}", theme_id));
            return result;
        }
    };

    let palette = &theme.palette;
    let bg = palette.background;
    let min_contrast = get_minimum_contrast();

    // Check main text colors
    let checks = [
        ("foreground", palette.foreground),
        ("primary_accent", palette.primary_accent),
        ("secondary_accent", palette.secondary_accent),
        ("alert", palette.alert),
        ("logo_accent", palette.logo_accent),
    ];

    for (name, color) in checks {
        let ratio = contrast_ratio(color, bg);
        if ratio < min_contrast {
            result.warnings.push(format!(
                "{} ({:02X}{:02X}{:02X}) has contrast ratio {:.2} < {:.1} against background",
                name, color.0, color.1, color.2, ratio, min_contrast
            ));
        }
    }

    result
}

/// Check if a theme is suitable for the detected terminal color scheme.
/// Returns true if the theme matches (light theme for light terminal, dark for dark).
pub fn theme_matches_terminal_scheme(theme_id: &str) -> bool {
    use crate::utils::ansi_capabilities::ColorScheme;
    use crate::utils::ansi_capabilities::detect_color_scheme;

    let scheme = detect_color_scheme();
    let theme_is_light = is_light_theme(theme_id);

    match scheme {
        ColorScheme::Light => theme_is_light,
        ColorScheme::Dark | ColorScheme::Unknown => !theme_is_light,
    }
}

/// Determine if a theme is a light theme based on its background luminance.
pub fn is_light_theme(theme_id: &str) -> bool {
    REGISTRY
        .get(theme_id)
        .map(|theme| {
            let bg = theme.palette.background;
            let luminance = relative_luminance(bg);
            // If background luminance > 0.5, it's a light theme
            luminance > 0.5
        })
        .unwrap_or(false)
}

/// Get a suggested theme based on terminal color scheme detection.
/// Returns a light or dark theme depending on detected terminal background.
pub fn suggest_theme_for_terminal() -> &'static str {
    use crate::utils::ansi_capabilities::ColorScheme;
    use crate::utils::ansi_capabilities::detect_color_scheme;

    match detect_color_scheme() {
        ColorScheme::Light => "vitesse-light",
        ColorScheme::Dark | ColorScheme::Unknown => DEFAULT_THEME_ID,
    }
}

/// Get the recommended syntax highlighting theme for a given UI theme.
/// This ensures that code highlighting colors complement the UI theme's background.
/// Based on: https://github.com/openai/codex/pull/11447, https://github.com/openai/codex/pull/12581
///
/// # Usage
///
/// For code blocks and syntax highlighting:
/// ```rust
/// use vtcode_core::ui::theme::{get_syntax_theme_for_ui_theme, active_theme_id};
/// let ui_theme = active_theme_id();
/// let syntax_theme = get_syntax_theme_for_ui_theme(&ui_theme);
/// // Use `syntax_theme` with syntect's ThemeSet
/// ```
///
/// For PTY/shell output highlighting, the same mapping applies.
/// The shell command highlighter should use the same color palette
/// as the syntax highlighting theme for visual consistency.
pub fn get_syntax_theme_for_ui_theme(ui_theme: &str) -> &'static str {
    match ui_theme.to_lowercase().as_str() {
        // Ayu themes - use matching syntect themes
        "ayu" => "ayu-dark",
        "ayu-mirage" => "ayu-mirage",

        // Catppuccin themes - use matching syntect themes
        "catppuccin-latte" => "catppuccin-latte",
        "catppuccin-frappe" => "catppuccin-frappe",
        "catppuccin-macchiato" => "catppuccin-macchiato",
        "catppuccin-mocha" => "catppuccin-mocha",

        // Solarized themes - exact TextMate theme names
        "solarized-dark" | "solarized-dark-hc" => "Solarized (dark)",
        "solarized-light" => "Solarized (light)",

        // Gruvbox themes
        "gruvbox-dark" | "gruvbox-dark-hard" => "gruvbox-dark",
        "gruvbox-light" | "gruvbox-light-hard" => "gruvbox-light",
        "gruvbox-material" | "gruvbox-material-dark" => "gruvbox-dark",
        "gruvbox-material-light" => "gruvbox-light",

        // Tomorrow themes - exact TextMate theme names
        "tomorrow" => "Tomorrow",
        "tomorrow-night" => "Tomorrow Night",
        "tomorrow-night-blue" => "Tomorrow Night Blue",
        "tomorrow-night-bright" => "Tomorrow Night Bright",
        "tomorrow-night-eighties" => "Tomorrow Night Eighties",
        "tomorrow-night-burns" => "Tomorrow Night",

        // GitHub themes - exact TextMate theme names
        "github-dark" => "GitHub Dark",
        "github" => "GitHub",

        // Atom themes - exact TextMate theme names
        "atom-one-dark" => "OneDark",
        "atom-one-light" => "OneLight",
        "atom" => "base16-ocean.dark",

        // Spacegray themes - use base16-ocean.dark as closest match
        "spacegray" | "spacegray-bright" | "spacegray-eighties" | "spacegray-eighties-dull" => {
            "base16-ocean.dark"
        }

        // Material themes - exact TextMate theme names
        "material-ocean" | "material-dark" | "material" => "Material Dark",

        // Other popular dark themes - exact TextMate theme names where available
        "dracula" => "Dracula",
        "monokai-classic" => "monokai-classic",
        "night-owl" => "Night Owl",
        "zenburn" => "Zenburn",

        // Fallback themes - use base16-ocean as a good general-purpose dark theme
        "jetbrains-darcula" => "base16-ocean.dark",
        "man-page" => "base16-ocean.dark",
        "homebrew" => "base16-ocean.dark",
        "framer" => "base16-ocean.dark",
        "espresso" => "base16-ocean.dark",
        "adventure-time" => "base16-ocean.dark",
        "afterglow" => "base16-ocean.dark",
        "apple-classic" => "base16-ocean.dark",
        "apple-system-colors" => "base16-ocean.dark",

        // Light themes - use base16-ocean.light as fallback
        "apple-system-colors-light" => "base16-ocean.light",
        "vitesse-light" | "vitesse-light-soft" => "base16-ocean.light",

        // Default dark themes
        "ciapre" | "ciapre-dark" | "ciapre-blue" => "base16-ocean.dark",
        "vitesse-black" | "vitesse-dark" | "vitesse-dark-soft" => "base16-ocean.dark",
        "mono" => "base16-ocean.dark",
        "ansi-classic" => "base16-ocean.dark",

        // Fallback to dark theme for unknown themes
        _ => "base16-ocean.dark",
    }
}

/// Get the recommended syntax highlighting theme for the currently active UI theme.
/// Convenience wrapper around `get_syntax_theme_for_ui_theme`.
pub fn get_active_syntax_theme() -> &'static str {
    get_syntax_theme_for_ui_theme(&active_theme_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mono_theme_exists() {
        let result = ensure_theme("mono");
        assert!(result.is_ok(), "Mono theme should be registered");
        assert_eq!(result.unwrap(), "Mono");
    }

    #[test]
    fn test_mono_theme_contrast() {
        let result = validate_theme_contrast("mono");
        assert!(result.errors.is_empty(), "Mono theme should have no errors");
        assert!(result.is_valid);
    }

    #[test]
    fn test_ansi_classic_theme_exists() {
        let result = ensure_theme("ansi-classic");
        assert!(result.is_ok(), "ANSI Classic theme should be registered");
        assert_eq!(result.unwrap(), "ANSI Classic");
    }

    #[test]
    fn test_all_themes_resolvable() {
        for id in available_themes() {
            assert!(
                ensure_theme(id).is_ok(),
                "Theme {} should be resolvable",
                id
            );
        }
    }

    #[test]
    fn test_all_themes_have_readable_foreground_and_accents() {
        let min_contrast = get_minimum_contrast();
        for definition in REGISTRY.values() {
            let styles = definition.palette.build_styles_with_contrast(min_contrast);
            let bg = definition.palette.background;

            for (name, color) in [
                ("foreground", style_rgb(styles.output)),
                ("primary", style_rgb(styles.primary)),
                ("secondary", style_rgb(styles.secondary)),
                ("user", style_rgb(styles.user)),
                ("response", style_rgb(styles.response)),
            ] {
                let color = color
                    .unwrap_or_else(|| panic!("{} missing fg color for {}", name, definition.id));
                let ratio = contrast_ratio(color, bg);
                assert!(
                    ratio >= min_contrast,
                    "theme={} style={} contrast {:.2} < {:.1}",
                    definition.id,
                    name,
                    ratio,
                    min_contrast
                );

                let luminance = relative_luminance(color);
                if relative_luminance(bg) < 0.5 {
                    assert!(
                        (MIN_DARK_BG_TEXT_LUMINANCE..=MAX_DARK_BG_TEXT_LUMINANCE)
                            .contains(&luminance),
                        "theme={} style={} luminance {:.3} outside dark-theme readability bounds",
                        definition.id,
                        name,
                        luminance
                    );
                } else {
                    assert!(
                        luminance <= MAX_LIGHT_BG_TEXT_LUMINANCE,
                        "theme={} style={} luminance {:.3} too bright for light theme",
                        definition.id,
                        name,
                        luminance
                    );
                }
            }
        }
    }

    fn style_rgb(style: Style) -> Option<RgbColor> {
        match style.get_fg_color() {
            Some(Color::Rgb(rgb)) => Some(rgb),
            _ => None,
        }
    }

    #[test]
    fn test_syntax_theme_mapping_dark_themes() {
        // Dark themes should map to dark syntax themes
        assert_eq!(get_syntax_theme_for_ui_theme("dracula"), "Dracula");
        assert_eq!(
            get_syntax_theme_for_ui_theme("monokai-classic"),
            "monokai-classic"
        );
        assert_eq!(get_syntax_theme_for_ui_theme("github-dark"), "GitHub Dark");
        assert_eq!(get_syntax_theme_for_ui_theme("atom-one-dark"), "OneDark");
        assert_eq!(get_syntax_theme_for_ui_theme("ayu"), "ayu-dark");
        assert_eq!(get_syntax_theme_for_ui_theme("ayu-mirage"), "ayu-mirage");
    }

    #[test]
    fn test_syntax_theme_mapping_light_themes() {
        // Light themes should map to light syntax themes
        assert_eq!(
            get_syntax_theme_for_ui_theme("solarized-light"),
            "Solarized (light)"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("vitesse-light"),
            "base16-ocean.light"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("apple-system-colors-light"),
            "base16-ocean.light"
        );
    }

    #[test]
    fn test_syntax_theme_mapping_solarized() {
        assert_eq!(
            get_syntax_theme_for_ui_theme("solarized-dark"),
            "Solarized (dark)"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("solarized-dark-hc"),
            "Solarized (dark)"
        );
    }

    #[test]
    fn test_syntax_theme_mapping_gruvbox() {
        assert_eq!(
            get_syntax_theme_for_ui_theme("gruvbox-dark"),
            "gruvbox-dark"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("gruvbox-light"),
            "gruvbox-light"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("gruvbox-material"),
            "gruvbox-dark"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("gruvbox-material-light"),
            "gruvbox-light"
        );
    }

    #[test]
    fn test_syntax_theme_mapping_tomorrow() {
        assert_eq!(
            get_syntax_theme_for_ui_theme("tomorrow-night"),
            "Tomorrow Night"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("tomorrow-night-blue"),
            "Tomorrow Night Blue"
        );
        assert_eq!(get_syntax_theme_for_ui_theme("tomorrow"), "Tomorrow");
    }

    #[test]
    fn test_syntax_theme_mapping_catppuccin() {
        assert_eq!(
            get_syntax_theme_for_ui_theme("catppuccin-mocha"),
            "catppuccin-mocha"
        );
        assert_eq!(
            get_syntax_theme_for_ui_theme("catppuccin-latte"),
            "catppuccin-latte"
        );
    }

    #[test]
    fn test_syntax_theme_mapping_fallback() {
        // Unknown themes should fall back to base16-ocean.dark
        assert_eq!(
            get_syntax_theme_for_ui_theme("unknown-theme-xyz"),
            "base16-ocean.dark"
        );
        assert_eq!(get_syntax_theme_for_ui_theme(""), "base16-ocean.dark");
    }

    #[test]
    fn test_syntax_theme_mapping_case_insensitive() {
        assert_eq!(get_syntax_theme_for_ui_theme("DRACULA"), "Dracula");
        assert_eq!(
            get_syntax_theme_for_ui_theme("Gruvbox-Dark"),
            "gruvbox-dark"
        );
    }
}
