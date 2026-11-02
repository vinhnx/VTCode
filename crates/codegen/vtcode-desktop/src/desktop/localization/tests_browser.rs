use super::UiLanguage;

/// Every browser key resolves in English and ships a real translation in
/// every other locale (no English fallback, no empty copy).
#[test]
fn browser_copy_is_translated_in_every_locale() {
    const KEYS: [(&str, &str); 4] = [
        ("app.browser.browse_threads", "Browse threads"),
        ("app.browser.empty_title", "No threads yet"),
        ("app.browser.empty_description", "New threads in any workspace will appear here for review."),
        ("app.browser.close", "Close"),
    ];
    for (key, english) in KEYS {
        assert_eq!(rust_i18n::t!(key, locale = "en").to_string(), english);
    }
    for language in UiLanguage::ALL {
        let locale = language.locale();
        if locale == "en" {
            continue;
        }
        for (key, english) in KEYS {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key} is empty");
            assert_ne!(text, english, "{locale}/{key} falls back to English");
        }
    }
}
