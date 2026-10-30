use super::UiLanguage;

#[test]
fn navigation_copy_is_localized_for_every_language() {
    for language in UiLanguage::ALL {
        let locale = language.locale();
        // app.navigation.new_thread is exactly covered by
        // workspace_and_composer_copy_covers_every_supported_language.
        for key in [
            "app.navigation.close_thread",
            "app.navigation.resize_navigation",
            "app.navigation.resize_inspector",
            "app.navigation.resize_hint",
            "app.navigation.close_session",
            "app.navigation.toggle_navigation",
            "app.navigation.show_navigation",
            "app.navigation.workspace_fallback",
            "app.navigation.back",
            "app.navigation.back_navigating",
            "app.navigation.forward",
            "app.navigation.forward_navigating",
            "app.navigation.workspace_utilities",
            "app.navigation.workspace_utilities_selected",
            "app.navigation.search_commands",
            "app.navigation.hide_panel",
            "app.navigation.show_panel",
            "app.navigation.toggle_inspector",
            "app.navigation.toggle_zen_mode",
            "app.navigation.toggle_status_bar",
            "app.navigation.settings_menu",
            "app.navigation.settings_button",
            "app.navigation.window_actions",
            "app.navigation.conversation_workspace",
            "app.navigation.enter_zen",
            "app.navigation.exit_zen",
            "app.navigation.projects_threads",
            "app.navigation.projects_threads_expanded",
            "app.navigation.navigation_label",
            "app.navigation.navigation_description",
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!value.is_empty(), "{locale}/{key}");
            if language != UiLanguage::English {
                assert_ne!(value, rust_i18n::t!(key, locale = "en").to_string(), "{locale}/{key}");
            }
        }
    }
}
