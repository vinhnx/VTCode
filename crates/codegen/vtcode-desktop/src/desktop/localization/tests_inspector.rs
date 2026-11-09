use super::UiLanguage;

#[test]
fn inspector_copy_is_localized_for_every_language() {
    // Correct translations that are byte-identical to English. Each entry is
    // asserted equal (not skipped) so a stale entry fails instead of hiding.
    const IDENTICAL_TO_ENGLISH: &[(&str, &str)] = &[
        ("it", "app.inspector.tab_output"),
        ("es", "app.inspector.tab_plan"),
        ("fr", "app.inspector.tab_plan"),
        ("de", "app.inspector.tab_plan"),
        ("nl", "app.inspector.tab_plan"),
        ("sv", "app.inspector.tab_plan"),
        ("pl", "app.inspector.tab_plan"),
        ("tr", "app.inspector.tab_plan"),
    ];
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.inspector.tab_changes",
            "app.inspector.tab_files",
            "app.inspector.tab_activity",
            "app.inspector.tab_output",
            "app.inspector.tab_utilities",
            "app.inspector.tab_plan",
            "app.inspector.empty_plan_title",
            "app.inspector.empty_plan_description",
            "app.inspector.empty_output_title",
            "app.inspector.empty_output_description",
            "app.inspector.history_tag",
            "app.inspector.follow_latest",
            "app.inspector.follow_latest_tooltip",
            "app.inspector.copy_loaded",
            "app.inspector.copy_output",
            "app.inspector.copy_loaded_notice",
            "app.inspector.copy_captured",
            "app.inspector.loading_output",
            "app.inspector.load_more_output",
            "app.inspector.loading_more_output",
            "app.inspector.reading_next_page",
            "app.inspector.truncation_notice",
            "app.inspector.jump_latest",
            "app.inspector.empty_activity_title",
            "app.inspector.empty_activity_description",
            "app.inspector.status_running",
            "app.inspector.status_done",
            "app.inspector.status_failed",
            "app.inspector.status_interrupted",
            "app.inspector.status_other",
            "app.inspector.older_excluded",
            "app.inspector.recent_activity",
            "app.inspector.view_captured_output",
            "app.inspector.jump_to_message",
            "app.inspector.jump_to_message_for",
            "app.inspector.partial_activity_history",
            "app.inspector.inspector_tabs",
            "app.inspector.tabs_hint",
            "app.inspector.empty_changes_title",
            "app.inspector.empty_changes_description",
            "app.inspector.partial_totals",
            "app.inspector.previous_file",
            "app.inspector.next_file",
            "app.inspector.changed_files",
            "app.inspector.partial_diff",
            "app.inspector.preview_unavailable",
            "app.inspector.kind_added",
            "app.inspector.kind_deleted",
            "app.inspector.kind_modified",
            "app.inspector.selected_suffix",
            "app.inspector.previous_change",
            "app.inspector.next_change",
            "app.inspector.expand_review",
            "app.inspector.copy_patch",
            "app.inspector.open_file",
            "app.inspector.partial_review_history",
            "app.inspector.diff_view_label",
            "app.inspector.show_all_activity",
            "app.inspector.show_all_files",
            "app.inspector.jump_latest_activity",
            "app.inspector.filter_output_lines",
            "app.inspector.no_matching_output_lines",
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
        let summary = rust_i18n::t!(
            "app.inspector.activity_summary",
            locale = locale,
            running = 1,
            done = 2,
            failed = 3,
            interrupted = 4,
            other = 5
        )
        .to_string();
        for marker in ['1', '2', '3', '4', '5'] {
            assert!(summary.contains(marker), "{locale}: {summary}");
        }
        let statuses = rust_i18n::t!(
            "app.inspector.statuses_label",
            locale = locale,
            summary = "demo-summary",
            older = "demo-older"
        )
        .to_string();
        assert!(statuses.contains("demo-summary"), "{locale}: {statuses}");
        assert!(statuses.contains("demo-older"), "{locale}: {statuses}");
        let shown = rust_i18n::t!("app.inspector.shown_count", locale = locale, count = 7).to_string();
        assert!(shown.contains('7'), "{locale}: {shown}");
        let filtered = rust_i18n::t!("app.inspector.filtered_count", locale = locale, shown = 2, total = 9).to_string();
        assert!(filtered.contains('2'), "{locale}: {filtered}");
        assert!(filtered.contains('9'), "{locale}: {filtered}");
        let filter =
            rust_i18n::t!("app.inspector.filter_by_status", locale = locale, status = "demo-status").to_string();
        assert!(filter.contains("demo-status"), "{locale}: {filter}");
        let view_output =
            rust_i18n::t!("app.inspector.view_output_for", locale = locale, title = "demo-title").to_string();
        assert!(view_output.contains("demo-title"), "{locale}: {view_output}");
        let jump_message =
            rust_i18n::t!("app.inspector.jump_to_message_for", locale = locale, title = "demo-title").to_string();
        assert!(jump_message.contains("demo-title"), "{locale}: {jump_message}");
        let files = rust_i18n::t!("app.inspector.files_count", locale = locale, count = 3).to_string();
        assert!(files.contains('3'), "{locale}: {files}");
        let review =
            rust_i18n::t!("app.inspector.review_file", locale = locale, path = "demo/path", suffix = " (demo)")
                .to_string();
        assert!(review.contains("demo/path"), "{locale}: {review}");
        assert!(review.contains(" (demo)"), "{locale}: {review}");
        let unavailable = rust_i18n::t!(
            "app.inspector.preview_unavailable_for",
            locale = locale,
            path = "demo/path",
            suffix = " (demo)"
        )
        .to_string();
        assert!(unavailable.contains("demo/path"), "{locale}: {unavailable}");
        assert!(unavailable.contains(" (demo)"), "{locale}: {unavailable}");
        let diff_view =
            rust_i18n::t!("app.inspector.diff_view_label", locale = locale, label = "demo-label", suffix = " (demo)")
                .to_string();
        assert!(diff_view.contains("demo-label"), "{locale}: {diff_view}");
        assert!(diff_view.contains(" (demo)"), "{locale}: {diff_view}");
    }
}
