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
fn double_escape_clears_the_composer_draft_and_typing_disarms_it(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.input("hello", cx);
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "hello", "single escape only arms");
        window.input("x", cx);
        window.render_frame(cx);
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "hellox", "typing disarms the pending clear");
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "", "consecutive presses clear the draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_closes_prompt_history_then_double_escape_clears_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.transcript.push(TranscriptItem::new(
                "history-user-1".into(),
                "user".into(),
                "You".into(),
                "Review stale diffs".into(),
            ));
            view.composer.update(cx, |state, cx| {
                state.set_value("stale", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "escape dismisses the picker");
        assert_eq!(view.read(cx).composer.read(cx).value(), "stale", "dismissal keeps the draft");
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "stale", "dismissal does not arm the clear");
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "", "consecutive presses clear the draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn overlay_open_and_dismiss_disarm_pending_double_escape(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.transcript.push(TranscriptItem::new(
                "history-user-1".into(),
                "user".into(),
                "You".into(),
                "Review stale diffs".into(),
            ));
            view.composer.update(cx, |state, cx| {
                state.set_value("stale", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("escape", cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "escape dismisses the picker");
        // The intervening open/dismiss disarmed the first press: this Esc
        // re-arms instead of clearing.
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "stale", "dismissal disarms the pending clear");
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "", "consecutive presses clear the draft");
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
fn palette_and_attachment_picker_are_mutually_exclusive(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let (commands, _) = tokio::sync::mpsc::channel(2);
        view.backend = Some(BackendHandle { commands });
    });
    cx.update_window(window, |_, window, cx| {
        // Picker open, then a centered palette modal: only the palette survives.
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("attachment-picker-tabs").is_some());
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "a modal palette closes the picker");
        assert!(window.try_find("attachment-picker-tabs").is_none());
        assert!(window.try_find("command-palette").is_some());
        view.update(cx, |view, cx| view.close_palette(window, cx));

        // Picker open, then a composer-anchored popup (the screenshot overlap):
        // opening the Models popup must drop the picker first.
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("attachment-picker-tabs").is_some());
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.attachment_picker.is_none(), "a palette popup closes the picker");
        assert!(window.try_find("attachment-picker-tabs").is_none());
        assert!(state.palette.is_some());
        assert!(state.palette_kind == super::super::PaletteKind::Models);
        assert!(window.try_find("command-palette").is_some());
        view.update(cx, |view, cx| view.close_palette(window, cx));

        // Asymmetric reverse: opening the picker drops an open palette.
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("command-palette").is_some());
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "the picker closes an open palette");
        assert!(window.try_find("command-palette").is_none());
        assert!(window.try_find("attachment-picker-tabs").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn second_attach_trigger_closes_the_picker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("attachment-picker-tabs").is_some());
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "second trigger closes the picker");
        assert!(window.try_find("attachment-picker-tabs").is_none());
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("attachment-picker-tabs").is_some(), "third trigger reopens");
    })
    .unwrap();
}

#[gpui_kit::test]
fn clicking_away_closes_the_attachment_picker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("attachment-picker-tabs").is_some());
        window.click("message-composer-frame", cx);
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "clicking away closes the picker");
        assert!(window.try_find("attachment-picker-tabs").is_none());
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "click-away lands focus in the composer"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn opening_a_route_drops_the_attachment_picker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for index in 0..3 {
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
            window.render_frame(cx);
            assert!(window.try_find("attachment-picker-tabs").is_some());
            match index {
                0 => view.update(cx, |view, cx| view.open_settings(window, cx)),
                1 => view.update(cx, |view, cx| view.open_thread_browser(window, cx)),
                _ => view.update(cx, |view, cx| view.show_review(window, cx)),
            }
            window.render_frame(cx);
            assert!(view.read(cx).attachment_picker.is_none(), "route {index} drops the composer-anchored picker");
            assert!(window.try_find("attachment-picker-tabs").is_none());
            view.update(cx, |view, cx| view.close_active_route(window, cx));
            window.render_frame(cx);
            assert!(
                view.read(cx).attachment_picker.is_none(),
                "returning to the workspace must not resurrect the picker"
            );
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn attachment_picker_stays_inside_a_narrow_window(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    cx.update(|cx| cx.set_reduce_motion(true));
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.files = vec![
            FileEntry {
                rel_path: "notes.md".into(),
                depth: 0,
                is_dir: false,
            },
            FileEntry {
                rel_path: "shot.png".into(),
                depth: 0,
                is_dir: false,
            },
        ];
        view.file_directory_paths = std::sync::Arc::new(Default::default());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        let viewport = window.viewport_size();
        for id in ["attachment-picker-tabs", "picker-file-notes.md", "picker-file-shot.png"] {
            let bounds = window.find(id).bounds();
            assert!(
                bounds.origin.x >= px(0.) && bounds.origin.y >= px(0.),
                "{id} starts outside the viewport: bounds={bounds:?}, viewport={viewport:?}"
            );
            assert!(
                bounds.origin.x + bounds.size.width <= viewport.width
                    && bounds.origin.y + bounds.size.height <= viewport.height,
                "{id} exceeds the viewport: bounds={bounds:?}, viewport={viewport:?}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn outside_click_keeps_editors_with_user_input(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.files = vec![FileEntry {
            rel_path: "README.md".into(),
            depth: 0,
            is_dir: false,
        }];
        view.file_directory_paths = std::sync::Arc::new(Default::default());
    });
    // Rename prefill is user-meaningful content: a stray click must not
    // discard the dialog.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_rename("README.md".to_string(), window, cx));
        window.render_frame(cx);
        assert!(window.try_find("file-rename-dialog").is_some());
        window.click("message-composer-frame", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_rename.is_some(), "outside click keeps the rename dialog");
        assert!(window.try_find("file-rename-dialog").is_some());
        view.update(cx, |view, cx| view.cancel_file_rename(window, cx));
    })
    .unwrap();
    // Same policy for the typed settings editor on its own route.
    view.update(cx, |view, _| {
        let (commands, _) = tokio::sync::mpsc::channel(2);
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
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.open_cli_config_editor("agent.default_model".into(), window, cx);
        });
        window.render_frame(cx);
        // The focus-trap wrapper consumes the root id; the cancel control
        // proves the editor mounted.
        assert!(window.try_find("cancel-cli-config-edit").is_some());
        window.click("settings-navigation", cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some(), "outside click keeps the typed editor");
        assert!(window.try_find("cancel-cli-config-edit").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn opening_a_modal_drops_the_attachment_picker(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "hello").expect("write file");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.files = vec![FileEntry {
            rel_path: "notes.md".into(),
            depth: 0,
            is_dir: false,
        }];
        view.file_directory_paths = std::sync::Arc::new(Default::default());
    });
    // Deferred popovers paint above modal scrims: every modal opener drops
    // the picker so it cannot float over the modal.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("attachment-picker-tabs").is_some());
        view.update(cx, |view, cx| view.open_about(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "about drops the picker");
        assert!(window.try_find("about-dialog").is_some());
        view.update(cx, |view, cx| view.close_about(window, cx));

        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        view.update(cx, |view, cx| view.open_running_confirm(super::super::RunningSwitchIntent::NewThread, window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "running confirm drops the picker");
        assert!(window.try_find("running-confirm-dialog").is_some());
        view.update(cx, |view, cx| view.cancel_running_confirm(window, cx));

        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        view.update(cx, |view, cx| view.open_license_viewer(super::super::licenses::LicenseDocument::Mit, window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "license viewer drops the picker");
        assert!(window.try_find("license-viewer").is_some());
        view.update(cx, |view, cx| view.close_license_viewer(window, cx));

        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        view.update(cx, |view, cx| view.open_file_rename("notes.md".to_string(), window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "rename drops the picker");
        assert!(window.try_find("file-rename-dialog").is_some());
        view.update(cx, |view, cx| view.cancel_file_rename(window, cx));

        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        view.update(cx, |view, cx| {
            view.open_full_access_confirm(
                vtcode_desktop_protocol::SessionOptionsSelectParams {
                    permission_mode: Some(vtcode_desktop_protocol::PermissionMode::FullAccess),
                    ..Default::default()
                },
                window,
                cx,
            )
        });
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "full-access confirm drops the picker");
        assert!(window.try_find("full-access-confirm-dialog").is_some());
        view.update(cx, |view, cx| view.cancel_full_access_confirm(window, cx));

        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
        view.update(cx, |view, cx| view.open_file_preview_overlay("notes.md".to_string(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none(), "file preview drops the picker");
        assert!(window.try_find("file-preview").is_some());
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
