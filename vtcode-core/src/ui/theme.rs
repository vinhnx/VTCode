use anstyle::{Color, Effects, RgbColor, Style};
use anyhow::{Context, Result, anyhow};
use catppuccin::PALETTE;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;

use crate::config::constants::defaults;

/// Identifier for the default theme.
pub const DEFAULT_THEME_ID: &str = defaults::DEFAULT_THEME;

const MIN_CONTRAST: f64 = 4.5;

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

        let fallback_light = RgbColor(0xFF, 0xFF, 0xFF);

        let text_color = ensure_contrast(
            self.foreground,
            background,
            MIN_CONTRAST,
            &[
                lighten(self.foreground, 0.25),
                lighten(secondary, 0.2),
                fallback_light,
            ],
        );
        let info_color = ensure_contrast(
            secondary,
            background,
            MIN_CONTRAST,
            &[lighten(secondary, 0.2), text_color, fallback_light],
        );
        let tool_candidate = mix(self.alert, background, 0.35);
        let tool_color = ensure_contrast(
            tool_candidate,
            background,
            MIN_CONTRAST,
            &[self.alert, mix(self.alert, secondary, 0.25), fallback_light],
        );
        let tool_body_candidate = mix(tool_color, text_color, 0.35);
        let tool_body_color = ensure_contrast(
            tool_body_candidate,
            background,
            MIN_CONTRAST,
            &[lighten(tool_color, 0.2), text_color, fallback_light],
        );
        let tool_style = Style::new().fg_color(Some(Color::Rgb(tool_color))).bold();
        let tool_detail_style = Style::new().fg_color(Some(Color::Rgb(tool_body_color)));
        let response_color = ensure_contrast(
            text_color,
            background,
            MIN_CONTRAST,
            &[lighten(text_color, 0.15), fallback_light],
        );
        let reasoning_color = ensure_contrast(
            lighten(secondary, 0.3),
            background,
            MIN_CONTRAST,
            &[lighten(secondary, 0.15), text_color, fallback_light],
        );
        let reasoning_style = Self::style_from(reasoning_color, false).effects(Effects::ITALIC);
        let user_color = ensure_contrast(
            lighten(primary, 0.25),
            background,
            MIN_CONTRAST,
            &[lighten(secondary, 0.15), info_color, text_color],
        );
        let alert_color = ensure_contrast(
            self.alert,
            background,
            MIN_CONTRAST,
            &[lighten(self.alert, 0.2), fallback_light, text_color],
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
                    lighten(primary, 0.35),
                    background,
                    MIN_CONTRAST,
                    &[lighten(primary, 0.5), info_color, text_color],
                ),
                true,
            ),
            mcp: Self::style_from(
                ensure_contrast(
                    lighten(self.logo_accent, 0.2),
                    background,
                    MIN_CONTRAST,
                    &[lighten(self.logo_accent, 0.35), info_color, fallback_light],
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

    let candidate = lighten(accent, 0.35);
    ensure_contrast(
        candidate,
        background,
        MIN_CONTRAST,
        &[lighten(accent, 0.5), lighten(secondary, 0.25), accent],
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
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let r = channel(color.0);
    let g = channel(color.1);
    let b = channel(color.2);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast_ratio(foreground: RgbColor, background: RgbColor) -> f64 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    let (lighter, darker) = if fg > bg { (fg, bg) } else { (bg, fg) };
    (lighter + 0.05) / (darker + 0.05)
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
    let ratio = ratio.clamp(0.0, 1.0);
    let blend = |c: u8, t: u8| -> u8 {
        let c = c as f64;
        let t = t as f64;
        ((c + (t - c) * ratio).round()).clamp(0.0, 255.0) as u8
    };
    RgbColor(
        blend(color.0, target.0),
        blend(color.1, target.1),
        blend(color.2, target.2),
    )
}

fn lighten(color: RgbColor, ratio: f64) -> RgbColor {
    mix(color, RgbColor(0xFF, 0xFF, 0xFF), ratio)
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
