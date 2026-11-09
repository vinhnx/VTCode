use super::UiLanguage;

#[test]
fn configuration_copy_is_localized_for_every_language() {
    // Correct translations that are byte-identical to English. Each entry is
    // asserted equal (not skipped) so a stale entry fails instead of hiding.
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[
        ("fr", "app.configuration.page_title"),
        ("nl", "app.configuration.target_project"),
        ("de", "app.configuration.format_text"),
        ("sv", "app.configuration.format_text"),
        ("id", "app.configuration.format_boolean"),
        ("id", "app.configuration.edit_target"),
    ];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.configuration.search_label",
            "app.configuration.browse_all",
            "app.configuration.browse_all_label",
            "app.configuration.reload",
            "app.configuration.reload_tooltip",
            "app.configuration.write_to",
            "app.configuration.clear_search",
            "app.configuration.search_placeholder",
            "app.configuration.title",
            "app.configuration.groups",
            "app.configuration.tagline",
            "app.configuration.open_file",
            "app.configuration.open_file_label",
            "app.configuration.edit",
            "app.configuration.cancel",
            "app.configuration.loading",
            "app.configuration.model_features",
            "app.configuration.page_title",
            "app.configuration.unset",
            "app.configuration.reference_only",
            "app.configuration.reset_hint",
            "app.configuration.edit_title",
            "app.configuration.choose_value",
            "app.configuration.saving",
            "app.configuration.save_reload",
            "app.configuration.choose_value_before_saving",
            "app.configuration.selected_suffix",
            "app.configuration.save_note",
            "app.configuration.reasoning_effort_hint",
            "app.configuration.credential_note",
            "app.configuration.omitted_large_note",
            "app.configuration.target_user",
            "app.configuration.target_workspace",
            "app.configuration.target_project",
            "app.configuration.available_efforts",
            "app.configuration.cli_configuration",
            "app.configuration.format_boolean",
            "app.configuration.format_text",
            "app.configuration.format_integer",
            "app.configuration.format_float",
            "app.configuration.format_array",
            "app.configuration.format_object",
            "app.configuration.choose_one_value",
            "app.configuration.choose_workspace",
            "app.configuration.empty_workspace_title",
            "app.configuration.empty_workspace_description",
            "app.configuration.empty_search_title",
            "app.configuration.empty_search_hint",
            "app.configuration.overview_group",
            "app.configuration.unavailable",
            "app.configuration.turn_off",
            "app.configuration.turn_on",
            "app.configuration.hide_from_models",
            "app.configuration.show_in_models",
            "app.configuration.use_cli_default",
            "app.configuration.enter_bool",
            "app.configuration.enter_integer",
            "app.configuration.enter_float",
            "app.configuration.enter_array",
            "app.configuration.enter_object",
            "app.configuration.unsupported_value",
            "app.configuration.save_disconnected",
            "app.configuration.opened_editor",
            "app.configuration.invalid_settings_response",
            "app.configuration.invalid_catalog",
            "app.configuration.choose_string_option",
            "app.configuration.saving_settings",
            "app.configuration.opening_file",
            "app.configuration.click_turn_off",
            "app.configuration.click_turn_on",
            "app.configuration.blocked_in_catalog",
            "app.configuration.row_sandboxing_title",
            "app.configuration.row_sandboxing_description",
            "app.configuration.row_approval_title",
            "app.configuration.row_approval_description",
            "app.configuration.row_sandbox_title",
            "app.configuration.row_sandbox_description",
            "app.configuration.row_network_title",
            "app.configuration.row_network_description",
            "app.configuration.row_web_search_title",
            "app.configuration.row_web_search_description",
            "app.configuration.row_verbosity_title",
            "app.configuration.row_verbosity_description",
            "app.configuration.row_reasoning_title",
            "app.configuration.row_reasoning_description",
            "app.configuration.row_communication_title",
            "app.configuration.row_communication_description",
            "app.configuration.match_summary",
            "app.configuration.editable_count",
            "app.configuration.showing_summary",
            "app.configuration.total_settings",
            "app.configuration.edit_target",
            "app.configuration.value_for",
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
        let schema =
            rust_i18n::t!("app.configuration.schema_reference", locale = locale, path = "demo.path").to_string();
        assert!(schema.contains("demo.path"), "{locale}: {schema}");
        let selected = rust_i18n::t!("app.configuration.selected_count", locale = locale, count = 7).to_string();
        assert!(selected.contains('7'), "{locale}: {selected}");
        let show_more = rust_i18n::t!("app.configuration.show_more", locale = locale, remaining = 3).to_string();
        assert!(show_more.contains('3'), "{locale}: {show_more}");
        let show_more_label =
            rust_i18n::t!("app.configuration.show_more_label", locale = locale, shown = 2, total = 9).to_string();
        assert!(show_more_label.contains('2'), "{locale}: {show_more_label}");
        assert!(show_more_label.contains('9'), "{locale}: {show_more_label}");
        let edit_title =
            rust_i18n::t!("app.configuration.edit_title_for", locale = locale, path = "demo.path").to_string();
        assert!(edit_title.contains("demo.path"), "{locale}: {edit_title}");
        let version =
            rust_i18n::t!("app.configuration.current_version", locale = locale, version = "9.9.9").to_string();
        assert!(version.contains("9.9.9"), "{locale}: {version}");
        let read_only = rust_i18n::t!("app.configuration.read_only_note", locale = locale, count = 4).to_string();
        assert!(read_only.contains('4'), "{locale}: {read_only}");
        let summary =
            rust_i18n::t!("app.configuration.editable_summary", locale = locale, editable = 6, count = 11).to_string();
        assert!(summary.contains('6'), "{locale}: {summary}");
        assert!(summary.contains("11"), "{locale}: {summary}");
        let target =
            rust_i18n::t!("app.configuration.write_target_label", locale = locale, label = "Demo", suffix = " (demo)")
                .to_string();
        assert!(target.contains("Demo"), "{locale}: {target}");
        assert!(target.contains(" (demo)"), "{locale}: {target}");
        let effort = rust_i18n::t!("app.configuration.effort_label", locale = locale, label = "Demo").to_string();
        assert!(effort.contains("Demo"), "{locale}: {effort}");
        let saved =
            rust_i18n::t!("app.configuration.saved_overridden", locale = locale, setting = "demo.path", layer = "Demo")
                .to_string();
        assert!(saved.contains("demo.path"), "{locale}: {saved}");
        assert!(saved.contains("Demo"), "{locale}: {saved}");
        let saved_simple =
            rust_i18n::t!("app.configuration.saved_simple", locale = locale, setting = "demo.path", layer = "Demo")
                .to_string();
        assert!(saved_simple.contains("demo.path"), "{locale}: {saved_simple}");
        assert!(saved_simple.contains("Demo"), "{locale}: {saved_simple}");
        let reloaded =
            rust_i18n::t!("app.configuration.settings_reloaded", locale = locale, notice = "demo-notice").to_string();
        assert!(reloaded.contains("demo-notice"), "{locale}: {reloaded}");
        let documented =
            rust_i18n::t!("app.configuration.choose_documented_values", locale = locale, options = "a, b").to_string();
        assert!(documented.contains("a, b"), "{locale}: {documented}");
        let origin = rust_i18n::t!("app.configuration.origin_hint", locale = locale, origin = "Demo").to_string();
        assert!(origin.contains("Demo"), "{locale}: {origin}");
        let default =
            rust_i18n::t!("app.configuration.default_hint", locale = locale, value = "demo-value").to_string();
        assert!(default.contains("demo-value"), "{locale}: {default}");
        let options = rust_i18n::t!("app.configuration.options_hint", locale = locale, options = "a, b").to_string();
        assert!(options.contains("a, b"), "{locale}: {options}");
        let matched = rust_i18n::t!(
            "app.configuration.match_summary",
            locale = locale,
            visible = 3,
            total = 11,
            query = "demo-query"
        )
        .to_string();
        assert!(matched.contains('3'), "{locale}: {matched}");
        assert!(matched.contains("11"), "{locale}: {matched}");
        assert!(matched.contains("demo-query"), "{locale}: {matched}");
        let editable =
            rust_i18n::t!("app.configuration.editable_count", locale = locale, editable = 4, total = 12).to_string();
        assert!(editable.contains('4'), "{locale}: {editable}");
        assert!(editable.contains("12"), "{locale}: {editable}");
        let showing =
            rust_i18n::t!("app.configuration.showing_summary", locale = locale, shown = 5, count = 13).to_string();
        assert!(showing.contains('5'), "{locale}: {showing}");
        assert!(showing.contains("13"), "{locale}: {showing}");
        let total = rust_i18n::t!("app.configuration.total_settings", locale = locale, count = 17).to_string();
        assert!(total.contains("17"), "{locale}: {total}");
        let edit = rust_i18n::t!("app.configuration.edit_target", locale = locale, target = "demo.target").to_string();
        assert!(edit.contains("demo.target"), "{locale}: {edit}");
        let value = rust_i18n::t!("app.configuration.value_for", locale = locale, target = "demo.target").to_string();
        assert!(value.contains("demo.target"), "{locale}: {value}");
    }
}
