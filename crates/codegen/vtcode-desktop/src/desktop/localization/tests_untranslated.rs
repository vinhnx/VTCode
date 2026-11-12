//! Resolution pins for English-literal copy: keys that stay English in
//! every locale until full translations ship. Translated keys belong in
//! the per-namespace `assert_ne!` lists instead; move these there (and
//! delete this file) once translators cover them.

use super::UiLanguage;

#[test]
fn discard_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: every locale
    // carries the English text, so this pins resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.discard_title",
            "app.settings.discard_detail",
            "app.configuration.discard_title",
            "app.files.discard_title",
            "app.files.discard_detail",
            "app.navigation.discard",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
        let detail = rust_i18n::t!("app.configuration.discard_detail", locale = locale, path = "demo.path").to_string();
        assert!(detail.contains("demo.path"), "{locale} dropped the path");
    }
    assert_eq!(rust_i18n::t!("app.navigation.discard", locale = "en").to_string(), "Discard");
}

#[test]
fn login_items_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.login_items.title",
            "app.settings.login_items.description",
            "app.settings.login_items.approval_title",
            "app.settings.login_items.approval_hint",
            "app.settings.login_items.open_settings",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
        let failure =
            rust_i18n::t!("app.settings.login_items.unavailable", locale = locale, error = "EACCES").to_string();
        assert!(failure.contains("EACCES"), "{locale} dropped the error");
    }
    assert_eq!(rust_i18n::t!("app.settings.login_items.title", locale = "en").to_string(), "Launch at login");
}

#[test]
fn swipe_navigation_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in ["app.navigation.swipe_back_to", "app.navigation.swipe_forward_to"] {
            let text = rust_i18n::t!(key, locale = locale, thread = "Demo thread").to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
            assert!(text.contains("Demo thread"), "{locale}/{key} dropped the thread name");
        }
    }
    assert_eq!(
        rust_i18n::t!("app.navigation.swipe_back_to", locale = "en", thread = "Demo").to_string(),
        "Back to Demo"
    );
    assert_eq!(
        rust_i18n::t!("app.navigation.swipe_forward_to", locale = "en", thread = "Demo").to_string(),
        "Forward to Demo"
    );
}

#[test]
fn update_install_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: every locale
    // carries the English text, so this pins resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.about.install_update",
            "app.settings.about.checking",
            "app.settings.about.verifying",
            "app.settings.about.extracting",
            "app.settings.about.replacing",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
        let progress = rust_i18n::t!(
            "app.settings.about.download_progress",
            locale = locale,
            downloaded = "1.0 MB",
            total = "2.0 MB"
        )
        .to_string();
        assert!(progress.contains("1.0 MB") && progress.contains("2.0 MB"), "{locale} dropped byte counts");
        let unknown =
            rust_i18n::t!("app.settings.about.download_progress_unknown_total", locale = locale, downloaded = "1.0 MB")
                .to_string();
        assert!(unknown.contains("1.0 MB"), "{locale} dropped the byte count");
        let installed = rust_i18n::t!("app.settings.about.installed", locale = locale).to_string();
        assert!(!installed.is_empty(), "{locale}/installed");
        let gated = rust_i18n::t!(
            "app.settings.about.installed_gatekeeper",
            locale = locale,
            done = "Update installed (0.181.8)"
        )
        .to_string();
        assert!(gated.contains("Open Anyway"), "{locale} dropped the approval step");
        let failed = rust_i18n::t!("app.settings.about.install_failed", locale = locale, message = "boom").to_string();
        assert!(failed.contains("boom"), "{locale} dropped the failure message");
    }
    assert_eq!(rust_i18n::t!("app.settings.about.install_update", locale = "en").to_string(), "Install update");
}

#[test]
fn terminal_colors_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.appearance.terminal_colors",
            "app.settings.appearance.terminal_colors_description",
            "app.settings.appearance.terminal_colors_auto",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(
        rust_i18n::t!("app.settings.appearance.terminal_colors", locale = "en").to_string(),
        "Terminal colors"
    );
}
