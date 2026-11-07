use super::UiLanguage;

#[test]
fn contrast_description_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let label = rust_i18n::t!("app.settings.appearance.contrast", locale = locale).to_string();
        let description = rust_i18n::t!("app.settings.appearance.contrast_description", locale = locale).to_string();
        assert!(!label.is_empty() && !description.is_empty(), "{locale}");
        if language != UiLanguage::English {
            assert_ne!(description, "Increase text contrast while preserving the selected theme", "{locale}");
        }
    }
}

#[test]
fn app_icon_copy_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for (key, english) in [
            ("app.settings.appearance.app_icon", "App Icon"),
            ("app.settings.appearance.app_icon_description", "Dynamic follows the active theme mode."),
            ("app.settings.appearance.app_icon_dynamic", "Dynamic"),
            ("app.settings.appearance.app_icon_clear", "Clear"),
            ("app.settings.appearance.app_icon_tinted", "Tinted"),
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if language != UiLanguage::English {
                assert_ne!(value, english, "{locale}/{key}");
            }
        }
    }
}

/// Every custom-theme string, split by how it can be checked: plain labels,
/// the two contrast captions (which interpolate the live ratio), and the group
/// copy (the only place a machine translation is guaranteed to differ from
/// English — seed labels legitimately read "Accent"/"Text" in several locales).
const CUSTOM_THEME_LABEL_KEYS: &[&str] = &[
    "app.appearance.custom_themes_empty",
    "app.appearance.custom_theme_new",
    "app.appearance.custom_theme_edit",
    "app.appearance.custom_theme_delete",
    "app.appearance.custom_theme_name",
    "app.appearance.custom_theme_name_placeholder",
    "app.appearance.custom_theme_preview",
    "app.appearance.custom_theme_save",
    "app.appearance.custom_theme_cancel",
    "app.appearance.custom_theme_delete_detail",
    "app.appearance.custom_theme_name_required",
    "app.appearance.custom_theme_name_too_long",
    "app.appearance.custom_theme_name_taken",
    "app.appearance.custom_theme_contrast_low_error",
];
const CUSTOM_THEME_RATIO_KEYS: &[&str] = &[
    "app.appearance.custom_theme_contrast",
    "app.appearance.custom_theme_contrast_low",
];
const CUSTOM_THEME_GROUP_KEYS: &[&str] = &[
    "app.appearance.custom_themes",
    "app.appearance.custom_themes_description",
];
/// Seed labels, mirroring `CustomColorSlot::ALL`. The view module is private,
/// so a new seed slot must also land here (the type itself has no other
/// caller of `label_key`).
const CUSTOM_COLOR_KEYS: &[&str] = &[
    "app.appearance.custom_color_background",
    "app.appearance.custom_color_foreground",
    "app.appearance.custom_color_accent",
    "app.appearance.custom_color_secondary",
    "app.appearance.custom_color_success",
    "app.appearance.custom_color_danger",
    "app.appearance.custom_color_warning",
];

/// The custom-theme editor and list render entirely through `t!` keys, so a
/// missing locale entry does not fall back to English text: `rust-i18n` hands
/// the raw key back and the surface shows `app.appearance.custom_…`. The keys
/// live in `app.appearance.yml`, which owns the `app.appearance` subtree (a
/// `app.settings.appearance` spelling silently misses the whole group).
#[test]
fn custom_theme_copy_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for &key in CUSTOM_THEME_LABEL_KEYS
            .iter()
            .chain(CUSTOM_THEME_GROUP_KEYS)
            .chain(CUSTOM_COLOR_KEYS)
        {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            assert_ne!(value, key, "{locale}/{key} renders the unresolved key");
        }
        // Group copy is the translation marker: seed labels legitimately read
        // "Accent"/"Text" in several locales, so only this prose proves the
        // block was actually localized rather than copied from English.
        for key in CUSTOM_THEME_GROUP_KEYS {
            let value = rust_i18n::t!(*key, locale = locale).to_string();
            if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(*key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        // The contrast captions are only rendered with the ratio interpolated,
        // so pin that form rather than the raw template.
        for key in CUSTOM_THEME_RATIO_KEYS {
            let value = rust_i18n::t!(*key, locale = locale, ratio = "7.0").to_string();
            assert!(value.contains("7.0"), "{locale}/{key} keeps the ratio argument");
            assert_ne!(value, *key, "{locale}/{key} renders the unresolved key");
            if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(*key, locale = "en", ratio = "7.0").to_string(), "{locale}/{key}");
            }
        }
    }
}

#[test]
fn translucent_sidebar_label_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let value =
            rust_i18n::t!("app.settings.appearance.translucent_sidebar", locale = language.locale()).to_string();
        assert!(!value.is_empty());
        if language != UiLanguage::English {
            assert_ne!(value, "Translucent sidebar", "{}", language.locale());
        }
    }
}

#[test]
fn appearance_and_cli_sync_copy_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.appearance.status_bar",
            "app.settings.appearance.reduce_motion",
            "app.settings.appearance.reduce_motion_description",
            "app.settings.cli_sync.group",
            "app.settings.cli_sync.sync_theme",
            "app.settings.cli_sync.reload_settings",
            "app.settings.cli_sync.use_theme",
            "app.settings.cli_sync.following",
            "app.settings.cli_sync.desktop_override",
            "app.settings.cli_sync.disconnected",
            "app.settings.cli_sync.theme_unavailable",
            "app.settings.cli_sync.saving_theme",
            "app.settings.cli_sync.reload_blocked",
            "app.settings.cli_sync.footer",
            "app.settings.cli_sync.reloading",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        let status = rust_i18n::t!(
            "app.settings.cli_sync.theme_status",
            locale = locale,
            theme = "demo-theme",
            mode = "demo-mode"
        )
        .to_string();
        assert!(status.contains("demo-theme"), "{locale}");
        assert!(status.contains("demo-mode"), "{locale}");
    }
}
