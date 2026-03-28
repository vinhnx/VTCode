use anstyle::{Color, RgbColor, Style};
use anyhow::{Context, Result, anyhow};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use vtcode_config::constants::ui;

use crate::color_math::{contrast_ratio, ensure_contrast, lighten};
use crate::registry::theme_definition;
use crate::types::{
    ColorAccessibilityConfig, DEFAULT_THEME_ID, ThemeDefinition, ThemeStyles,
    ThemeValidationResult,
};

#[derive(Clone, Debug)]
struct ActiveTheme {
    definition: &'static ThemeDefinition,
    styles: ThemeStyles,
}

static COLOR_CONFIG: Lazy<RwLock<ColorAccessibilityConfig>> =
    Lazy::new(|| RwLock::new(ColorAccessibilityConfig::default()));

fn current_color_config() -> ColorAccessibilityConfig {
    COLOR_CONFIG.read().clone()
}

static ACTIVE: Lazy<RwLock<ActiveTheme>> = Lazy::new(|| {
    let default = theme_definition(DEFAULT_THEME_ID).expect("default theme must exist");
    let styles = default.palette.build_styles_with_accessibility(&current_color_config());
    RwLock::new(ActiveTheme {
        definition: default,
        styles,
    })
});

pub fn set_color_accessibility_config(config: ColorAccessibilityConfig) {
    *COLOR_CONFIG.write() = config;
}

pub fn get_minimum_contrast() -> f64 {
    COLOR_CONFIG.read().minimum_contrast
}

pub fn is_bold_bright_mode() -> bool {
    COLOR_CONFIG.read().bold_is_bright
}

pub fn is_safe_colors_only() -> bool {
    COLOR_CONFIG.read().safe_colors_only
}

pub fn set_active_theme(theme_id: &str) -> Result<()> {
    let id_lc = theme_id.trim().to_lowercase();
    let theme =
        theme_definition(id_lc.as_str()).ok_or_else(|| anyhow!("Unknown theme '{theme_id}'"))?;

    let styles = theme.palette.build_styles_with_accessibility(&current_color_config());
    let mut guard = ACTIVE.write();
    guard.definition = theme;
    guard.styles = styles;
    Ok(())
}

pub fn active_theme_id() -> String {
    ACTIVE.read().definition.id.to_string()
}

pub fn active_theme_label() -> String {
    ACTIVE.read().definition.label.to_string()
}

pub fn active_styles() -> ThemeStyles {
    ACTIVE.read().styles.clone()
}

pub fn banner_color() -> RgbColor {
    let guard = ACTIVE.read();
    let accent = guard.definition.palette.logo_accent;
    let secondary = guard.definition.palette.secondary_accent;
    let background = guard.definition.palette.background;
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

pub fn banner_style() -> Style {
    let accent = banner_color();
    Style::new().fg_color(Some(Color::Rgb(accent))).bold()
}

pub fn logo_accent_color() -> RgbColor {
    ACTIVE.read().definition.palette.logo_accent
}

pub fn resolve_theme(preferred: Option<String>) -> String {
    preferred
        .and_then(|candidate| {
            let trimmed = candidate.trim().to_lowercase();
            if trimmed.is_empty() {
                None
            } else if theme_definition(trimmed.as_str()).is_some() {
                Some(trimmed)
            } else {
                None
            }
        })
        .unwrap_or_else(|| DEFAULT_THEME_ID.to_string())
}

pub fn ensure_theme(theme_id: &str) -> Result<&'static str> {
    theme_definition(theme_id)
        .map(|definition| definition.label)
        .context("Theme not found")
}

pub fn rebuild_active_styles() {
    let mut guard = ACTIVE.write();
    guard.styles = guard
        .definition
        .palette
        .build_styles_with_accessibility(&current_color_config());
}

pub fn validate_theme_contrast(theme_id: &str) -> ThemeValidationResult {
    let mut result = ThemeValidationResult {
        is_valid: true,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    let theme = match theme_definition(theme_id) {
        Some(theme) => theme,
        None => {
            result.is_valid = false;
            result.errors.push(format!("Unknown theme: {}", theme_id));
            return result;
        }
    };

    let palette = &theme.palette;
    let bg = palette.background;
    let min_contrast = get_minimum_contrast();

    for (name, color) in [
        ("foreground", palette.foreground),
        ("primary_accent", palette.primary_accent),
        ("secondary_accent", palette.secondary_accent),
        ("alert", palette.alert),
        ("logo_accent", palette.logo_accent),
    ] {
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
