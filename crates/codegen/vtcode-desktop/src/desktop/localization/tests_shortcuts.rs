use super::UiLanguage;

#[test]
fn shortcuts_copy_is_localized_for_every_language() {
    // Correct translations that are byte-identical to English. Each entry is
    // asserted equal (not skipped) so a stale entry fails instead of hiding.
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.shortcuts.commands_label",
            "app.shortcuts.commands_description",
            "app.shortcuts.conversation_search_label",
            "app.shortcuts.conversation_search_description",
            "app.shortcuts.new_session_label",
            "app.shortcuts.new_session_description",
            "app.shortcuts.close_session_label",
            "app.shortcuts.close_session_description",
            "app.shortcuts.workspace_label",
            "app.shortcuts.workspace_description",
            "app.shortcuts.composer_label",
            "app.shortcuts.composer_description",
            "app.shortcuts.sidebar_label",
            "app.shortcuts.sidebar_description",
            "app.shortcuts.inspector_label",
            "app.shortcuts.inspector_description",
            "app.shortcuts.zen_mode_label",
            "app.shortcuts.zen_mode_description",
            "app.shortcuts.cancel_label",
            "app.shortcuts.cancel_description",
            "app.shortcuts.dismiss_label",
            "app.shortcuts.dismiss_description",
            "app.shortcuts.settings_label",
            "app.shortcuts.settings_description",
            "app.shortcuts.settings_search_label",
            "app.shortcuts.settings_search_description",
            "app.shortcuts.model_label",
            "app.shortcuts.model_description",
            "app.shortcuts.permissions_label",
            "app.shortcuts.permissions_description",
            "app.shortcuts.agent_mode_label",
            "app.shortcuts.agent_mode_description",
            "app.shortcuts.add_context_label",
            "app.shortcuts.add_context_description",
            "app.shortcuts.status_bar_label",
            "app.shortcuts.status_bar_description",
            "app.shortcuts.reload_cli_settings_label",
            "app.shortcuts.reload_cli_settings_description",
            "app.shortcuts.sync_cli_theme_label",
            "app.shortcuts.sync_cli_theme_description",
            "app.shortcuts.utilities_label",
            "app.shortcuts.utilities_description",
            "app.shortcuts.workspace_files_label",
            "app.shortcuts.workspace_files_description",
            "app.shortcuts.workspace_review_label",
            "app.shortcuts.workspace_review_description",
            "app.shortcuts.refresh_workspace_label",
            "app.shortcuts.refresh_workspace_description",
            "app.shortcuts.navigate_back_label",
            "app.shortcuts.navigate_back_description",
            "app.shortcuts.navigate_forward_label",
            "app.shortcuts.navigate_forward_description",
            "app.shortcuts.next_review_change_label",
            "app.shortcuts.next_review_change_description",
            "app.shortcuts.previous_review_change_label",
            "app.shortcuts.previous_review_change_description",
            "app.shortcuts.next_review_file_label",
            "app.shortcuts.next_review_file_description",
            "app.shortcuts.previous_review_file_label",
            "app.shortcuts.previous_review_file_description",
            "app.shortcuts.queue_batched_label",
            "app.shortcuts.queue_batched_description",
            "app.shortcuts.prompt_history_label",
            "app.shortcuts.prompt_history_description",
            "app.shortcuts.edit_queue_label",
            "app.shortcuts.edit_queue_description",
            "app.shortcuts.unsaved_description",
            "app.shortcuts.browse_description",
            "app.shortcuts.save",
            "app.shortcuts.reset_all",
            "app.shortcuts.import_cli",
            "app.shortcuts.group_label",
            "app.shortcuts.search_aria",
            "app.shortcuts.footer_hint",
            "app.shortcuts.saved",
            "app.shortcuts.already_default",
            "app.shortcuts.staged_defaults",
            "app.shortcuts.press_keys_hint",
            "app.shortcuts.badge_unassigned",
            "app.shortcuts.badge_invalid",
            "app.shortcuts.no_cli_overrides",
            "app.shortcuts.cli_imported",
            "app.shortcuts.unknown_action",
            "app.shortcuts.a11y_not_set",
            "app.shortcuts.a11y_invalid",
            "app.shortcuts.invalid_shortcut",
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
        let no_match = rust_i18n::t!("app.shortcuts.no_match", locale = locale, query = "demo-query").to_string();
        assert!(no_match.contains("demo-query"), "{locale}: {no_match}");
        let press = rust_i18n::t!("app.shortcuts.press_keys", locale = locale, label = "demo-label").to_string();
        assert!(press.contains("demo-label"), "{locale}: {press}");
        let edit = rust_i18n::t!("app.shortcuts.edit_for", locale = locale, label = "demo-label").to_string();
        assert!(edit.contains("demo-label"), "{locale}: {edit}");
        let remove = rust_i18n::t!("app.shortcuts.remove_for", locale = locale, label = "demo-label").to_string();
        assert!(remove.contains("demo-label"), "{locale}: {remove}");
        let row = rust_i18n::t!("app.shortcuts.row_aria", locale = locale, label = "demo-label", value = "demo-value")
            .to_string();
        assert!(row.contains("demo-label"), "{locale}: {row}");
        assert!(row.contains("demo-value"), "{locale}: {row}");
        let badge =
            rust_i18n::t!("app.shortcuts.badge_aria", locale = locale, label = "demo-label", value = "demo-value")
                .to_string();
        assert!(badge.contains("demo-label"), "{locale}: {badge}");
        assert!(badge.contains("demo-value"), "{locale}: {badge}");
        let imported =
            rust_i18n::t!("app.shortcuts.cli_imported_warnings", locale = locale, warnings = "demo-warnings")
                .to_string();
        assert!(imported.contains("demo-warnings"), "{locale}: {imported}");
        let enter = rust_i18n::t!("app.shortcuts.enter_one_combo", locale = locale, label = "demo-label").to_string();
        assert!(enter.contains("demo-label"), "{locale}: {enter}");
        let invalid =
            rust_i18n::t!("app.shortcuts.invalid_combo", locale = locale, label = "demo-label", error = "demo-error")
                .to_string();
        assert!(invalid.contains("demo-label"), "{locale}: {invalid}");
        assert!(invalid.contains("demo-error"), "{locale}: {invalid}");
        let binding = rust_i18n::t!(
            "app.shortcuts.invalid_shortcut",
            locale = locale,
            label = "demo-label",
            error = "demo-error"
        )
        .to_string();
        assert!(binding.contains("demo-label"), "{locale}: {binding}");
        assert!(binding.contains("demo-error"), "{locale}: {binding}");
        let reserved =
            rust_i18n::t!("app.shortcuts.reserved_shortcut", locale = locale, label = "demo-label").to_string();
        assert!(reserved.contains("demo-label"), "{locale}: {reserved}");
        let modifier =
            rust_i18n::t!("app.shortcuts.include_modifier", locale = locale, label = "demo-label").to_string();
        assert!(modifier.contains("demo-label"), "{locale}: {modifier}");
        let assigned =
            rust_i18n::t!("app.shortcuts.already_assigned", locale = locale, label = "demo-label").to_string();
        assert!(assigned.contains("demo-label"), "{locale}: {assigned}");
    }
}
