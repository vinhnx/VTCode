use super::UiLanguage;

#[test]
fn session_copy_is_localized_for_every_language() {
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[
        ("fr", "app.session.fallback_title"),
        ("sv", "app.session.fallback_title"),
    ];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.session.updating_session",
            "app.session.thread_ready",
            "app.session.load_more_failed",
            "app.session.restoring",
            "app.session.load_failed",
            "app.session.invalid_load_result",
            "app.session.transcript_limit",
            "app.session.invalid_history_page",
            "app.session.wrong_thread",
            "app.session.wrong_session_retained",
            "app.session.loaded_partial",
            "app.session.loaded_prompt",
            "app.session.switching_model",
            "app.session.fallback_title",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if IDENTICAL_TO_ENGLISH.contains(&(locale, key)) {
                assert_eq!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            } else if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        let fallback = rust_i18n::t!("app.session.fallback_title", locale = locale, id = "demo-id").to_string();
        assert!(fallback.contains("demo-id"), "{locale}: {fallback}");
    }
}
