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
            "app.navigation.back_swipe",
            "app.navigation.forward",
            "app.navigation.forward_navigating",
            "app.navigation.forward_swipe",
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

/// The tooltip owns the full message: appending a hardcoded English hint to
/// the translated direction word leaves every non-English locale half-English.
#[test]
fn swipe_navigation_tooltips_are_whole_translated_messages() {
    assert_eq!(rust_i18n::t!("app.navigation.back_swipe", locale = "en").to_string(), "Back (two-finger swipe)");
    assert_eq!(
        rust_i18n::t!("app.navigation.forward_swipe", locale = "en").to_string(),
        "Forward (two-finger swipe)"
    );
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for (key, direction_key) in [
            ("app.navigation.back_swipe", "app.navigation.back"),
            ("app.navigation.forward_swipe", "app.navigation.forward"),
        ] {
            let value = rust_i18n::t!(key, locale = locale).to_string();
            let direction = rust_i18n::t!(direction_key, locale = locale).to_string();
            assert!(value.contains(&direction), "{locale}/{key} must keep the translated direction");
            assert_ne!(value, direction, "{locale}/{key} must own its hint");
            if language != UiLanguage::English {
                assert!(!value.contains("two-finger"), "{locale}/{key} leaked the English hint");
            }
        }
    }

    // Catalog values cannot catch a call site that rebuilds the tooltip from
    // the direction key plus an English literal — that was the original bug.
    let toolbar =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/desktop/view/chrome.rs"))
            .expect("toolbar source is readable");
    assert!(toolbar.contains("app.navigation.back_swipe"));
    assert!(toolbar.contains("app.navigation.forward_swipe"));
    assert!(!toolbar.contains("(two-finger swipe)"));
}
