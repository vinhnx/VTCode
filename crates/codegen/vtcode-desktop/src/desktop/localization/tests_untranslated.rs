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
