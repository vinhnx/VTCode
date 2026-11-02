use super::UiLanguage;

#[test]
fn service_copy_is_localized_for_every_language() {
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.service.runtime_init",
            "app.service.transport_thread",
            "app.service.no_proto_out",
            "app.service.no_proto_in",
            "app.service.backlog_full",
            "app.service.id_exhausted",
            "app.service.not_accepting",
            "app.service.malformed",
            "app.service.bad_frame",
            "app.service.cannot_start",
            "app.service.no_binary",
            "app.service.no_parent",
            "app.service.missing_helper",
            "app.service.missing_dev_backend",
            "app.service.request_rejected",
            "app.service.disconnected",
            "app.service.startup_wait",
            "app.service.startup_question",
            "app.service.decision_rejected",
            "app.service.request_failed",
            "app.service.review_invalid",
            "app.service.connected_service",
            "app.service.invalid_init",
            "app.service.init_failed",
            "app.service.invalid_catalog",
            "app.service.invalid_resource_catalog",
            "app.service.runtime_updated",
            "app.service.invalid_runtime",
            "app.service.theme_saved",
            "app.service.unsupported_theme",
            "app.service.theme_failed",
            "app.service.invalid_theme",
            "app.service.invalid_session_list",
            "app.service.session_ready",
            "app.service.creation_failed",
            "app.service.invalid_ack",
            "app.service.invalid_ack_detail",
            "app.service.running",
            "app.service.completed_status",
            "app.service.cancel_accepted",
            "app.service.empty_history",
            "app.service.awaiting_decision",
            "app.service.credentials_required",
            "app.service.exited",
            "app.service.review_no_start",
            "app.service.review_exited",
            "app.service.invalid_summary",
            "app.service.invalid_delete",
            "app.service.invalid_worktrees",
            "app.service.invalid_remove",
            "app.service.invalid_updates",
            "app.service.review_disconnected",
            "app.service.interaction_cancelled",
            "app.service.decision_accepted",
            "app.service.invalid_slash_catalog",
            "app.service.invalid_session",
            "app.service.reconnect_with_draft",
            "app.service.reconnect_plain",
            "app.service.authentication_failed",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if IDENTICAL_TO_ENGLISH.contains(&(locale, key)) {
                assert_eq!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            } else if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
        let switched = rust_i18n::t!("app.service.switched_model", locale = locale, model = "demo-model").to_string();
        assert!(switched.contains("demo-model"), "{locale}: {switched}");
        let exited = rust_i18n::t!("app.service.exited_code", locale = locale, code = 3).to_string();
        assert!(exited.contains('3'), "{locale}: {exited}");
        let detail =
            rust_i18n::t!("app.service.disconnected_detail", locale = locale, detail = "demo-detail").to_string();
        assert!(detail.contains("demo-detail"), "{locale}: {detail}");
        let auth =
            rust_i18n::t!("app.service.authentication_failed", locale = locale, detail = "demo-detail").to_string();
        assert!(auth.contains("demo-detail"), "{locale}: {auth}");
        assert!(auth.contains("vtcode auth"), "{locale}: {auth}");
    }
}
