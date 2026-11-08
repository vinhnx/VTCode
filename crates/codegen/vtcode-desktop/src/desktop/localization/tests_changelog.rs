use super::UiLanguage;

#[test]
fn changelog_copy_resolves_in_english_until_translated() {
    // En-only by design (thread-menu precedent): other locales fall back to
    // English until native-quality translations ship.
    assert_eq!(
        rust_i18n::t!("app.changelog.title", locale = "en", version = "0.182.0").to_string(),
        "What's new in v0.182.0"
    );
    assert_eq!(
        rust_i18n::t!("app.changelog.subtitle", locale = "en", current = "0.181.0", latest = "0.182.0").to_string(),
        "VT Code 0.181.0 → 0.182.0"
    );
    assert_eq!(
        rust_i18n::t!("app.changelog.empty", locale = "en").to_string(),
        "Highlights aren't available for this release."
    );
    assert_eq!(rust_i18n::t!("app.changelog.view_full_notes", locale = "en").to_string(), "View full release notes");
    assert_eq!(rust_i18n::t!("app.changelog.done", locale = "en").to_string(), "Done");
    assert_eq!(
        rust_i18n::t!("app.changelog.banner_title", locale = "en", version = "0.182.0").to_string(),
        "What's new in v0.182.0"
    );
    assert_eq!(
        rust_i18n::t!("app.changelog.banner_more", locale = "en", summary = "Faster startup", count = 2).to_string(),
        "Faster startup (+2 more)"
    );
    assert_eq!(rust_i18n::t!("app.changelog.banner_view", locale = "en").to_string(), "View");
    assert_eq!(rust_i18n::t!("app.changelog.banner_dismiss", locale = "en").to_string(), "Dismiss release notes");
    assert_eq!(
        rust_i18n::t!("app.settings.notifications.view_changelog", locale = "en").to_string(),
        "View changelog"
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.changelog.title",
            "app.changelog.subtitle",
            "app.changelog.empty",
            "app.changelog.view_full_notes",
            "app.changelog.done",
            "app.changelog.banner_title",
            "app.changelog.banner_more",
            "app.changelog.banner_view",
            "app.changelog.banner_dismiss",
            "app.settings.notifications.view_changelog",
        ] {
            let value = rust_i18n::t!(
                key,
                locale = locale,
                version = "0.182.0",
                current = "0.0.0",
                latest = "0.0.0",
                summary = "Summary",
                count = 1
            )
            .to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            // Untranslated keys fall back to English verbatim (interpolated
            // where the key carries placeholders).
            let english = rust_i18n::t!(
                key,
                locale = "en",
                version = "0.182.0",
                current = "0.0.0",
                latest = "0.0.0",
                summary = "Summary",
                count = 1
            )
            .to_string();
            assert_eq!(value, english, "{locale}/{key} must fall back to English");
        }
    }
}
