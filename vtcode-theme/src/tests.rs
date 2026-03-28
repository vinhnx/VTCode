use anstyle::{Color, RgbColor, Style};

use crate::color_math::{
    MAX_DARK_BG_TEXT_LUMINANCE, MAX_LIGHT_BG_TEXT_LUMINANCE, MIN_DARK_BG_TEXT_LUMINANCE,
    contrast_ratio, relative_luminance,
};
use crate::registry::all_theme_definitions;
use crate::*;

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
fn test_available_theme_suites_contains_expected_groups() {
    let suites = available_theme_suites();
    let suite_ids: Vec<&str> = suites.iter().map(|suite| suite.id).collect();
    assert!(suite_ids.contains(&"ciapre"));
    assert!(suite_ids.contains(&"vitesse"));
    assert!(suite_ids.contains(&"catppuccin"));
    assert!(suite_ids.contains(&"mono"));
}

#[test]
fn test_theme_suite_resolution() {
    assert_eq!(theme_suite_id("catppuccin-mocha"), Some("catppuccin"));
    assert_eq!(theme_suite_id("vitesse-light"), Some("vitesse"));
    assert_eq!(theme_suite_id("ciapre-dark"), Some("ciapre"));
    assert_eq!(theme_suite_id("mono"), Some("mono"));
    assert_eq!(theme_suite_id("unknown-theme"), None);
}

#[test]
fn test_all_themes_have_readable_foreground_and_accents() {
    let accessibility = ColorAccessibilityConfig::default();
    let min_contrast = accessibility.minimum_contrast;
    for definition in all_theme_definitions().values() {
        let styles = definition
            .palette
            .build_styles_with_accessibility(&accessibility);
        let bg = definition.palette.background;

        for (name, color) in [
            ("foreground", style_rgb(styles.output)),
            ("primary", style_rgb(styles.primary)),
            ("secondary", style_rgb(styles.secondary)),
            ("user", style_rgb(styles.user)),
            ("response", style_rgb(styles.response)),
        ] {
            let color =
                color.unwrap_or_else(|| panic!("{} missing fg color for {}", name, definition.id));
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
                    (MIN_DARK_BG_TEXT_LUMINANCE..=MAX_DARK_BG_TEXT_LUMINANCE).contains(&luminance),
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

#[test]
fn test_syntax_theme_mapping_dark_themes() {
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

fn style_rgb(style: Style) -> Option<RgbColor> {
    match style.get_fg_color() {
        Some(Color::Rgb(rgb)) => Some(rgb),
        _ => None,
    }
}
