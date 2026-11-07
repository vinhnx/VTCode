//! User-authored desktop themes.
//!
//! A custom theme stores the same seven seed colors as a built-in palette row,
//! so the shared [`super::apply_palette`] derivation still supplies every
//! derived token (fills, borders, muted text, focus ring) with its WCAG floors.
//! Rows therefore cannot make panels or controls unreadable by construction.

use gpui_kit::Hsla;
use gpui_kit::component::theme::ThemeMode;
use serde::{Deserialize, Serialize};

/// Longest display name a saved theme may carry.
const MAX_CUSTOM_THEME_NAME: usize = 40;
/// Prefix that keeps custom ids from ever colliding with a CLI palette id.
const CUSTOM_THEME_ID_PREFIX: &str = "custom-";
/// WCAG AA floor for body text.
const MINIMUM_TEXT_CONTRAST: f32 = 4.5;

/// One saved theme. `id` is stable across renames and never sent to the CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CustomTheme {
    pub id: String,
    pub name: String,
    pub mode: ThemeMode,
    pub background: Hsla,
    pub foreground: Hsla,
    pub accent: Hsla,
    pub secondary: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub warning: Hsla,
}

/// Why a staged theme cannot be saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomThemeError {
    EmptyName,
    NameTooLong,
    DuplicateName,
    LowContrast,
}

/// Validate a staged theme. `name_taken` compares case-insensitively against
/// every other saved theme, so a rename may keep its own name.
pub(crate) fn validate(
    name: &str,
    foreground: Hsla,
    background: Hsla,
    name_taken: bool,
) -> Result<(), CustomThemeError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CustomThemeError::EmptyName);
    }
    if name.chars().count() > MAX_CUSTOM_THEME_NAME {
        return Err(CustomThemeError::NameTooLong);
    }
    if name_taken {
        return Err(CustomThemeError::DuplicateName);
    }
    if super::contrast_ratio(foreground, background) < MINIMUM_TEXT_CONTRAST {
        return Err(CustomThemeError::LowContrast);
    }
    Ok(())
}

impl CustomTheme {
    /// Build a saved theme from a staged seed palette.
    pub(crate) fn from_seeds(id: String, name: String, seeds: super::PaletteSeeds) -> Self {
        Self {
            id,
            name,
            mode: seeds.mode,
            background: seeds.background,
            foreground: seeds.foreground,
            accent: seeds.accent,
            secondary: seeds.secondary,
            success: seeds.success,
            danger: seeds.danger,
            warning: seeds.warning,
        }
    }
}

/// True when `name` (trimmed, case-insensitive) belongs to another saved theme.
pub(crate) fn name_taken(customs: &[CustomTheme], name: &str, excluding: Option<&str>) -> bool {
    let name = name.trim().to_lowercase();
    customs
        .iter()
        .filter(|theme| Some(theme.id.as_str()) != excluding)
        .any(|theme| theme.name.trim().to_lowercase() == name)
}

/// Stable id for a new theme: `custom-<slug>`, suffixed when the slug is taken.
/// Existing ids never change, so renames keep their references intact.
pub(crate) fn custom_id_for(name: &str, customs: &[CustomTheme]) -> String {
    let slug = slug(name);
    let base = format!("{CUSTOM_THEME_ID_PREFIX}{slug}");
    // With N saved themes, N + 2 distinct candidates always contain a free
    // one, so the search terminates without a panic path.
    std::iter::once(base.clone())
        .chain((2_u32..).map(|suffix| format!("{base}-{suffix}")))
        .take(customs.len() + 2)
        .find(|candidate| !customs.iter().any(|theme| &theme.id == candidate))
        .unwrap_or(base)
}

fn slug(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "theme".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Skip invalid entries instead of quarantining every saved preference, so one
/// hand-edited or stale theme cannot make the application unreadable.
pub(crate) fn deserialize_custom_themes<'de, D>(deserializer: D) -> Result<Vec<CustomTheme>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = Vec::<serde_json::Value>::deserialize(deserializer)?;
    let mut themes = Vec::new();
    for value in values {
        match serde_json::from_value::<CustomTheme>(value) {
            Ok(theme) => {
                if !theme.id.starts_with(CUSTOM_THEME_ID_PREFIX)
                    || theme.id.len() == CUSTOM_THEME_ID_PREFIX.len()
                    || super::mode_for_id(&theme.id).is_some()
                    || themes.iter().any(|saved: &CustomTheme| saved.id == theme.id)
                {
                    vtcode_commons::diagnostics::stderr(format_args!(
                        "skipping custom desktop theme with invalid or duplicate id"
                    ));
                } else if let Err(error) = validate(&theme.name, theme.foreground, theme.background, false) {
                    vtcode_commons::diagnostics::stderr(format_args!(
                        "skipping invalid custom desktop theme: {error:?}"
                    ));
                } else {
                    themes.push(theme);
                }
            }
            Err(error) => {
                vtcode_commons::diagnostics::stderr(format_args!("skipping unreadable custom desktop theme: {error}"));
            }
        }
    }
    Ok(themes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::rgb;

    fn seed(id: &str, name: &str) -> CustomTheme {
        CustomTheme {
            id: id.to_string(),
            name: name.to_string(),
            mode: ThemeMode::Dark,
            background: rgb(0x101010).into(),
            foreground: rgb(0xf0f0f0).into(),
            accent: rgb(0x7aa2f7).into(),
            secondary: rgb(0xbb9af7).into(),
            success: rgb(0x63c779).into(),
            danger: rgb(0xf7768e).into(),
            warning: rgb(0xe0af68).into(),
        }
    }

    #[test]
    fn ids_slug_names_and_never_collide() {
        let mut customs = vec![seed("custom-ocean-night", "Ocean Night")];
        assert_eq!(custom_id_for("Paper", &customs), "custom-paper");
        assert_eq!(custom_id_for("Ocean Night", &customs), "custom-ocean-night-2");
        customs.push(seed("custom-ocean-night-2", "Ocean Night 2"));
        assert_eq!(custom_id_for("Ocean Night", &customs), "custom-ocean-night-3");
        assert_eq!(custom_id_for("***", &[]), "custom-theme");
        assert_eq!(custom_id_for("  Mixed Case  ", &[]), "custom-mixed-case");
    }

    /// The `custom-` prefix has no built-in counterpart, so a generated id can
    /// never shadow a palette row (which would make `apply_selection` render
    /// the custom palette while `cli_id_for` resolved the built-in).
    #[test]
    fn generated_ids_never_shadow_a_builtin_palette_id() {
        for theme in ["Ciapre", "System Light", "Catppuccin Latte", "custom", ""] {
            let id = custom_id_for(theme, &[]);
            assert!(id.starts_with(CUSTOM_THEME_ID_PREFIX), "{theme} -> {id}");
            assert!(super::super::find(&id).is_none(), "{id} must not resolve as a built-in");
            assert!(super::super::cli_id_for(&id).is_none(), "{id} must not map to a CLI palette");
        }
    }

    #[test]
    fn duplicate_names_ignore_case_and_the_edited_row() {
        let customs = vec![seed("custom-a", "Paper"), seed("custom-b", "Ocean")];
        assert!(name_taken(&customs, "paper", None));
        assert!(name_taken(&customs, " PAPER ", None));
        assert!(!name_taken(&customs, "Paper", Some("custom-a")));
        assert!(!name_taken(&customs, "Night", None));
    }

    #[test]
    fn validation_rejects_empty_long_duplicate_and_low_contrast() {
        let background = rgb(0x101010).into();
        assert_eq!(validate("  ", background, background, false), Err(CustomThemeError::EmptyName));
        assert_eq!(
            validate(&"x".repeat(MAX_CUSTOM_THEME_NAME + 1), background, background, false),
            Err(CustomThemeError::NameTooLong)
        );
        assert_eq!(validate("Paper", rgb(0xf0f0f0).into(), background, true), Err(CustomThemeError::DuplicateName));
        // Same color on both seats can never reach 4.5:1.
        assert_eq!(validate("Paper", background, background, false), Err(CustomThemeError::LowContrast));
        assert_eq!(validate("Paper", rgb(0xf0f0f0).into(), background, false), Ok(()));
    }

    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default, deserialize_with = "deserialize_custom_themes")]
        themes: Vec<CustomTheme>,
    }

    #[test]
    fn unreadable_entries_are_skipped_not_fatal() {
        let wrapper: Wrapper = serde_json::from_value(serde_json::json!({
            "themes": [
                {"id": "custom-good", "name": "Good", "mode": "dark",
                 "background": "#101010ff", "foreground": "#f0f0f0ff", "accent": "#7aa2f7ff",
                 "secondary": "#bb9af7ff", "success": "#63c779ff", "danger": "#f7768eff",
                 "warning": "#e0af68ff"},
                {"id": "custom-bad"}
            ]
        }))
        .unwrap();
        assert_eq!(wrapper.themes.len(), 1);
        assert_eq!(wrapper.themes[0].id, "custom-good");
        let empty: Wrapper = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(empty.themes.is_empty());
    }

    #[test]
    fn invalid_saved_contrast_does_not_discard_readable_neighbors() {
        let mut invalid = seed("custom-invalid", "Invalid");
        invalid.foreground = invalid.background;
        let mut light = seed("custom-light", "Light");
        light.mode = ThemeMode::Light;
        light.background = rgb(0xfafafa).into();
        light.foreground = rgb(0x202020).into();
        let wrapper: Wrapper = serde_json::from_value(serde_json::json!({
            "themes": [seed("custom-dark", "Dark"), invalid, light]
        }))
        .unwrap();
        assert_eq!(
            wrapper.themes.iter().map(|theme| theme.id.as_str()).collect::<Vec<_>>(),
            ["custom-dark", "custom-light"]
        );
    }

    #[test]
    fn loaded_ids_cannot_shadow_builtins_or_alias_another_theme() {
        let first = seed("custom-ocean", "First");
        let duplicate = seed("custom-ocean", "Second");
        let wrapper: Wrapper = serde_json::from_value(serde_json::json!({
            "themes": [seed("ciapre", "Shadow"), seed("system-light", "System"),
                       seed("custom-", "Empty"), first, duplicate, seed("custom-paper", "Paper")]
        }))
        .unwrap();
        assert_eq!(
            wrapper
                .themes
                .iter()
                .map(|theme| (theme.id.as_str(), theme.name.as_str()))
                .collect::<Vec<_>>(),
            [("custom-ocean", "First"), ("custom-paper", "Paper")]
        );
    }
}
