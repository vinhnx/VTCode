use super::UiLanguage;

#[test]
fn workspace_flow_copy_is_localized_for_every_language() {
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.workspace.unsent_guard",
            "app.workspace.busy_guard",
            "app.workspace.picker_closed",
            "app.workspace.workspace_ready",
            "app.workspace.invalid_folder",
            "app.workspace.shutting_down",
            "app.workspace.queue_full",
            "app.workspace.connect_failed",
            "app.workspace.starting_service",
            "app.workspace.startup_hint",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if IDENTICAL_TO_ENGLISH.contains(&(locale, key)) {
                assert_eq!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            } else if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        let failed = rust_i18n::t!("app.workspace.picker_failed", locale = locale, error = "demo-error").to_string();
        assert!(failed.contains("demo-error"), "{locale}: {failed}");
    }
}
