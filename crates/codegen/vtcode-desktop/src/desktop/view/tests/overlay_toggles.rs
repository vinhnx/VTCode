use super::*;

#[gpui_kit::test]
fn second_trigger_closes_settings_browser_and_same_kind_palettes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Action dispatch is deferred; park the loop before asserting each effect.
    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "open-settings", cx);
        window.dispatch_action(Box::new(super::super::OpenSettings), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        window.dispatch_action(Box::new(super::super::OpenSettings), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Settings, "second Settings action must close the page");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "nav-browse-threads", cx);
        window.dispatch_action(Box::new(super::super::OpenThreadBrowser), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Browser);
        window.dispatch_action(Box::new(super::super::OpenThreadBrowser), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Browser, "second browser action must close the browser");

        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "second Commands open must close the palette");

        view.update(cx, |view, _| {
            let (commands, _) = tokio::sync::mpsc::channel(2);
            view.backend = Some(BackendHandle { commands });
        });
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "second Models open must close the palette");

        // A different palette kind replaces instead of closing.
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.palette.is_some());
        assert!(state.palette_kind == super::super::PaletteKind::Models, "switching kinds must replace the palette");
    })
    .unwrap();
}

#[gpui_kit::test]
fn route_back_key_returns_to_workspace_and_focuses_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::OpenSettings), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        window.press("cmd-[", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "cmd-[ must go back to the workspace");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "route back lands focus in the composer"
        );
        // On the workspace route cmd-[ propagates instead of acting.
        window.press("cmd-[", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace);
    })
    .unwrap();
}

#[gpui_kit::test]
fn route_panels_embed_under_persistent_chrome(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for index in 0..3 {
        cx.update_window(window, |_, window, cx| {
            let panel = match index {
                0 => {
                    view.update(cx, |view, cx| view.open_settings(window, cx));
                    "settings-panel"
                }
                1 => {
                    view.update(cx, |view, cx| view.open_thread_browser(window, cx));
                    "thread-browser"
                }
                _ => {
                    view.update(cx, |view, cx| view.show_review(window, cx));
                    "diff-review-panel"
                }
            };
            window.render_frame(cx);
            assert!(window.try_find("title-bar").is_some(), "toolbar persists under {panel}");
            assert!(window.try_find("primary-navigation").is_some(), "sidebar persists under {panel}");
            assert_panel_embeds(window, panel);
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn narrow_panels_collapse_the_sidebar_to_its_rail(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("project-thread-sidebar").is_some(), "sidebar starts expanded");
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("navigation-rail").is_some(), "rail persists under the panel");
        assert!(window.try_find("project-thread-sidebar").is_none(), "panel collapses the sidebar");
        assert!(view.read(cx).sidebar_open, "collapse keeps the preference");
        view.update(cx, |view, cx| view.close_settings(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("project-thread-sidebar").is_some(), "sidebar returns after close");
    })
    .unwrap();
}

#[gpui_kit::test]
fn toolbar_back_pops_the_panel_and_forward_reopens_it(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.click("navigate-back", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "toolbar back pops the panel");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "pop lands focus in the composer"
        );
        // A second synthetic titlebar click trips Kit's unimplemented test
        // start_window_move (TitleBar arms a drag on mouse-down), so drive
        // Forward through its action; enablement is pinned below instead.
        window.dispatch_action(Box::new(super::super::NavigateForward), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "forward reopens the panel");
        assert_eq!(window.find("settings-navigation").focused(), Some(true), "reopened panel takes focus");
    })
    .unwrap();
}

#[gpui_kit::test]
fn forward_reopens_each_panel_with_its_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for index in 0..3 {
        cx.update_window(window, |_, window, cx| {
            let (panel, focus_id) = match index {
                0 => {
                    view.update(cx, |view, cx| view.open_settings(window, cx));
                    ("settings-panel", "settings-navigation")
                }
                1 => {
                    view.update(cx, |view, cx| view.open_thread_browser(window, cx));
                    ("thread-browser", "thread-browser")
                }
                _ => {
                    view.update(cx, |view, cx| view.show_review(window, cx));
                    ("diff-review-panel", "diff-review-panel")
                }
            };
            window.render_frame(cx);
            view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
            window.render_frame(cx);
            assert!(view.read(cx).current_route() == Route::Workspace);
            view.update(cx, |view, cx| assert!(view.go_route_forward(window, cx)));
            window.render_frame(cx);
            assert!(window.try_find(panel).is_some(), "forward reopens {panel}");
            assert_eq!(window.find(focus_id).focused(), Some(true), "{panel} takes focus");
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn close_truncates_the_panel_while_back_preserves_forward(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.click("settings-dismiss", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace);
        assert!(!view.read(cx).route_history.can_forward(), "close truncates forward entries");
        assert_eq!(window.find("navigate-forward").disabled(), Some(true));
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.click("navigate-back", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace);
        assert!(view.read(cx).route_history.can_forward(), "back preserves forward entries");
        assert_ne!(window.find("navigate-forward").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn route_back_walks_settings_interior_before_the_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.config_page_open = true;
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_page_open);
        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        window.render_frame(cx);
        assert!(!view.read(cx).config_page_open, "back returns to the parent tab");
        assert!(view.read(cx).current_route() == Route::Settings);
        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "next back pops the panel");
    })
    .unwrap();
}

#[gpui_kit::test]
fn route_back_peels_settings_layers_in_order(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
            settings: vec![vtcode_desktop_protocol::ConfigSetting {
                path: "agent.default_model".into(),
                value: serde_json::json!("model-a"),
                origin: Some("workspace".into()),
                label: Some("Default model".into()),
                type_name: Some("string".into()),
                default_value: None,
                description: None,
                options: Vec::new(),
                group: None,
                group_description: None,
                editable: true,
            }],
            target_versions: Vec::new(),
            omitted_sensitive: 0,
            omitted_large: 0,
            omitted_limit: 0,
        });
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.click("shortcut-edit-commands", cx);
        view.update(cx, |view, cx| {
            view.open_cli_config_editor("agent.default_model".into(), window, cx);
            view.open_settings_search_palette(window, cx);
            view.config_page_open = true;
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some());
        assert!(view.read(cx).shortcut_capture.is_some());
        assert!(view.read(cx).palette.is_some());
        assert!(view.read(cx).config_page_open);

        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        assert!(view.read(cx).config_editor.is_none(), "back closes the editor first");
        assert!(view.read(cx).shortcut_capture.is_some());
        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        assert!(view.read(cx).shortcut_capture.is_none(), "back cancels capture second");
        assert!(view.read(cx).palette.is_some());
        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        assert!(view.read(cx).palette.is_none(), "back closes the palette third");
        assert!(view.read(cx).config_page_open);
        assert!(view.read(cx).current_route() == Route::Settings);
        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        assert!(!view.read(cx).config_page_open, "back returns to the parent tab");
        assert!(view.read(cx).current_route() == Route::Settings);
        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        assert!(view.read(cx).current_route() == Route::Workspace, "back pops the panel last");
        view.update(cx, |view, cx| assert!(!view.go_route_back(window, cx)));
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_closes_settings_palette_before_the_panel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.open_settings_search_palette(window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "escape closes the palette layer first");
        assert!(view.read(cx).current_route() == Route::Settings);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "next escape pops the panel");
    })
    .unwrap();
}

#[gpui_kit::test]
fn toolbar_back_prefers_the_panel_over_session_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        let workspace = view.workspace.clone().unwrap();
        view.navigation_history
            .record(NavigationLocation { workspace: workspace.clone(), session_id: None });
        view.navigation_history
            .record(NavigationLocation { workspace, session_id: Some("current".into()) });
        view.selected_session_id = Some("current".into());
    });
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).can_navigate_back(), "history back is available");
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.click("navigate-back", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "back pops the panel first");
        assert!(view.read(cx).can_navigate_back(), "session history survives the panel pop");
    })
    .unwrap();
}

#[gpui_kit::test]
fn new_session_action_pops_an_open_panel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        window.dispatch_action(Box::new(super::super::NewSession), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            view.read(cx).current_route() == Route::Workspace,
            "starting a thread leaves the panel for the workspace"
        );
    })
    .unwrap();
}
