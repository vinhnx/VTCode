use super::*;

fn open_settings_search(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DesktopView>) {
    open_settings_search_sized(cx, 1280., 900.)
}

fn open_settings_search_sized(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (AnyWindowHandle, Entity<DesktopView>) {
    let (window, view) = open_desktop_sized(cx, width, height);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.press("cmd-f", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    (window, view)
}

fn reopen_settings_search(window: AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The legacy search button is gone on wide viewports (Kit owns
        // the sidebar there), so reopen through the same palette entry
        // the button dispatches instead of clicking it.
        view.update(cx, |view, cx| view.open_settings_search_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn license_settings_are_discoverable_and_have_an_accessible_action(cx: &mut TestAppContext) {
    let (window, view) = open_settings_search(cx);
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("license", cx);
        window.render_frame(cx);
        let action = super::super::PaletteAction::SelectSettingsTab(super::super::settings::SettingsTab::About);
        assert_eq!(window.find(palette_action_id(&action)).label(), Some("About"));
        window.within("command-palette").click(palette_action_id(&action), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::About);
        assert_eq!(window.find("about-settings-license").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("about-settings-license").label(), Some("VT Code license"));
        assert_eq!(window.find("about-settings-acknowledgments").label(), Some("Acknowledgments"));
        assert_eq!(window.find("about-settings-website").role(), Some(gpui_kit::Role::Link));
    })
    .unwrap();
    reopen_settings_search(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("menu bar", cx);
        window.render_frame(cx);
        let action = super::super::PaletteAction::SelectSettingsTab(super::super::settings::SettingsTab::VtCode);
        assert_eq!(window.find(palette_action_id(&action)).label(), Some("VT Code"));
        window.within("command-palette").click(palette_action_id(&action), cx);
    })
    .unwrap();
    cx.run_until_parked();
    reopen_settings_search(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("third-party notices", cx);
        window.render_frame(cx);
        let action = super::super::PaletteAction::SelectSettingsTab(super::super::settings::SettingsTab::About);
        assert_eq!(window.find(palette_action_id(&action)).label(), Some("About"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn clicking_settings_chrome_dismisses_the_search(cx: &mut TestAppContext) {
    let (window, _view) = open_settings_search(cx);
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("command-palette").is_some(), "search renders inline");
        // The palette replaces the hosted content while open, so the Kit
        // sidebar is unreachable: an outside click lands on the panel
        // header, which dismisses through the shared panel handler.
        window.click("settings-title", cx);
        window.render_frame(cx);
        assert!(window.try_find("command-palette").is_none(), "outside click dismisses the search");
    })
    .unwrap();
}

#[gpui_kit::test]
fn clicking_the_search_button_again_closes_the_search(cx: &mut TestAppContext) {
    // The search button lives in the legacy navigation: narrow keeps it.
    let (window, _view) = open_settings_search_sized(cx, 600., 700.);
    cx.update_window(window, |_, window, cx| {
        assert!(window.try_find("command-palette").is_some(), "search renders inline");
        window.click("settings-filter-search", cx);
        window.render_frame(cx);
        assert!(window.try_find("command-palette").is_none(), "the search button toggles the search closed");
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_search_matches_singular_and_plural_theme_queries(cx: &mut TestAppContext) {
    let (window, view) = open_settings_search(cx);
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("themes", cx);
        window.render_frame(cx);
        let action = super::super::PaletteAction::SelectSettingsTab(super::super::settings::SettingsTab::Appearance);
        assert_eq!(window.find(palette_action_id(&action)).label(), Some("Appearance"));
        window.within("command-palette").click(palette_action_id(&action), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Appearance);
    })
    .unwrap();
    reopen_settings_search(window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("theme", cx);
        window.render_frame(cx);
        let action = super::super::PaletteAction::SelectSettingsTab(super::super::settings::SettingsTab::Appearance);
        assert_eq!(window.find(palette_action_id(&action)).label(), Some("Appearance"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_search_filters_categories_and_cli_config_paths(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(std::rc::Rc::new(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![vtcode_desktop_protocol::ConfigSetting {
                    path: "agent.default_model".into(),
                    value: json!("model-a"),
                    origin: Some("workspace".into()),
                    label: Some("Default model".into()),
                    type_name: Some("string".into()),
                    default_value: None,
                    description: Some("Model used for new conversations.".into()),
                    options: Vec::new(),
                    group: Some("Model & Provider".into()),
                    group_description: None,
                    editable: true,
                }],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            }));
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.press("cmd-f", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert!(window.within("settings-panel").try_find("command-palette").is_some());
        assert_eq!(window.find("settings-panel").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("command-palette").label(), Some("Search settings and CLI config"));
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::SettingsSearch));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("font", cx);
        window.render_frame(cx);
        assert_eq!(
            window
                .find(palette_action_id(&super::super::PaletteAction::SelectSettingsTab(
                    super::super::settings::SettingsTab::Appearance
                )))
                .label(),
            Some("Appearance")
        );
        window.within("command-palette").click(
            palette_action_id(&super::super::PaletteAction::SelectSettingsTab(
                super::super::settings::SettingsTab::Appearance,
            )),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Appearance);

        view.update(cx, |view, cx| view.open_settings_search_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("agent.default_model", cx);
        window.render_frame(cx);
        let config_action = super::super::PaletteAction::EditConfigSetting("agent.default_model".into());
        assert!(
            window
                .find(palette_action_id(&config_action))
                .label()
                .is_some_and(|label| label.starts_with("Default model"))
        );
        window.within("command-palette").click(palette_action_id(&config_action), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some(), "selecting a CLI setting opens its editor");
        assert!(view.read(cx).current_route() == Route::Settings);
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_config_browser_uses_its_own_search_and_returns_to_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(std::rc::Rc::new(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![vtcode_desktop_protocol::ConfigSetting {
                    path: "agent.default_model".into(),
                    value: json!("model-a"),
                    origin: Some("workspace".into()),
                    label: Some("Default model".into()),
                    type_name: Some("string".into()),
                    default_value: None,
                    description: Some("Model used for new conversations.".into()),
                    options: Vec::new(),
                    group: Some("Model & Provider".into()),
                    group_description: None,
                    editable: true,
                }],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            }));
            let mut owned = view.cli_config_settings.as_deref().unwrap().clone();
            let mut reference = owned.settings[0].clone();
            reference.path = "tools.example".into();
            reference.group = Some("Tools".into());
            reference.label = Some("Tool reference".into());
            reference.editable = false;
            owned.settings.push(reference);
            view.cli_config_settings = Some(std::rc::Rc::new(owned));
            view.slash_commands = vec![vtcode_desktop_protocol::SlashCommandInfo {
                name: "config".into(),
                description: "Open CLI configuration".into(),
            }];
            view.composer.update(cx, |input, cx| {
                input.set_value("/config", window, cx);
                input.set_cursor_position(gpui_kit::component::input::Position::new(0, 7), window, cx);
            });
            view.update_composer_suggestions(window, cx);
            let action =
                super::super::composer_controls::desktop_slash_action("/config").expect("native /config route");
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert!(view.read(cx).config_page_open);
        assert!(window.try_find("cli-config-page").is_some());
        assert!(window.try_find("config-category-curated").is_some());
        window.click("config-category-Model & Provider", cx);
        window.render_frame(cx);
        assert!(window.try_find("config-edit-agent.default_model").is_some());
        assert!(window.try_find("config-category-Model & Provider").is_some());
        assert!(view.read(cx).palette.is_none(), "/config opens the configuration page first");
        window.click("config-category-Tools", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_page_group.as_deref(), Some("Tools"));
        assert!(window.try_find("config-edit-agent.default_model").is_none());
        assert!(window.try_find("config-edit-tools.example").is_none());
        window.click("config-category-Model & Provider", cx);
        window.render_frame(cx);
        assert!(window.try_find("config-edit-agent.default_model").is_some());
        window.click("browse-cli-config", cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::ConfigSettings));
        assert_eq!(window.find("command-palette").label(), Some("VT Code configuration"));
        let action = super::super::PaletteAction::EditConfigSetting("agent.default_model".into());
        assert!(window.try_find(palette_action_id(&action)).is_some());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            view.read(cx).current_route() == Route::Settings,
            "closing the settings browser returns to its parent panel"
        );
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_config_page_keeps_header_fixed_while_catalog_scrolls_in_short_windows(cx: &mut TestAppContext) {
    // 800px tall leaves room for the catalog once the inflexible chrome lays
    // out: the embedded page stacks toolbar, status bar, settings nav, config
    // header, and rail above the rows, which starves them at the 560px
    // sibling fixture under a large UI font.
    let (window, view) = open_desktop_sized(cx, 600., 800.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.set_ui_font_size(view.theme_mode, 24);
            view.apply_typography(cx);
            let settings = (0..6)
                .map(|index| {
                    config_page_setting(
                        &format!("agent.setting_{index}"),
                        json!(format!("value-{index}")),
                        "Model & Provider",
                    )
                })
                .collect();
            view.cli_config_settings = Some(std::rc::Rc::new(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings,
                target_versions: Vec::new(),
                omitted_sensitive: 1,
                omitted_large: 1,
                omitted_limit: 0,
            }));
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        for id in [
            "cli-config-header",
            "config-search",
            "browse-cli-config",
            "reload-cli-config",
            "config-rail",
            "cli-config-page-content",
            "cli-config-footer",
        ] {
            assert!(window.try_find(id).is_some(), "{id} stays visible in short windows");
        }
        let header = window.find("cli-config-header").bounds();
        let content = window.find("cli-config-page-content").bounds();
        let footer = window.find("cli-config-footer").bounds();
        assert!(
            header.size.height > px(0.) && content.size.height > px(0.) && footer.size.height > px(0.),
            "header={header:?} content={content:?} footer={footer:?}"
        );
        assert!(
            header.origin.y + header.size.height <= content.origin.y + px(1.),
            "the header sits above the catalog: {header:?} {content:?}"
        );
        assert!(
            footer.origin.y >= content.origin.y,
            "the notes trail inside the catalog scroll region: {content:?} {footer:?}"
        );
        let before = view.read(cx).settings_scroll.offset().y;
        window.scroll("cli-config-page-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-300.))), cx);
        window.render_frame(cx);
        assert!(view.read(cx).settings_scroll.offset().y < before, "the catalog scrolls");
        assert_eq!(
            window.find("cli-config-header").bounds(),
            header,
            "the header stays fixed while the catalog scrolls"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_rail_sits_beside_content_when_wide_and_above_when_narrow(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1200., 800.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let settings = (0..6)
                .map(|index| {
                    config_page_setting(
                        &format!("agent.setting_{index}"),
                        json!(format!("value-{index}")),
                        "Model & Provider",
                    )
                })
                .collect();
            view.cli_config_settings = Some(std::rc::Rc::new(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings,
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            }));
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        let rail = window.find("config-rail").bounds();
        let content = window.find("cli-config-page-content").bounds();
        assert!(
            rail.origin.x + rail.size.width <= content.origin.x + px(1.),
            "wide viewports put the rail beside the content: rail={rail:?} content={content:?}"
        );
    })
    .unwrap();

    cx.simulate_window_resize(window, size(px(600.), px(700.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let rail = window.find("config-rail").bounds();
        let content = window.find("cli-config-page-content").bounds();
        assert!(
            rail.origin.y + rail.size.height <= content.origin.y + px(1.),
            "narrow viewports stack the rail above the content: rail={rail:?} content={content:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_boolean_switch_writes_the_toggled_value_in_both_directions(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    // Reduce motion so trailing row controls land inside the viewport on
    // the first frame instead of mid-fade.
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(std::rc::Rc::new(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![
                    config_page_setting("agent.verbose", json!(true), "Model & Provider"),
                    config_page_setting("agent.quiet", json!(false), "Model & Provider"),
                ],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            }));
            view.config_page_group = Some("Model & Provider".into());
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("config-toggle-agent.verbose").checked(), Some(true));
        assert_eq!(window.find("config-toggle-agent.quiet").checked(), Some(false));
        window.click("config-toggle-agent.verbose", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("verbose write request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "agent.verbose");
    assert_eq!(params["value"], false);

    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).config_write_pending);
        assert_eq!(
            window.find("config-toggle-agent.quiet").disabled(),
            Some(true),
            "the in-flight write disables every row control"
        );
        view.update(cx, |view, cx| {
            // Simulate the completed write; the reload cycle it triggers is covered elsewhere.
            view.config_write_pending = false;
            view.session_request = None;
            cx.notify();
        });
        window.render_frame(cx);
        window.click("config-toggle-agent.quiet", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("quiet write request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "agent.quiet");
    assert_eq!(params["value"], true);
}

#[gpui_kit::test]
fn config_page_footer_shows_reset_hint_and_write_notice(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.cli_config_settings = Some(std::rc::Rc::new(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![config_page_setting(
                    "agent.default_model",
                    json!("model-a"),
                    "Model & Provider",
                )],
                target_versions: Vec::new(),
                omitted_sensitive: 1,
                omitted_large: 0,
                omitted_limit: 0,
            }));
            view.config_write_notice = Some("Saved agent.default_model to workspace configuration.".into());
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("config-write-notice").role(), Some(gpui_kit::Role::Status));
        assert_eq!(
            window.find("config-write-notice").label(),
            Some("Saved agent.default_model to workspace configuration.")
        );
        assert!(window.try_find("config-reset-hint").is_some());
        assert!(window.try_find("cli-config-footer").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_refreshes_recent_threads_on_demand(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_command_palette(window, cx);
            assert!(
                view.palette_groups[0]
                    .1
                    .iter()
                    .any(|entry| entry.label == "Refresh recent threads")
            );
            view.run_palette_action(super::super::PaletteAction::RefreshSessions, window, cx);
        });
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected recent thread refresh request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LIST);
    assert_eq!(params, json!({ "limit": SESSION_LIST_PAGE_SIZE }));
}

#[gpui_kit::test]
fn config_target_switch_flips_selected_accessibility_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(std::rc::Rc::new(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![config_page_setting(
                    "agent.default_model",
                    json!("model-a"),
                    "Model & Provider",
                )],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            }));
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("config-target-user").checked(), Some(false));
        assert_eq!(window.find("config-target-workspace").checked(), Some(true));
        assert_eq!(window.find("config-target-user").label(), Some("Write CLI settings to User configuration"));
        window.click("config-target-user", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_target, vtcode_desktop_protocol::ConfigTarget::User);
        assert_eq!(window.find("config-target-user").checked(), Some(true));
        assert_eq!(
            window.find("config-target-user").label(),
            Some("Write CLI settings to User configuration (selected)")
        );
        assert_eq!(window.find("config-target-workspace").checked(), Some(false));
    })
    .unwrap();
}
