use super::*;

#[gpui_kit::test]
fn config_deep_link_selects_group_setting_or_search(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![
                    config_page_setting("agent.default_model", json!("model-a"), "Model & Provider"),
                    config_page_setting("agent.thinking_budget", json!(8_000), "Model & Provider"),
                    config_page_setting("tools.auto_approve", json!(true), "Tools"),
                ],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            let action = super::super::composer_controls::DesktopSlashAction::Configuration(Some("Tools".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_page_open);
        assert_eq!(view.read(cx).config_page_group.as_deref(), Some("Tools"));
        assert!(view.read(cx).config_search_input.read(cx).value().is_empty());
        assert!(window.try_find("config-category-Tools").is_some());
        assert!(window.try_find("config-category-Model & Provider").is_some());

        view.update(cx, |view, cx| {
            let action =
                super::super::composer_controls::DesktopSlashAction::Configuration(Some("agent.default_model".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_page_group.as_deref(), Some("Model & Provider"));
        assert!(window.try_find("config-edit-agent.default_model").is_some());
        assert!(
            window.try_find("config-edit-agent.thinking_budget").is_none(),
            "the exact path narrows past its shared prefix"
        );

        view.update(cx, |view, cx| {
            view.config_page_group = Some("Tools".into());
            let action = super::super::composer_controls::DesktopSlashAction::Configuration(Some("budget".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("config-category-Tools").is_none(), "groups without matches leave the rail");
        assert!(window.try_find("config-category-Model & Provider").is_some());
        assert_eq!(
            window.find("config-category-Model & Provider").label(),
            Some("Model & Provider (1)"),
            "searching rail entries carry their match count"
        );
        assert!(window.try_find("config-edit-agent.thinking_budget").is_some());

        view.update(cx, |view, cx| {
            let action =
                super::super::composer_controls::DesktopSlashAction::Configuration(Some("zzz-no-such-setting".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("config-rail").is_none(), "no match leaves no group rail");
        assert!(window.try_find("clear-config-search").is_some());
        window.click("clear-config-search", cx);
        window.render_frame(cx);
        assert!(window.try_find("config-category-Tools").is_some(), "clearing restores every group");
        assert_eq!(
            window.find("config-category-Model & Provider").label(),
            Some("Model & Provider"),
            "clearing the search drops the match counts"
        );
        assert!(window.try_find("cli-config-page-content").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_deep_link_before_catalog_load_filters_once_settings_arrive(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.cli_config_settings.is_none());
            let action = super::super::composer_controls::DesktopSlashAction::Configuration(Some("budget".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_page_open);
        assert_eq!(view.read(cx).config_search_input.read(cx).value().as_str(), "budget");
        assert!(window.try_find("config-loading-label").is_some());

        view.update(cx, |view, _| {
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![
                    config_page_setting("agent.default_model", json!("model-a"), "Model & Provider"),
                    config_page_setting("agent.thinking_budget", json!(8_000), "Model & Provider"),
                    config_page_setting("tools.auto_approve", json!(true), "Tools"),
                ],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
        });
        window.render_frame(cx);
        assert!(window.try_find("config-edit-agent.thinking_budget").is_some());
        assert!(window.try_find("config-edit-agent.default_model").is_none());
        assert!(window.try_find("config-category-Tools").is_none());
    })
    .unwrap();
}
