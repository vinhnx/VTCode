use anstyle::{Color, RgbColor, Style};
use anyhow::{Context, Result, anyhow};
use arc_swap::{ArcSwap, ArcSwapOption};
use once_cell::sync::Lazy;
use std::sync::Arc;
use vtcode_config::constants::ui;

use crate::theme::color_math::{contrast_ratio, ensure_contrast, lighten};
use crate::theme::registry::theme_definition;
use crate::theme::types::{
    ColorAccessibilityConfig, DEFAULT_THEME_ID, ThemeDefinition, ThemeStyles, ThemeValidationResult,
};

#[derive(Clone, Debug)]
struct ActiveTheme {
    definition: &'static ThemeDefinition,
    styles: ThemeStyles,
}

static COLOR_CONFIG: Lazy<ArcSwap<ColorAccessibilityConfig>> =
    Lazy::new(|| ArcSwap::from_pointee(ColorAccessibilityConfig::default()));

fn current_color_config() -> Arc<ColorAccessibilityConfig> {
    COLOR_CONFIG.load_full()
}

static ACTIVE: Lazy<ArcSwap<ActiveTheme>> = Lazy::new(|| {
    let default = theme_definition(DEFAULT_THEME_ID).expect("default theme must exist");
    let styles = default.palette.build_styles_with_accessibility(&current_color_config());
    ArcSwap::from_pointee(ActiveTheme { definition: default, styles })
});

/// Preview state: when set, `active_styles()` returns the preview styles
/// instead of the committed theme styles. This allows theme palette
/// navigation to show a live preview without committing the selection.
///
/// Read-mostly whole-value state (reads on every render, writes only on
/// theme switch/preview): `ArcSwap` keeps reads lock-free while preserving
/// atomic replacement. This follows the RwLock-vs-lockfree guidance — a
/// coarse `RwLock` would work but pays an atomic read-modify-write per
/// acquisition even without contention; whole-value swap avoids it.
static PREVIEW: Lazy<ArcSwapOption<ActiveTheme>> = Lazy::new(ArcSwapOption::empty);

/// Update the runtime color accessibility configuration.
pub fn set_color_accessibility_config(config: ColorAccessibilityConfig) {
    COLOR_CONFIG.store(Arc::new(config));
}

/// Return the currently configured minimum contrast ratio.
pub fn get_minimum_contrast() -> f32 {
    COLOR_CONFIG.load_full().minimum_contrast
}

/// Report whether bold text should avoid terminal bright-color behavior.
pub fn is_bold_bright_mode() -> bool {
    COLOR_CONFIG.load_full().bold_is_bright
}

/// Report whether the UI should restrict itself to safe ANSI colors.
pub fn is_safe_colors_only() -> bool {
    COLOR_CONFIG.load_full().safe_colors_only
}

/// Activate a built-in theme by identifier.
///
/// A committed selection supersedes any temporary palette preview. Leaving a
/// preview in place here would make [`active_styles`] return stale preview
/// colors after the caller has changed the committed theme.
pub fn set_active_theme(theme_id: &str) -> Result<()> {
    let id_lc = theme_id.trim().to_lowercase();
    let theme = theme_definition(id_lc.as_str()).ok_or_else(|| anyhow!("Unknown theme '{theme_id}'"))?;

    let styles = theme.palette.build_styles_with_accessibility(&current_color_config());
    PREVIEW.store(None);
    ACTIVE.store(Arc::new(ActiveTheme { definition: theme, styles }));
    Ok(())
}

/// Return the active theme identifier.
pub fn active_theme_id() -> String {
    ACTIVE.load_full().definition.id.to_string()
}

/// Return the active theme label.
pub fn active_theme_label() -> String {
    ACTIVE.load_full().definition.label.to_string()
}

/// Return a clone of the active style set.
/// When a preview theme is active, returns the preview styles instead.
pub fn active_styles() -> ThemeStyles {
    if let Some(preview) = PREVIEW.load_full() {
        return preview.styles.clone();
    }
    ACTIVE.load_full().styles.clone()
}

/// Set a preview theme by identifier. The preview is returned by
/// `active_styles()` until `clear_preview_theme()` is called.
pub fn set_preview_theme(theme_id: &str) -> Result<()> {
    let id_lc = theme_id.trim().to_lowercase();
    let theme = theme_definition(id_lc.as_str()).ok_or_else(|| anyhow!("Unknown theme '{theme_id}'"))?;
    let styles = theme.palette.build_styles_with_accessibility(&current_color_config());
    PREVIEW.store(Some(Arc::new(ActiveTheme { definition: theme, styles })));
    Ok(())
}

/// Return true when a preview theme is active.
pub fn has_preview_theme() -> bool {
    PREVIEW.load_full().is_some()
}

/// Clear the preview theme, reverting `active_styles()` to the committed theme.
pub fn clear_preview_theme() {
    PREVIEW.store(None);
}

/// Return a readable accent color for banner-like copy.
pub fn banner_color() -> RgbColor {
    let active = ACTIVE.load_full();
    let accent = active.definition.palette.logo_accent;
    let secondary = active.definition.palette.secondary_accent;
    let background = active.definition.palette.background;

    let min_contrast = get_minimum_contrast();
    let candidate = lighten(accent, ui::THEME_LOGO_ACCENT_BANNER_LIGHTEN_RATIO);
    ensure_contrast(
        candidate,
        background,
        min_contrast,
        &[
            lighten(accent, ui::THEME_PRIMARY_STATUS_SECONDARY_LIGHTEN_RATIO),
            lighten(secondary, ui::THEME_LOGO_ACCENT_BANNER_SECONDARY_LIGHTEN_RATIO),
            accent,
        ],
    )
}

/// Return a bold banner style derived from the active theme.
pub fn banner_style() -> Style {
    let accent = banner_color();
    Style::new().fg_color(Some(Color::Rgb(accent))).bold()
}

/// Return the raw logo accent color from the active theme.
pub fn logo_accent_color() -> RgbColor {
    ACTIVE.load_full().definition.palette.logo_accent
}

/// Contrast ratio of a style's foreground against the active theme background.
///
/// Returns `None` when the style carries no RGB foreground (unset, ANSI16, or
/// ANSI256 are not theme-relative). Consumers that build their own styled
/// surfaces — for example the CLI exit postamble — use this to prove the
/// surface meets the configured WCAG minimum (`get_minimum_contrast`, 4.5:1 by
/// default) instead of shipping hand-picked colors.
pub fn style_contrast_ratio(style: &Style) -> Option<f32> {
    let Color::Rgb(foreground) = style.get_fg_color()? else {
        return None;
    };
    let background = ACTIVE.load_full().definition.palette.background;
    Some(contrast_ratio(RgbColor(foreground.r(), foreground.g(), foreground.b()), background))
}

/// Resolve a requested theme to a valid built-in identifier or the default.
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

/// Validate that a theme exists and return its label.
pub fn ensure_theme(theme_id: &str) -> Result<&'static str> {
    theme_definition(theme_id)
        .map(|definition| definition.label)
        .context("Theme not found")
}

/// Rebuild the active styles after accessibility settings change.
pub fn rebuild_active_styles() {
    let current = ACTIVE.load_full();
    let mut updated = (*current).clone();
    updated.styles = updated
        .definition
        .palette
        .build_styles_with_accessibility(&current_color_config());
    ACTIVE.store(Arc::new(updated));
}

/// Validate a theme's base palette contrast ratios.
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
            result.errors.push(format!("Unknown theme: {theme_id}"));
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
