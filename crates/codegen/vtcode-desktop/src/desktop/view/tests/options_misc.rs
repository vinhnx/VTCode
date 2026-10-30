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

        assert!(view.read(cx).settings_open, "the Settings shortcut opens the dialog");
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
