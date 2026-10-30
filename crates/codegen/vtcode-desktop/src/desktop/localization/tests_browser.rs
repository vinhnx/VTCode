use super::UiLanguage;

/// Browser copy resolves in English and stays non-empty everywhere else
/// through the documented English fallback until translations land.
#[test]
fn browser_copy_resolves_in_english_and_falls_back_elsewhere() {
    const KEYS: [(&str, &str); 4] = [
        ("app.browser.browse_threads", "Browse threads"),
        ("app.browser.empty_title", "No threads yet"),
        ("app.browser.empty_description", "Threads you start in any workspace will appear here for review."),
        ("app.browser.close", "Close"),
    ];
    for (key, english) in KEYS {
        assert_eq!(rust_i18n::t!(key, locale = "en").to_string(), english);
    }
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for (key, _) in KEYS {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key} is empty");
        }
    }
}
