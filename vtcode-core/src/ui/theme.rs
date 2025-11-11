use anstyle::{Color, Effects, RgbColor, Style};
use anyhow::{Context, Result, anyhow};
use catppuccin::PALETTE;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;

use crate::config::constants::{defaults, ui};

/// Identifier for the default theme.
pub const DEFAULT_THEME_ID: &str = defaults::DEFAULT_THEME;

const MIN_CONTRAST: f64 = ui::THEME_MIN_CONTRAST_RATIO;

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
    fn style_from(color: RgbColor, bold: bool) -> Style {
        let mut style = Style::new().fg_color(Some(Color::Rgb(color)));
        if bold {
            style = style.bold();
        }
        style
    }

    fn build_styles(&self) -> ThemeStyles {
        let primary = self.primary_accent;
        let background = self.background;
        let secondary = self.secondary_accent;

        let fallback_light = RgbColor(
            ui::THEME_COLOR_WHITE_RED,
            ui::THEME_COLOR_WHITE_GREEN,
            ui::THEME_COLOR_WHITE_BLUE,
        );

        let text_color = ensure_contrast(
            self.foreground,
            background,
            MIN_CONTRAST,
            &[
                lighten(self.foreground, ui::THEME_FOREGROUND_LIGHTEN_RATIO),
                lighten(secondary, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                fallback_light,
            ],
        );
        let info_color = ensure_contrast(
            secondary,
            background,
            MIN_CONTRAST,
            &[
                lighten(secondary, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                text_color,
                fallback_light,
            ],
        );
        // Light gray for tool output derived from theme colors
        let light_tool_color = lighten(text_color, ui::THEME_MIX_RATIO); // Lighter version of the text color
        let tool_color = ensure_contrast(
            light_tool_color,
            background,
            MIN_CONTRAST,
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
            MIN_CONTRAST,
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
            MIN_CONTRAST,
            &[
                lighten(text_color, ui::THEME_RESPONSE_COLOR_LIGHTEN_RATIO),
                fallback_light,
            ],
        );
        let reasoning_color = ensure_contrast(
            lighten(secondary, ui::THEME_REASONING_COLOR_LIGHTEN_RATIO),
            background,
            MIN_CONTRAST,
            &[
                lighten(secondary, ui::THEME_RESPONSE_COLOR_LIGHTEN_RATIO),
                text_color,
                fallback_light,
            ],
        );
        let reasoning_style = Self::style_from(reasoning_color, false).effects(Effects::ITALIC);
        // Make user messages more distinct using secondary accent color
        let user_color = ensure_contrast(
            lighten(secondary, ui::THEME_USER_COLOR_LIGHTEN_RATIO),
            background,
            MIN_CONTRAST,
            &[
                lighten(secondary, ui::THEME_SECONDARY_USER_COLOR_LIGHTEN_RATIO),
                info_color,
                text_color,
            ],
        );
        let alert_color = ensure_contrast(
            self.alert,
            background,
            MIN_CONTRAST,
            &[
                lighten(self.alert, ui::THEME_LUMINANCE_LIGHTEN_RATIO),
                fallback_light,
                text_color,
            ],
        );

        ThemeStyles {
            info: Self::style_from(info_color, true),
            error: Self::style_from(alert_color, true),
            output: Self::style_from(text_color, false),
            response: Self::style_from(response_color, false),
            reasoning: reasoning_style,
            tool: tool_style,
            tool_detail: tool_detail_style,
            status: Self::style_from(
                ensure_contrast(
                    lighten(primary, ui::THEME_PRIMARY_STATUS_LIGHTEN_RATIO),
                    background,
                    MIN_CONTRAST,
                    &[
                        lighten(primary, ui::THEME_PRIMARY_STATUS_SECONDARY_LIGHTEN_RATIO),
                        info_color,
                        text_color,
                    ],
                ),
                true,
            ),
            mcp: Self::style_from(
                ensure_contrast(
                    lighten(self.logo_accent, ui::THEME_SECONDARY_LIGHTEN_RATIO),
                    background,
                    MIN_CONTRAST,
                    &[
                        lighten(self.logo_accent, ui::THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO),
                        info_color,
                        fallback_light,
                    ],
                ),
                true,
            ),
            user: Self::style_from(user_color, false),
            primary: Self::style_from(primary, false),
            secondary: Self::style_from(secondary, false),
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

    let candidate = lighten(accent, ui::THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO);
    ensure_contrast(
        candidate,
        background,
        MIN_CONTRAST,
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
    candidate
}

fn mix(color: RgbColor, target: RgbColor, ratio: f64) -> RgbColor {
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
