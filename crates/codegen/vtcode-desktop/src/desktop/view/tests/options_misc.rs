use super::*;

#[test]
fn permission_palette_rows_resolve_shield_modes_only_for_permission_picks() {
    use super::super::{PaletteAction, PaletteKind, permission_palette_mode};
    use vtcode_desktop_protocol::{PermissionMode, SessionOptionsSelectParams};

    let options = |permission_mode: Option<PermissionMode>| {
        PaletteAction::SelectSessionOptions(SessionOptionsSelectParams { permission_mode, ..Default::default() })
    };
    assert_eq!(
        permission_palette_mode(PaletteKind::Permissions, &options(Some(PermissionMode::Ask))),
        Some(PermissionMode::Ask)
    );
    assert_eq!(
        permission_palette_mode(PaletteKind::Permissions, &options(Some(PermissionMode::AutoApproveCommands))),
        Some(PermissionMode::AutoApproveCommands)
    );
    assert_eq!(permission_palette_mode(PaletteKind::Permissions, &options(None)), None);
    assert_eq!(
        permission_palette_mode(
            PaletteKind::Permissions,
            &PaletteAction::SelectSessionOptions(SessionOptionsSelectParams {
                permission_mode: Some(PermissionMode::Ask),
                mode: Some("plan".into()),
                ..Default::default()
            })
        ),
        Some(PermissionMode::Ask),
        "mixed selections still carry their shield mode"
    );
    assert_eq!(permission_palette_mode(PaletteKind::Permissions, &PaletteAction::NewSession), None);
    assert_eq!(
        permission_palette_mode(PaletteKind::Modes, &options(Some(PermissionMode::Ask))),
        None,
        "other palettes stay text-only"
    );
    assert_eq!(permission_palette_mode(PaletteKind::Commands, &options(None)), None);
}

#[gpui_kit::test]
fn composer_tracks_the_active_agent_without_changing_next_session_mode(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("session-a".into());
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Build"));

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::PrimaryAgentChanged(vtcode_desktop_protocol::PrimaryAgentChanged {
                    session_id: "session-a".into(),
                    display_name: Some("Plan".into()),
                }),
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Plan"));
        assert_eq!(
            view.read(cx)
                .configuration
                .as_ref()
                .unwrap()
                .session_options
                .as_ref()
                .unwrap()
                .mode,
            "build"
        );

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::PrimaryAgentChanged(vtcode_desktop_protocol::PrimaryAgentChanged {
                    session_id: "session-b".into(),
                    display_name: Some("Stale agent".into()),
                }),
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Plan"));

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::PrimaryAgentChanged(vtcode_desktop_protocol::PrimaryAgentChanged {
                    session_id: "session-a".into(),
                    display_name: None,
                }),
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Build"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_settings_reload_waits_for_shutdown_and_preserves_draft_sources(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "notes.md".into(),
                text: "local draft source".into(),
            });
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this unsent draft", window, cx));
            view.reload_cli_settings(cx);
            assert!(matches!(requests.try_recv(), Ok(BackendCommand::Stop)));
            assert_eq!(view.pending_workspace, view.workspace);
            assert!(view.backend.is_some(), "must not replace a still-running service");
            view.reload_cli_settings(cx);
            assert!(requests.try_recv().is_err(), "only one shutdown may be pending");
            let workspace = view.workspace.clone().unwrap();
            view.prepare_workspace(&workspace, cx);
            assert_eq!(view.attachments[0].text, "local draft source");
            assert_eq!(view.composer.read(cx).value().as_str(), "Keep this unsent draft");
            assert!(view.configuration.is_none());
            assert!(view.selected_session_id.is_none());
            view.prepare_workspace(&workspace.join("different-workspace"), cx);
            assert!(view.attachments.is_empty(), "workspace-bound sources cannot cross workspaces");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_settings_reload_rejects_busy_and_closed_transport(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.running = true;
            view.reload_cli_settings(cx);
            assert!(requests.try_recv().is_err());
            assert!(view.pending_workspace.is_none());
            view.running = false;
            view.attachments_picker_open = true;
            view.reload_cli_settings(cx);
            assert!(requests.try_recv().is_err());
            view.attachments_picker_open = false;
            drop(requests);
            view.reload_cli_settings(cx);
            assert!(view.pending_workspace.is_none(), "failed shutdown must not strand the transition gate");
            assert!(view.error.as_deref().unwrap().contains("queue is unavailable"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_settings_reload_button_and_action_share_the_transition_guard(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 3, cx);
        window.click("reload-cli-settings", cx);
        assert!(matches!(requests.try_recv(), Ok(BackendCommand::Stop)));
        assert!(view.read(cx).pending_workspace.is_some());
        window.dispatch_action(Box::new(super::super::ReloadCliSettings), cx);
        assert!(requests.try_recv().is_err(), "the keyboard action cannot bypass the pending restart");
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_workspace_is_inset_below_the_toolbar_and_after_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let navigation = window.find("primary-navigation").bounds();
        let toolbar = window.find("title-bar").bounds();
        let workspace = window.find("workspace-surface");
        let bounds = workspace.bounds();
        let navigation_on_right = view.read(cx).preferences.navigation_on_right();

        assert_eq!(workspace.role(), Some(gpui_kit::Role::Group));
        assert_eq!(workspace.label(), Some("Conversation workspace"));
        if navigation_on_right {
            assert!(bounds.right() < navigation.left(), "workspace must have a visible right inset: {bounds:?}");
        } else {
            assert!(bounds.left() > navigation.right(), "workspace must have a visible left inset: {bounds:?}");
        }
        assert!(bounds.top() > toolbar.bottom(), "workspace must have a visible top inset: {bounds:?}");
        assert!(bounds.size.width > px(500.));
        assert!(bounds.size.height > px(500.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn navigation_sidebar_spans_the_full_window_height(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).preferences.navigation_on_right(), "navigation defaults to the right");
        let viewport = window.viewport_size();
        let toolbar = window.find("title-bar").bounds();
        let navigation = window.find("primary-navigation").bounds();
        let panel = window.find("project-thread-sidebar").bounds();
        assert_eq!(navigation.top(), toolbar.top(), "navigation starts at the window top");
        assert_eq!(navigation.bottom(), viewport.height, "navigation spans the full window height");
        assert_eq!(panel.top(), toolbar.top(), "sidebar panel starts at the window top");
        assert_eq!(panel.bottom(), viewport.height, "sidebar panel spans the full window height");

        // The status bar lives in the content column, so showing it must not
        // shorten the sidebar.
        view.update(cx, |view, _| view.preferences.show_status_bar = true);
        window.render_frame(cx);
        let status = window.find("status-bar").bounds();
        let navigation = window.find("primary-navigation").bounds();
        assert_eq!(status.bottom(), viewport.height, "status bar anchors the content column bottom");
        assert_eq!(navigation.top(), toolbar.top());
        assert_eq!(navigation.bottom(), viewport.height);
    })
    .unwrap();
}

#[gpui_kit::test]
fn left_navigation_surface_stays_full_height_while_content_clears_the_toolbar(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| view.preferences.navigation_on_right = Some(false));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let viewport = window.viewport_size();
        let toolbar = window.find("title-bar").bounds();
        let navigation = window.find("primary-navigation").bounds();
        let panel = window.find("project-thread-sidebar").bounds();
        assert_eq!(navigation.top(), toolbar.top(), "left navigation surface stays full height");
        assert_eq!(navigation.bottom(), viewport.height);
        assert_eq!(panel.top(), toolbar.top());
        assert!(
            window.find("nav-home").bounds().top() >= toolbar.bottom(),
            "rail buttons start below the traffic-light row"
        );
        assert!(
            window.find("session-list").bounds().top() >= toolbar.bottom(),
            "sidebar content starts below the traffic-light row"
        );
    })
    .unwrap();
}

fn browser_session(
    id: &str,
    workspace: &str,
    updated_at: Option<u64>,
    size_bytes: u64,
) -> vtcode_desktop_protocol::SessionSummary {
    vtcode_desktop_protocol::SessionSummary {
        session_id: id.into(),
        title: Some(format!("Thread {id}")),
        workspace: workspace.into(),
        completed: false,
        resumable: true,
        size_bytes,
        updated_at,
    }
}

#[gpui_kit::test]
fn thread_browser_groups_current_workspace_first_at_full_window_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let current = std::env::temp_dir().display().to_string();
    view.update(cx, |view, _| {
        // Newest-first globally would put other-new on top; grouping must win.
        view.sessions = vec![
            browser_session("other-new", "/work/other", Some(50), 2048),
            browser_session("cur-old", current.as_str(), Some(10), 100),
            browser_session("cur-new", current.as_str(), Some(30), 0),
            browser_session("other-old", "/work/other", Some(5), 100),
            browser_session("ancient", "/work/other", None, 0),
        ];
        view.selected_session_id = Some("cur-old".into());
    });

    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        let viewport = window.viewport_size();
        let dialog = window.find("thread-browser");
        assert_eq!(dialog.role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(dialog.label(), Some("Browse threads"));
        let bounds = dialog.bounds();
        assert!(
            bounds.left() < px(100.) && bounds.top() < px(100.),
            "dialog starts near the window edge: {bounds:?}"
        );
        assert!(
            bounds.right() > viewport.width - px(100.) && bounds.bottom() > viewport.height - px(100.),
            "dialog spans nearly the full window: {bounds:?} in {viewport:?}"
        );
        assert_eq!(window.find("thread-browser-list").role(), Some(gpui_kit::Role::List));

        let top = |id: &str| window.find(format!("browser-session-row-{id}")).bounds().top();
        assert!(top("cur-new") < top("cur-old"), "current group sorts newest first");
        assert!(top("cur-old") < top("other-new"), "current group precedes other workspaces");
        assert!(top("other-new") < top("other-old"), "other group sorts newest first");
        assert!(top("other-old") < top("ancient"), "unknown timestamps sort last");
        assert!(window.try_find("browser-session-meta-other-new").is_some(), "age and size share one lane");
        assert!(window.try_find("browser-session-meta-cur-new").is_some(), "age alone still shows a lane");
        assert!(
            window.try_find("browser-session-meta-ancient").is_none(),
            "unknown timestamp with unknown size shows no lane"
        );
        assert_eq!(window.find("browser-session-row-cur-old").selected(), Some(true));
        assert_eq!(window.find("browser-session-row-cur-new").selected(), Some(false));
        assert_eq!(window.find("browser-session-cur-old").label(), Some("Thread cur-old (current thread)"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn thread_browser_row_load_selects_thread_and_closes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.sessions = vec![browser_session("load-me", "/work/other", Some(9), 0)];
    });

    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        window.click("browser-session-load-me", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_none(), "loading dismisses the browser");
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("selecting a browser row must request the session load");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "load-me" }));
}

#[gpui_kit::test]
fn thread_browser_dismiss_button_closes_and_focuses_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "nav-browse-threads", cx);
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());

        window.click("browser-dismiss", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_none());
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "closing the route lands focus deterministically in the composer"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn thread_browser_escape_focuses_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "nav-browse-threads", cx);
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());
        assert_eq!(window.find("thread-browser").focused(), Some(true));

        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_none());
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "escape from the route lands focus deterministically in the composer"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn thread_browser_new_thread_requests_close_and_marks_pending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.sessions = vec![browser_session("current", "/work/main", Some(9), 0)];
        view.selected_session_id = Some("current".into());
    });

    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        window.click("browser-new-session", cx);
        window.render_frame(cx);
        assert!(view.read(cx).pending_new_thread);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("browser new-thread must close the selected session first");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    assert_eq!(params, json!({ "sessionId": "current" }));
}

#[gpui_kit::test]
fn thread_browser_close_thread_closes_selected_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.sessions = vec![browser_session("stale", "/work/main", Some(9), 0)];
        view.selected_session_id = Some("stale".into());
    });

    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        window.click("browser-close-session", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("browser close-thread must close the selected session");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    assert_eq!(params, json!({ "sessionId": "stale" }));
}

#[gpui_kit::test]
fn thread_browser_shows_empty_state_without_sessions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.sessions = Vec::new();
    });

    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());
        assert!(window.try_find("thread-browser-list").is_none(), "empty browser shows no list");
        assert!(window.try_find("browser-new-session").is_some(), "empty browser still offers a new thread");
    })
    .unwrap();
}

#[gpui_kit::test]
fn thread_browser_and_settings_stay_mutually_exclusive(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "nav-browse-threads", cx);
        window.dispatch_action(Box::new(super::super::OpenThreadBrowser), cx);
    })
    .unwrap();
    // Dispatch is deferred; park the loop before asserting the effect.
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some(), "the action opens the browser");

        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("settings-panel").is_some());
        assert!(window.try_find("thread-browser").is_none(), "settings replaces the browser");

        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());
        assert!(view.read(cx).route != Route::Settings, "browser replaces settings");
    })
    .unwrap();
}

#[gpui_kit::test]
fn thread_browser_replaces_command_palette(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("command-palette").is_some());

        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());
        assert!(window.try_find("command-palette").is_none(), "browser replaces the palette");
    })
    .unwrap();
}

#[gpui_kit::test]
fn thread_browser_from_palette_focuses_composer_on_escape(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        focus_element_by_tab(window, "nav-browse-threads", cx);
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.input("browse", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window
                .find(palette_action_id(&super::super::PaletteAction::BrowseThreads))
                .label(),
            Some("Browse threads")
        );
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("command-palette").is_none());
        assert!(window.try_find("thread-browser").is_some(), "confirming the command opens the browser");

        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_none());
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "escape from the palette-opened route lands focus deterministically in the composer"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_activation_dismisses_browser_and_focuses_input(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());

        view.update(cx, |view, cx| view.activate_composer(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_none(), "activating the composer dismisses the browser");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "activating the composer focuses its input"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn trackpad_swipe_over_open_browser_does_not_navigate(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());
        window.click_with_options(
            "thread-browser",
            ClickOptions::new().with_button(MouseButton::Navigate(NavigationDirection::Back)),
            cx,
        );
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "a swipe over an open browser must not navigate the background");

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.close_browser(window, cx));
        window.render_frame(cx);
        window.click_with_options(
            "pane-root",
            ClickOptions::new().with_button(MouseButton::Navigate(NavigationDirection::Back)),
            cx,
        );
    })
    .unwrap();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("closing the browser must re-arm background swipe navigation");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
}

#[gpui_kit::test]
fn settings_shortcut_opens_webmcp_sections_with_unique_accessibility_ids(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.webmcp_status = Some(connected_webmcp_status());
            view.webmcp_pairing = Some(vtcode_desktop_protocol::WebmcpPairResult {
                endpoint: "ws://127.0.0.1:43719/mcp".into(),
                origin: "https://example.test".into(),
                pairing_code: "0123456789abcdef".into(),
                expires_in_secs: 300,
            });
        });
        window.press("cmd-,", cx);
        window.render_frame(cx);
        select_settings_category(window, 4, cx);

        assert!(view.read(cx).route == Route::Settings, "the Settings shortcut opens the dialog");
        assert_eq!(window.find_all("webmcp-status").len(), 1);
        assert_eq!(window.find_all("webmcp-pairing").len(), 1);
        assert_eq!(window.find_all("refresh-webmcp-status").len(), 1);
        assert_eq!(window.find_all("webmcp-pairing-details").len(), 1);
        assert_eq!(window.find("webmcp-session-status").role(), Some(gpui_kit::Role::Status));
        assert_eq!(window.find("webmcp-session-status").label(), Some("Connected · desktop session ready"));
        view.update(cx, |view, cx| {
            view.webmcp_status_dirty = true;
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("webmcp-session-status").label(),
            Some("Connected · desktop session availability unavailable")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_utilities_can_return_to_changes_without_creating_a_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.workspace = Some(std::env::temp_dir()));
        window.render_frame(cx);
        window.click("nav-utilities", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.press("home", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        assert!(view.read(cx).selected_session_id.is_none());
        assert_eq!(window.find("send-prompt").label(), Some("Send message"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn failed_working_tree_refresh_keeps_the_last_successful_snapshot(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let patch = "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\n";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
                slash_commands: Vec::new(),
            });
            view.open_workspace_review(window, cx);
            view.refresh_workspace_review(cx);
        });
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
            panic!("initial review request")
        };
        view.update(cx, |view, cx| {
            view.accept_workspace_review(
                vtcode_desktop_protocol::WorkspaceReviewResult {
                    include_untracked: false,
                    scope: String::new(),
                    request_id: params["requestId"].as_str().unwrap().to_owned(),
                    workspace: "/workspace".into(),
                    captured_at: "first capture".into(),
                    patch: Some(patch.into()),
                    truncated: false,
                    note: "Tracked changes against HEAD".into(),
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("workspace-review-diff").is_some());

        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("refresh request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::WORKSPACE_REVIEW);
        window.render_frame(cx);
        assert!(window.try_find("workspace-review-refreshing").is_some());
        assert!(window.try_find("workspace-review-diff").is_some());

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::WORKSPACE_REVIEW.into(),
                    result: None,
                    error: Some("Git capture timed out".into()),
                },
                cx,
            );
            let review = view.workspace_review.as_ref().unwrap();
            assert!(!review.pending);
            assert_eq!(review.snapshot.as_ref().unwrap().patch.as_deref(), Some(patch));
            assert!(review.viewer.is_some());
        });
        window.render_frame(cx);
        assert!(window.try_find("workspace-review-refreshing").is_none());
        assert!(window.try_find("workspace-review-refresh-warning").is_some());
        assert!(window.try_find("workspace-review-diff").is_some());
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch));
    })
    .unwrap();
}

#[gpui_kit::test]
fn effort_knob_track_click_commits_the_supported_level(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        window.render_frame(cx);
        window.click(("effort-stop", 2usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("effort request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn model_options_panel_stays_open_for_effort_and_speed_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        view.update(cx, |view, cx| view.effort_focus.focus(window, cx));
        // medium -> high exercises a successful commit; the panel must survive it.
        window.press("right", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("effort request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.palette.is_some(), "effort change must keep the panel open");
        assert!(state.palette_kind == super::super::PaletteKind::ModelOptions);
        assert!(state.effort_slider.is_some(), "slider state must survive the commit");
        assert_eq!(state._effort_slider_subscriptions.len(), 2);
        assert!(state.effort_focus.is_focused(window), "focus must stay on the knob, not jump to the composer");
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        assert!(!state.can_select_session_options(), "controls lock while the CLI request is pending");
        // While pending, knob input and the speed toggle must neither submit nor move state.
        let slider_value = state.effort_slider.as_ref().unwrap().read(cx).value().start();
        assert_eq!(slider_value, 2., "knob shows the committed high level");
        view.update(cx, |view, cx| view.effort_focus.focus(window, cx));
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).effort_slider.as_ref().unwrap().read(cx).value().start(),
            2.,
            "pending knob input must not move the slider"
        );
        window.click("fast-mode-toggle", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "pending input must not submit");
    cx.update_window(window, |_, window, cx| {
        // CLI acknowledgement re-enables the panel without dismissing it.
        let mut ack = runtime_options_config();
        ack.session_options.as_mut().unwrap().reasoning_effort = "high".into();
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: Some(json!(ack)),
                    error: None,
                },
                cx,
            );
        });
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.palette.is_some(), "CLI ack must keep the panel open");
        assert!(state.palette_kind == super::super::PaletteKind::ModelOptions);
        assert!(state.can_select_session_options(), "ack re-enables the controls");
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: high"));
        // Boundary no-op when idle: already at high, pressing right submits nothing.
        view.update(cx, |view, cx| view.effort_focus.focus(window, cx));
        window.press("right", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "boundary press must not submit");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.effort_focus.focus(window, cx));
        window.press("left", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("medium request") else {
        panic!("medium request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "medium");
    cx.update_window(window, |_, window, cx| {
        // A drag released while blocked must revert to the CLI value, not stick.
        view.update(cx, |view, cx| {
            let slider = view.effort_slider.clone().unwrap();
            slider.update(cx, |state, cx| {
                state.set_value(0., window, cx);
                cx.emit(gpui_kit::component::slider::SliderEvent::Release(state.value()));
            });
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "blocked drag must not submit");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).effort_slider.as_ref().unwrap().read(cx).value().start(),
            2.,
            "blocked drag must revert to the CLI high level"
        );
        assert!(view.read(cx).palette.is_some(), "revert must keep the panel open");
        let mut ack = runtime_options_config();
        ack.session_options.as_mut().unwrap().reasoning_effort = "medium".into();
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: Some(json!(ack)),
                    error: None,
                },
                cx,
            );
        });
        window.render_frame(cx);
        // Slider (high) now trails the acked CLI value (medium); the next knob
        // step re-syncs without a spurious request.
        view.update(cx, |view, cx| view.effort_focus.focus(window, cx));
        window.press("left", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "re-sync step must not submit");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).effort_slider.as_ref().unwrap().read(cx).value().start(),
            1.,
            "re-sync step must land on the CLI medium level"
        );
        assert!(view.read(cx).palette.is_some(), "boundary no-op must keep the panel open");
        window.click("fast-mode-toggle", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("speed request") else {
        panic!("speed request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["fastMode"], false);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.palette.is_some(), "speed toggle must keep the panel open");
        assert!(state.palette_kind == super::super::PaletteKind::ModelOptions);
        assert!(state.effort_slider.is_some());
        let mut ack = runtime_options_config();
        let options = ack.session_options.as_mut().unwrap();
        options.reasoning_effort = "medium".into();
        options.fast_mode_enabled = false;
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: Some(json!(ack)),
                    error: None,
                },
                cx,
            );
            view.close_palette(window, cx);
            let mut config = runtime_options_config();
            config.session_options.as_mut().unwrap().reasoning_effort = "max".into();
            view.configuration = Some(config);
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        // Unsupported inherited value offers explicit choices without a knob.
        assert!(view.read(cx).effort_slider.is_none());
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("explicit effort request") else {
        panic!("explicit effort request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "low");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.palette.is_some(), "explicit effort choice must keep the panel open");
        assert!(state.palette_kind == super::super::PaletteKind::ModelOptions);
        assert!(window.try_find("command-palette").is_some());
        // Escape still dismisses the topmost surface and returns focus.
        view.update(cx, |view, cx| view.dismiss_overlay(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "Escape must still dismiss the panel");
    })
    .unwrap();
}

#[gpui_kit::test]
fn borderless_composer_keeps_prompt_marker_and_layout(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.activate_composer(window, cx));
        window.render_frame(cx);
        // CLI-minimal contract: the card chrome is gone, so the prompt marker
        // is the input affordance and the frame must still lay out with height.
        let frame = window.find("message-composer-frame").bounds();
        let marker = window.find("composer-prompt-marker").bounds();
        assert!(frame.size.height > px(0.));
        assert!(frame.contains(&marker.center()), "prompt marker must sit inside the composer frame");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "activating the composer focuses its input"
        );
    })
    .unwrap();
}
