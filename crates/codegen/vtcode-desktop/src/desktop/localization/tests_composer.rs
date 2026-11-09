use super::UiLanguage;

#[test]
fn composer_copy_is_localized_for_every_language() {
    // Correct translations that are byte-identical to English. Each entry is
    // asserted equal (not skipped) so a stale entry fails instead of hiding.
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[("fr", "app.composer.attachment_image")];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.composer.message_not_submitted",
            "app.composer.delivery_unconfirmed",
            "app.composer.draft_not_submitted",
            "app.composer.clear_draft_hint",
            "app.composer.restore_draft",
            "app.composer.no_turn_to_stop",
            "app.composer.no_thread_to_close",
            "app.composer.mode_switch_busy",
            "app.composer.choose_workspace_files",
            "app.composer.palette_group_files",
            "app.composer.palette_group_commands",
            "app.composer.attachments_limit",
            "app.composer.attachment_image",
            "app.composer.picker_this_computer",
            "app.composer.view_attachment",
            "app.composer.open_attachment",
            "app.composer.palette_group_effort",
            "app.composer.attach_files",
            "app.composer.choose_agent_mode",
            "app.composer.permission_restore_hint",
            "app.composer.creating_session",
            "app.composer.submitting_turn",
            "app.composer.draft_restored",
            "app.composer.cancelling_turn",
            "app.composer.cancellation_queued",
            "app.composer.cancellation_not_queued",
            "app.composer.answers_too_long",
            "app.composer.sending_decision",
            "app.composer.decision_not_sent",
            "app.composer.choose_option",
            "app.composer.choose_all_options",
            "app.composer.copy_prompt",
            "app.composer.theme_unavailable",
            "app.composer.use_effort",
            "app.composer.agent_mode_aria",
            "app.composer.speed_aria",
            "app.composer.effort_aria",
            "app.composer.attached_list",
            "app.composer.prompt_too_long",
            "app.composer.dismiss",
            "app.composer.queued_count",
            "app.composer.edit_queued",
            "app.composer.queue_dropped",
            "app.composer.queue_wait_time",
            "app.composer.remove_queued",
            "app.composer.queued_pending",
            "app.composer.background_operation",
            "app.composer.background_operation_tooltip",
            "app.composer.force_cancel",
            "app.composer.force_cancel_tooltip",
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
        let attachment = rust_i18n::t!("app.composer.attachment_file", locale = locale, bytes = 42).to_string();
        assert!(attachment.contains("42"), "{locale}: {attachment}");
        let remove = rust_i18n::t!("app.composer.remove_attachment", locale = locale, path = "demo/path").to_string();
        assert!(remove.contains("demo/path"), "{locale}: {remove}");
        let limit = rust_i18n::t!("app.composer.attachments_limit", locale = locale, n = 5).to_string();
        assert!(limit.contains('5'), "{locale}: {limit}");
        let theme = rust_i18n::t!("app.composer.theme_unavailable", locale = locale, theme = "demo-theme").to_string();
        assert!(theme.contains("demo-theme"), "{locale}: {theme}");
        let effort = rust_i18n::t!("app.composer.use_effort", locale = locale, effort = "demo-effort").to_string();
        assert!(effort.contains("demo-effort"), "{locale}: {effort}");
        let mode = rust_i18n::t!("app.composer.agent_mode_aria", locale = locale, mode = "demo-mode").to_string();
        assert!(mode.contains("demo-mode"), "{locale}: {mode}");
        let speed = rust_i18n::t!("app.composer.speed_aria", locale = locale, label = "demo-label").to_string();
        assert!(speed.contains("demo-label"), "{locale}: {speed}");
        let reasoning = rust_i18n::t!("app.composer.effort_aria", locale = locale, effort = "demo-effort").to_string();
        assert!(reasoning.contains("demo-effort"), "{locale}: {reasoning}");
        let attached = rust_i18n::t!("app.composer.attached_list", locale = locale, paths = "a, b").to_string();
        assert!(attached.contains("a, b"), "{locale}: {attached}");
        let queued = rust_i18n::t!("app.composer.queued_count", locale = locale, count = 3).to_string();
        assert!(queued.contains('3'), "{locale}: {queued}");
        let dropped = rust_i18n::t!("app.composer.queue_dropped", locale = locale, dropped = 7).to_string();
        assert!(dropped.contains('7'), "{locale}: {dropped}");
    }
}
