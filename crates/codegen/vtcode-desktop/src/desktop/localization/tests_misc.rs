use super::UiLanguage;

/// Every key resolves to a non-empty translation in all 20 locales, and every
/// non-English translation differs from English unless it is listed in
/// `identical` (correct cognates asserted equal so stale entries fail).
fn assert_localized(keys: &[&str], identical: &[(&str, &str)]) {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in keys {
            let key: &str = key;
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if identical.contains(&(locale, key)) {
                assert_eq!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            } else if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
    }
}

#[test]
fn secrets_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.secrets.panel_title",
            "app.secrets.provider_aria",
            "app.secrets.saving",
            "app.secrets.save_key",
            "app.secrets.empty_key",
            "app.secrets.saving_status",
            "app.secrets.invalid_response",
            "app.secrets.saved",
            "app.secrets.saved_send",
            "app.secrets.saved_missing",
            "app.secrets.not_saved",
            "app.secrets.disconnected_while_saving",
            "app.secrets.paste_key",
        ],
        &[],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let stored = rust_i18n::t!("app.secrets.stored_as", locale = locale, var = "DEMO_KEY").to_string();
        assert!(stored.contains("DEMO_KEY"), "{locale}: {stored}");
    }
}

#[test]
fn interaction_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.interaction.sending_aria",
            "app.interaction.progress_aria",
            "app.interaction.steps_aria",
            "app.interaction.cancel_request",
            "app.interaction.previous",
            "app.interaction.submit_answers",
            "app.interaction.next_question",
            "app.interaction.your_answer",
            "app.interaction.recommended",
            "app.interaction.selected_label",
        ],
        &[],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let recommended =
            rust_i18n::t!("app.interaction.recommended", locale = locale, label = "demo-label").to_string();
        assert!(recommended.contains("demo-label"), "{locale}: {recommended}");
        let selected =
            rust_i18n::t!("app.interaction.selected_label", locale = locale, label = "demo-label").to_string();
        assert!(selected.contains("demo-label"), "{locale}: {selected}");
        let step = rust_i18n::t!("app.interaction.step_fallback", locale = locale, n = 7).to_string();
        assert!(step.contains('7'), "{locale}: {step}");
        let counter = rust_i18n::t!("app.interaction.step_counter", locale = locale, n = 2, total = 9).to_string();
        assert!(counter.contains('2'), "{locale}: {counter}");
        assert!(counter.contains('9'), "{locale}: {counter}");
    }
}

#[test]
fn utilities_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.utilities.no_workspace",
            "app.utilities.group_aria",
            "app.utilities.changes_title",
            "app.utilities.counts_unavailable",
            "app.utilities.recent_activity",
            "app.utilities.draft_sources",
            "app.utilities.attach_sources",
            "app.utilities.attach_tooltip",
            "app.utilities.attach_hint",
            "app.utilities.browse_files",
        ],
        &[],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let unavailable = rust_i18n::t!("app.utilities.changes_unavailable", locale = locale, n = 3).to_string();
        assert!(unavailable.contains('3'), "{locale}: {unavailable}");
        let totals = rust_i18n::t!("app.utilities.changes_totals", locale = locale, n = 3, a = 11, d = 5).to_string();
        for marker in ["3", "11", "5"] {
            assert!(totals.contains(marker), "{locale}: {totals}");
        }
        let summary =
            rust_i18n::t!("app.utilities.recent_summary", locale = locale, summary = "demo-summary").to_string();
        assert!(summary.contains("demo-summary"), "{locale}: {summary}");
        let open = rust_i18n::t!("app.utilities.open_source", locale = locale, path = "demo/path").to_string();
        assert!(open.contains("demo/path"), "{locale}: {open}");
    }
}

#[test]
fn notifications_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.notifications.copied",
            "app.notifications.agent_done",
            "app.notifications.agent_attention",
            "app.notifications.approval_waiting",
            "app.notifications.input_waiting",
            "app.notifications.service_disconnected",
            "app.notifications.update_available",
        ],
        &[],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let update =
            rust_i18n::t!("app.notifications.update_available", locale = locale, latest = "v9.9.9").to_string();
        assert!(update.contains("v9.9.9"), "{locale}: {update}");
    }
}

#[test]
fn markdown_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.markdown.render_error",
            "app.markdown.copy_code",
            "app.markdown.images_blocked",
        ],
        &[],
    );
}

#[test]
fn preview_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.preview.queue_full",
            "app.preview.load_failed",
            "app.preview.title",
            "app.preview.retry",
            "app.preview.loading",
            "app.preview.not_loaded",
            "app.preview.static_notice",
            "app.preview.no_text",
            "app.preview.truncated",
        ],
        &[],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let retry = rust_i18n::t!("app.preview.retry_for", locale = locale, uri = "demo-uri").to_string();
        assert!(retry.contains("demo-uri"), "{locale}: {retry}");
        let aria = rust_i18n::t!("app.preview.static_aria", locale = locale, uri = "demo-uri").to_string();
        assert!(aria.contains("demo-uri"), "{locale}: {aria}");
    }
}

#[test]
fn files_errors_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.files.missing",
            "app.files.not_regular",
            "app.files.no_parent",
            "app.files.open_failed",
            "app.files.attachments_workspace",
            "app.files.picker_disconnected",
            "app.files.picker_failed",
            "app.errors.preferences_unavailable",
            "app.errors.preferences_save_failed",
        ],
        &[],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let launch = rust_i18n::t!("app.files.launch_failed", locale = locale, error = "demo-error").to_string();
        assert!(launch.contains("demo-error"), "{locale}: {launch}");
        let picker = rust_i18n::t!("app.files.picker_failed", locale = locale, error = "demo-error").to_string();
        assert!(picker.contains("demo-error"), "{locale}: {picker}");
        let prefs =
            rust_i18n::t!("app.errors.preferences_save_failed", locale = locale, error = "demo-error").to_string();
        assert!(prefs.contains("demo-error"), "{locale}: {prefs}");
        let detail = rust_i18n::t!("app.files.open_failed_detail", locale = locale, detail = "demo-detail").to_string();
        assert!(detail.contains("demo-detail"), "{locale}: {detail}");
    }
}

#[test]
fn appearance_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.appearance.motion_system",
            "app.appearance.motion_on",
            "app.appearance.motion_off",
            "app.appearance.selected_suffix",
            "app.appearance.reread_system",
            "app.appearance.reduce_animations",
            "app.appearance.allow_animations",
            "app.appearance.code_font_size",
            "app.appearance.code_font_desc",
            "app.appearance.ui_font_size",
            "app.appearance.ui_font_desc",
            "app.appearance.system_default",
            "app.appearance.system_pro",
            "app.appearance.system_short",
            "app.appearance.ui_font",
            "app.appearance.code_font",
            "app.appearance.installed_fonts",
            "app.appearance.branch_label",
            "app.appearance.sandbox_label",
            "app.appearance.context_label",
            "app.appearance.decrease",
            "app.appearance.increase",
            "app.appearance.size_pixels",
            "app.appearance.choose_current",
        ],
        &[
            ("de", "app.appearance.branch_label"),
            ("pt-BR", "app.appearance.branch_label"),
            ("nl", "app.appearance.branch_label"),
            ("es", "app.appearance.sandbox_label"),
            ("de", "app.appearance.sandbox_label"),
            ("it", "app.appearance.sandbox_label"),
            ("pt-BR", "app.appearance.sandbox_label"),
            ("nl", "app.appearance.sandbox_label"),
            ("id", "app.appearance.sandbox_label"),
            ("nl", "app.appearance.context_label"),
            ("pt-BR", "app.appearance.size_pixels"),
            ("nl", "app.appearance.size_pixels"),
            ("de", "app.appearance.motion_system"),
            ("sv", "app.appearance.motion_system"),
            ("pl", "app.appearance.motion_system"),
            ("it", "app.appearance.motion_on"),
            ("it", "app.appearance.motion_off"),
            ("de", "app.appearance.system_pro"),
            ("sv", "app.appearance.system_pro"),
            ("pl", "app.appearance.system_pro"),
            ("de", "app.appearance.system_short"),
            ("sv", "app.appearance.system_short"),
            ("pl", "app.appearance.system_short"),
        ],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let aria =
            rust_i18n::t!("app.appearance.motion_aria", locale = locale, label = "demo-label", suffix = "demo-suffix")
                .to_string();
        assert!(aria.contains("demo-label"), "{locale}: {aria}");
        assert!(aria.contains("demo-suffix"), "{locale}: {aria}");
        let font = rust_i18n::t!("app.appearance.system_default_font", locale = locale, font = "demo-font").to_string();
        assert!(font.contains("demo-font"), "{locale}: {font}");
        let branch = rust_i18n::t!("app.appearance.branch_label", locale = locale, value = "demo-branch").to_string();
        assert!(branch.contains("demo-branch"), "{locale}: {branch}");
        let sandbox =
            rust_i18n::t!("app.appearance.sandbox_label", locale = locale, value = "demo-sandbox").to_string();
        assert!(sandbox.contains("demo-sandbox"), "{locale}: {sandbox}");
        let context = rust_i18n::t!("app.appearance.context_label", locale = locale, percent = 42).to_string();
        assert!(context.contains("42"), "{locale}: {context}");
        let size =
            rust_i18n::t!("app.appearance.size_pixels", locale = locale, label = "demo-label", size = 13).to_string();
        assert!(size.contains("demo-label"), "{locale}: {size}");
        assert!(size.contains("13"), "{locale}: {size}");
        let choose =
            rust_i18n::t!("app.appearance.choose_current", locale = locale, label = "demo-label", value = "demo-value")
                .to_string();
        assert!(choose.contains("demo-label"), "{locale}: {choose}");
        assert!(choose.contains("demo-value"), "{locale}: {choose}");
    }
}

#[test]
fn permissions_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.permissions.desc_ask",
            "app.permissions.desc_auto",
            "app.permissions.desc_full",
            "app.permissions.group_next_session",
            "app.permissions.group_default_agent",
            "app.permissions.updating_runtime",
            "app.permissions.confirm_full_title",
            "app.permissions.confirm_full_body",
            "app.permissions.enable_full",
            "app.permissions.save_failed",
            "app.permissions.restore_failed",
        ],
        &[],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let failed = rust_i18n::t!("app.permissions.save_failed", locale = locale, error = "demo-error").to_string();
        assert!(failed.contains("demo-error"), "{locale}: {failed}");
    }
}

#[test]
fn menus_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.menus.settings_menu",
            "app.menus.services",
            "app.menus.hide_app",
            "app.menus.hide_others",
            "app.menus.show_all",
            "app.menus.quit_app",
            "app.menus.file_menu",
            "app.menus.new_session",
            "app.menus.open_workspace",
            "app.menus.close_session",
            "app.menus.choose_model",
            "app.menus.edit_menu",
            "app.menus.undo",
            "app.menus.redo",
            "app.menus.cut",
            "app.menus.copy",
            "app.menus.paste",
            "app.menus.select_all",
            "app.menus.view_menu",
            "app.menus.command_palette",
            "app.menus.find_conversation",
            "app.menus.focus_composer",
            "app.menus.toggle_inspector",
            "app.menus.toggle_zen",
            "app.menus.cancel_run",
            "app.menus.window_menu",
            "app.menus.show_app",
            "app.menus.minimize",
            "app.menus.zoom",
            "app.menus.close_window",
        ],
        &[
            ("fr", "app.menus.services"),
            ("it", "app.menus.file_menu"),
            ("id", "app.menus.file_menu"),
            ("id", "app.menus.edit_menu"),
            ("es", "app.menus.zoom"),
            ("fr", "app.menus.zoom"),
            ("it", "app.menus.zoom"),
            ("pt-BR", "app.menus.zoom"),
            ("nl", "app.menus.zoom"),
        ],
    );
}

#[test]
fn palette_badges_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.palette.context_suffix",
            "app.palette.current_suffix",
            "app.palette.enable_processing",
            "app.palette.disable_processing",
            "app.palette.cap_reasoning",
            "app.palette.cap_vision",
            "app.palette.cap_tools",
            "app.palette.cap_structured",
            "app.palette.cap_audio",
            "app.palette.cap_video",
        ],
        &[
            ("de", "app.palette.cap_reasoning"),
            ("fr", "app.palette.cap_vision"),
            ("de", "app.palette.cap_vision"),
            ("de", "app.palette.cap_tools"),
            ("nl", "app.palette.cap_tools"),
            ("es", "app.palette.cap_audio"),
            ("fr", "app.palette.cap_audio"),
            ("de", "app.palette.cap_audio"),
            ("it", "app.palette.cap_audio"),
            ("nl", "app.palette.cap_audio"),
            ("id", "app.palette.cap_audio"),
            ("vi", "app.palette.cap_video"),
            ("de", "app.palette.cap_video"),
            ("it", "app.palette.cap_video"),
            ("nl", "app.palette.cap_video"),
            ("sv", "app.palette.cap_video"),
            ("tr", "app.palette.cap_video"),
            ("id", "app.palette.cap_video"),
        ],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let context = rust_i18n::t!("app.palette.context_suffix", locale = locale, window = "demo-window").to_string();
        assert!(context.contains("demo-window"), "{locale}: {context}");
        let enable = rust_i18n::t!("app.palette.enable_processing", locale = locale, label = "demo-label").to_string();
        assert!(enable.contains("demo-label"), "{locale}: {enable}");
        let disable =
            rust_i18n::t!("app.palette.disable_processing", locale = locale, label = "demo-label").to_string();
        assert!(disable.contains("demo-label"), "{locale}: {disable}");
    }
}

#[test]
fn settings_diagnostics_copy_is_localized_for_every_language() {
    assert_localized(
        &[
            "app.settings.licenses.locate_failed",
            "app.settings.storage.byte_one",
            "app.settings.storage.bytes_many",
        ],
        &[
            ("vi", "app.settings.storage.byte_one"),
            ("es", "app.settings.storage.byte_one"),
            ("it", "app.settings.storage.byte_one"),
            ("pt-BR", "app.settings.storage.byte_one"),
            ("nl", "app.settings.storage.byte_one"),
            ("sv", "app.settings.storage.byte_one"),
            ("es", "app.settings.storage.bytes_many"),
            ("pt-BR", "app.settings.storage.bytes_many"),
            ("nl", "app.settings.storage.bytes_many"),
        ],
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let failed =
            rust_i18n::t!("app.settings.licenses.locate_failed", locale = locale, error = "demo-error").to_string();
        assert!(failed.contains("demo-error"), "{locale}: {failed}");
        let many = rust_i18n::t!("app.settings.storage.bytes_many", locale = locale, n = 42).to_string();
        assert!(many.contains("42"), "{locale}: {many}");
    }
}

#[test]
fn workspace_review_stats_copy_is_localized_for_every_language() {
    assert_localized(&["app.workspace_review.file_stats"], &[]);
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let stats = rust_i18n::t!("app.workspace_review.file_stats", locale = locale, add = 7, del = 3).to_string();
        assert!(stats.contains('7'), "{locale}: {stats}");
        assert!(stats.contains('3'), "{locale}: {stats}");
    }
}
