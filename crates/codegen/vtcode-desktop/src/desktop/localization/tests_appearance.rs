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
