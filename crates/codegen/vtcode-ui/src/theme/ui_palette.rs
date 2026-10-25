//! Converts the terminal-free [`super::gpui_kit`] theme specs into shared UI
//! palette tokens, so the desktop app (and any other UI consumer) renders the
//! exact colors the CLI registry derives from the same upstream rows.

use anstyle::RgbColor;
use vtcode_config::constants::ui;

use crate::theme::color_math::{aa_muted_text, contrast_ratio, filled_text, relative_luminance};
use crate::theme::gpui_kit::{UiThemePalette, UiThemeSpec, theme_specs};

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

/// Derive the shared palette tokens for a gpui-kit theme id.
pub fn gpui_kit_theme_colors(theme_id: &str) -> Option<UiThemeColors> {
    find_spec(theme_id).map(|spec| colors_for(&spec.palette))
}

/// Every gpui-kit theme as `(id, dark, colors)`, in upstream order.
pub fn gpui_kit_theme_colors_all() -> impl Iterator<Item = (&'static str, bool, UiThemeColors)> {
    theme_specs().iter().map(|spec| (spec.id, spec.dark, colors_for(&spec.palette)))
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
