use super::UiLanguage;

#[test]
fn webmcp_copy_is_localized_for_every_language() {
    // Correct translations that are byte-identical to English. Each entry is
    // asserted equal (not skipped) so a stale entry fails instead of hiding.
    // "WebSocket URL" is a technical term kept verbatim in these locales.
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[
        ("ja", "app.webmcp.ws_url"),
        ("ko", "app.webmcp.ws_url"),
        ("hi", "app.webmcp.ws_url"),
    ];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.webmcp.connect_usage",
            "app.webmcp.attachments_error",
            "app.webmcp.pairing_title",
            "app.webmcp.status_title",
            "app.webmcp.pair_app",
            "app.webmcp.connection_title",
            "app.webmcp.pair_description",
            "app.webmcp.pair_browser",
            "app.webmcp.ws_url",
            "app.webmcp.onetime_code",
            "app.webmcp.connected_prefix",
            "app.webmcp.connected_detail_checking",
            "app.webmcp.connected_detail_unavailable",
            "app.webmcp.connected_detail_ready",
            "app.webmcp.connected_detail_busy",
            "app.webmcp.connected_detail_select",
            "app.webmcp.not_connected",
            "app.webmcp.checking_state",
            "app.webmcp.state_unavailable",
            "app.webmcp.shares_description",
            "app.webmcp.refresh_status",
            "app.webmcp.origin_aria",
            "app.webmcp.create_code",
            "app.webmcp.open_app",
            "app.webmcp.disconnect",
            "app.webmcp.pairing_instructions",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if IDENTICAL_TO_ENGLISH.contains(&(locale, key)) {
                assert_eq!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            } else if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        // Interpolation uses distinct values per variable so a swapped or
        // dropped variable fails instead of passing on identical markers.
        let expires =
            rust_i18n::t!("app.webmcp.pairing_expires", locale = locale, origin = "demo-origin", secs = 42).to_string();
        assert!(expires.contains("demo-origin"), "{locale}: {expires}");
        assert!(expires.contains("42"), "{locale}: {expires}");
        let copy = rust_i18n::t!("app.webmcp.copy_value", locale = locale, label = "demo-label").to_string();
        assert!(copy.contains("demo-label"), "{locale}: {copy}");
    }
}
