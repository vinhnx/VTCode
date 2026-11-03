use super::UiLanguage;

#[test]
fn transcript_copy_is_localized_for_every_language() {
    // Correct translations that are byte-identical to English. Each entry is
    // asserted equal (not skipped) so a stale entry fails instead of hiding.
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[
        ("fr", "app.transcript.conversation_label"),
        ("es", "app.transcript.plan_title"),
        ("fr", "app.transcript.plan_title"),
        ("de", "app.transcript.plan_title"),
        ("nl", "app.transcript.plan_title"),
        ("sv", "app.transcript.plan_title"),
        ("pl", "app.transcript.plan_title"),
        ("tr", "app.transcript.plan_title"),
    ];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.transcript.choose_workspace",
            "app.transcript.welcome_label",
            "app.transcript.jump_latest",
            "app.transcript.scroll_first",
            "app.transcript.filter_next",
            "app.transcript.filter_clear",
            "app.transcript.conversation_label",
            "app.transcript.zen_start_label",
            "app.transcript.navigating_back",
            "app.transcript.navigating_forward",
            "app.transcript.navigating",
            "app.transcript.copy_message",
            "app.transcript.archived_output",
            "app.transcript.hide_details",
            "app.transcript.show_details",
            "app.transcript.copy_full_output",
            "app.transcript.patch_failed",
            "app.transcript.preview_incomplete",
            "app.transcript.applied",
            "app.transcript.hide_diff",
            "app.transcript.show_diff",
            "app.transcript.copy_patch",
            "app.transcript.previews_unavailable",
            "app.transcript.diff_truncated",
            "app.transcript.file_changes_title",
            "app.transcript.tool_output_title",
            "app.transcript.web_search",
            "app.transcript.tool_result",
            "app.transcript.turn_unverified",
            "app.transcript.run_status",
            "app.transcript.planning_update",
            "app.transcript.runtime_error",
            "app.transcript.error_title",
            "app.transcript.app_refused_title",
            "app.transcript.plan_title",
            "app.transcript.decision_title",
            "app.transcript.activity_total",
            "app.transcript.activity_running",
            "app.transcript.activity_failed",
            "app.transcript.collapse",
            "app.transcript.expand",
            "app.transcript.copy_output_for",
            "app.transcript.you",
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
        let change = rust_i18n::t!("app.transcript.change_workspace", locale = locale, name = "demo-ws").to_string();
        assert!(change.contains("demo-ws"), "{locale}: {change}");
        let details = rust_i18n::t!(
            "app.transcript.details_label",
            locale = locale,
            toggle = "demo-toggle",
            title = "demo-title",
            status = "demo-status"
        )
        .to_string();
        for marker in ["demo-toggle", "demo-title", "demo-status"] {
            assert!(details.contains(marker), "{locale}: {details}");
        }
        let via =
            rust_i18n::t!("app.transcript.web_search_via", locale = locale, provider = "demo-provider").to_string();
        assert!(via.contains("demo-provider"), "{locale}: {via}");
        let refused = rust_i18n::t!(
            "app.transcript.app_refused_body",
            locale = locale,
            tool = "demo-tool",
            tier = "demo-tier",
            uris = "demo-uris"
        )
        .to_string();
        for marker in ["demo-tool", "demo-tier", "demo-uris"] {
            assert!(refused.contains(marker), "{locale}: {refused}");
        }
        let total = rust_i18n::t!("app.transcript.activity_total", locale = locale, total = 7).to_string();
        assert!(total.contains('7'), "{locale}: {total}");
        let running = rust_i18n::t!("app.transcript.activity_running", locale = locale, count = 3).to_string();
        assert!(running.contains('3'), "{locale}: {running}");
        let failed = rust_i18n::t!("app.transcript.activity_failed", locale = locale, count = 9).to_string();
        assert!(failed.contains('9'), "{locale}: {failed}");
        let copy = rust_i18n::t!("app.transcript.copy_output_for", locale = locale, title = "demo-title").to_string();
        assert!(copy.contains("demo-title"), "{locale}: {copy}");
        let next = rust_i18n::t!("app.transcript.filter_next", locale = locale, position = 2, total = 7).to_string();
        for marker in ["2", "7"] {
            assert!(next.contains(marker), "{locale}: {next}");
        }
    }
}
