//! Shared UI palette tokens derived from the CLI theme registry.
//!
//! [`builtin_theme_colors`] is the consumer API: the desktop app (and any
//! other UI consumer) renders these colors, which run through the same
//! accessible style pipeline as the TUI. The [`gpui_kit_theme_colors`] family
//! instead exposes the raw upstream spec rows and exists to verify the port
//! (scheme agreement, upstream hue pins); for curated themes (Catppuccin,
//! Gruvbox, Solarized) the registry intentionally overrides the spec rows, so
//! the two answers differ there by design.

use anstyle::{Color, RgbColor};
use vtcode_config::constants::ui;

use crate::theme::color_math::{aa_muted_text, contrast_ratio, filled_text, mix, relative_luminance};
use crate::theme::gpui_kit::{UiThemePalette, UiThemeSpec, theme_specs};
use crate::theme::registry::theme_definition;
use crate::theme::types::ColorAccessibilityConfig;

/// Semantic palette colors for one theme, ready for RGB/HSLA consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiThemeColors {
    pub background: RgbColor,
    pub foreground: RgbColor,
    /// Brand accent: primary buttons, badges, focus rings.
    pub accent: RgbColor,
    /// Quieter companion accent for secondary highlights.
    pub secondary: RgbColor,
    pub danger: RgbColor,
    pub warning: RgbColor,
    pub info: RgbColor,
    pub success: RgbColor,
    /// Muted text that still clears the WCAG AA floor.
    pub muted: RgbColor,
    /// Nearest readable text color on a filled accent surface.
    pub accent_contrast: RgbColor,
}

/// The six ANSI hue slots carried by every VT Code theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiAnsiSlots {
    pub red: RgbColor,
    pub green: RgbColor,
    pub yellow: RgbColor,
    pub blue: RgbColor,
    pub magenta: RgbColor,
    pub cyan: RgbColor,
}

/// Pack an RGB color as `0xRRGGBB`, the form GPUI/theme pickers consume.
pub const fn rgb_u32(color: RgbColor) -> u32 {
    (color.0 as u32) << 16 | (color.1 as u32) << 8 | color.2 as u32
}

/// Whether a theme id belongs to the upstream gpui-kit theme sets.
pub fn is_gpui_kit_theme(theme_id: &str) -> bool {
    find_spec(theme_id).is_some()
}

/// Display label of a gpui-kit theme id, for pickers and docs.
pub fn gpui_kit_theme_label(theme_id: &str) -> Option<&'static str> {
    find_spec(theme_id).map(|spec| spec.label)
}

/// Derive the raw upstream palette tokens for a gpui-kit theme id.
/// Port-verification oracle only; UI consumers must use
/// [`builtin_theme_colors`], which reflects the curated registry entries.
pub fn gpui_kit_theme_colors(theme_id: &str) -> Option<UiThemeColors> {
    find_spec(theme_id).map(|spec| colors_for(&spec.palette))
}

/// Every gpui-kit theme as `(id, dark, colors)`, in upstream order.
/// Port-verification oracle only; see [`gpui_kit_theme_colors`].
pub fn gpui_kit_theme_colors_all() -> impl Iterator<Item = (&'static str, bool, UiThemeColors)> {
    theme_specs().iter().map(|spec| (spec.id, spec.dark, colors_for(&spec.palette)))
}

/// Shared desktop-ready colors for any built-in CLI theme.
/// Uses the CLI registry and the same accessible text and semantic styles as the TUI.
pub fn builtin_theme_colors(theme_id: &str) -> Option<UiThemeColors> {
    let palette = &theme_definition(theme_id)?.palette;
    let styles = palette.build_styles_with_accessibility(&ColorAccessibilityConfig::default());
    let rgb = |color: Option<Color>, fallback: RgbColor| match color {
        Some(Color::Rgb(color)) => color,
        _ => fallback,
    };
    let original_foreground = rgb(Some(styles.foreground), palette.foreground);
    // Desktop needs enough contrast headroom for raised surfaces as well as
    // the canvas. Move toward the best neutral only when the CLI text color
    // cannot support those surfaces, preserving its hue as far as possible.
    let black = RgbColor(0, 0, 0);
    let white = RgbColor(255, 255, 255);
    let target = if contrast_ratio(black, palette.background) > contrast_ratio(white, palette.background) {
        black
    } else {
        white
    };
    let foreground = (0..=100)
        .map(|step| mix(original_foreground, target, step as f32 / 100.0))
        .find(|color| contrast_ratio(*color, palette.background) >= 7.0)
        .unwrap_or(target);
    let accent = rgb(styles.mcp.get_fg_color(), foreground);
    Some(UiThemeColors {
        background: palette.background,
        foreground,
        accent,
        secondary: rgb(styles.secondary.get_fg_color(), foreground),
        danger: rgb(styles.error.get_fg_color(), foreground),
        warning: rgb(styles.warning.get_fg_color(), foreground),
        info: rgb(styles.info.get_fg_color(), foreground),
        success: if relative_luminance(palette.background) > 0.5 {
            RgbColor(0x23, 0x7a, 0x45)
        } else {
            RgbColor(0x63, 0xc7, 0x79)
        },
        muted: aa_muted_text(palette.background, foreground),
        accent_contrast: filled_text(accent),
    })
}

fn find_spec(theme_id: &str) -> Option<&'static UiThemeSpec> {
    let trimmed = theme_id.trim();
    theme_specs().iter().find(|spec| spec.id.eq_ignore_ascii_case(trimmed))
}

fn colors_for(palette: &UiThemePalette) -> UiThemeColors {
    let accent = pick_accent(&[palette.accent], palette);
    UiThemeColors {
        background: palette.background,
        foreground: palette.foreground,
        accent,
        secondary: pick_accent(&[palette.magenta, palette.cyan, palette.blue], palette),
        danger: pick_accent(&[palette.red], palette),
        warning: warning_for(palette),
        info: pick_accent(&[palette.blue, palette.cyan], palette),
        success: pick_accent(&[palette.green], palette),
        muted: aa_muted_text(palette.background, palette.foreground),
        accent_contrast: filled_text(accent),
    }
}

/// Keep the intended hue: first candidate that clears the configured contrast
/// floor wins; otherwise the most readable candidate (the shared style
/// pipeline then re-applies the floor with the foreground as fallback).
fn pick_accent(candidates: &[RgbColor], palette: &UiThemePalette) -> RgbColor {
    let floor = ui::THEME_MIN_CONTRAST_RATIO;
    if let Some(&readable) = candidates.iter().find(|&&c| contrast_ratio(c, palette.background) >= floor) {
        return readable;
    }
    candidates
        .iter()
        .copied()
        .reduce(|best, candidate| {
            if contrast_ratio(candidate, palette.background) > contrast_ratio(best, palette.background) {
                candidate
            } else {
                best
            }
        })
        .unwrap_or(palette.foreground)
}

/// Warning keeps the dedicated amber semantics the CLI token uses, falling
/// back to the theme yellow and then the default amber before readability.
fn warning_for(palette: &UiThemePalette) -> RgbColor {
    const AMBER: RgbColor = RgbColor(0xD9, 0x9A, 0x4E);
    const DARK_AMBER: RgbColor = RgbColor(0x98, 0x63, 0x11);
    let (candidate, alternate) = if relative_luminance(palette.background) < 0.5 {
        (AMBER, DARK_AMBER)
    } else {
        (DARK_AMBER, AMBER)
    };
    pick_accent(&[candidate, alternate, palette.yellow], palette)
}
