use super::*;

/// Asserts the armed-swipe overlay names `expected` and exposes a status role.
fn assert_swipe_overlay(window: AnyWindowHandle, expected: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let overlay = window.find("swipe-navigation-overlay");
        assert_eq!(overlay.role(), Some(gpui_kit::Role::Status));
        assert_eq!(overlay.label(), Some(expected));
    })
    .unwrap();
}

fn assert_no_swipe_overlay(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("swipe-navigation-overlay").is_none(), "the armed overlay must unmount");
    })
    .unwrap();
}

#[gpui_kit::test]
fn scroll_crossing_arms_overlay_without_navigating(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    scroll_swipe(window, 70., 0., cx);
    assert!(requests.try_recv().is_err(), "a sub-threshold scroll must not navigate");
    assert_no_swipe_overlay(window, cx);

    scroll_swipe(window, 70., 0., cx);
    assert!(requests.try_recv().is_err(), "a threshold crossing arms instead of navigating");
    view.update(cx, |view, _| {
        assert_eq!(view.swipe_arming.pending_direction(), Some(NavigationDirection::Back));
    });
    assert_swipe_overlay(window, "Back to Thread swipe-first", cx);
}

#[gpui_kit::test]
fn finger_lift_commits_armed_scroll(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    scroll_swipe(window, 70., 0., cx);
    scroll_swipe(window, 70., 0., cx);
    scroll_swipe_with_phase(window, 0., 0., TouchPhase::Ended, cx);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("finger lift should commit the armed swipe");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-first" }));
    view.update(cx, |view, _| {
        assert_eq!(view.pending_navigation.map(|pending| pending.target_index), Some(0));
        assert!(view.swipe_arming.pending_direction().is_none());
    });
    assert_no_swipe_overlay(window, cx);
}

#[gpui_kit::test]
fn settle_timer_commits_discrete_swipe(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Forward, cx);
    assert!(requests.try_recv().is_err(), "a discrete swipe arms instead of navigating");
    assert_swipe_overlay(window, "Forward to Thread swipe-third", cx);

    settle_swipe_commit(cx);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("the settle timer should commit the armed swipe");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-third" }));
    view.update(cx, |view, _| {
        assert_eq!(view.pending_navigation.map(|pending| pending.target_index), Some(2));
    });
    assert_no_swipe_overlay(window, cx);
}

#[gpui_kit::test]
fn reversal_cancels_armed_swipe(cx: &mut TestAppContext) {
    let (window, _view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &_view, commands);

    scroll_swipe(window, 70., 0., cx);
    scroll_swipe(window, 70., 0., cx);
    scroll_swipe(window, -50., 0., cx);
    assert_no_swipe_overlay(window, cx);

    settle_swipe_commit(cx);
    scroll_swipe_with_phase(window, 0., 0., TouchPhase::Ended, cx);
    assert!(requests.try_recv().is_err(), "neither the timer nor the lift may commit a canceled arm");
}

#[gpui_kit::test]
fn vertical_scroll_cancels_armed_swipe(cx: &mut TestAppContext) {
    let (window, _view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &_view, commands);

    scroll_swipe(window, 70., 0., cx);
    scroll_swipe(window, 70., 0., cx);
    scroll_swipe(window, 10., 500., cx);
    assert_no_swipe_overlay(window, cx);

    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "a vertically canceled arm must never commit");
}

#[gpui_kit::test]
fn opposite_swipe_supersedes_pending_arm(cx: &mut TestAppContext) {
    let (window, _view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &_view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    swipe_navigate(window, NavigationDirection::Forward, cx);
    assert_swipe_overlay(window, "Forward to Thread swipe-third", cx);

    settle_swipe_commit(cx);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("the superseding arm should commit");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-third" }));
    assert!(requests.try_recv().is_err(), "the superseded arm must not commit alongside");
}

#[gpui_kit::test]
fn repeated_same_direction_swipe_navigates_once(cx: &mut TestAppContext) {
    let (window, _view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &_view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    swipe_navigate(window, NavigationDirection::Back, cx);
    assert_swipe_overlay(window, "Back to Thread swipe-first", cx);

    settle_swipe_commit(cx);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("the single arm should commit");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-first" }));
    assert!(requests.try_recv().is_err(), "the repeated swipe must not double-commit");
}

#[gpui_kit::test]
fn commit_cooldown_refuses_a_fresh_arm(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_ok(), "the first arm commits");
    // Resolve the load so the pending-transition gate cannot mask the
    // cooldown: forward stays available, only the cooldown refuses it.
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({
                    "session": summaries[0],
                    "messages": [{ "role": "assistant", "text": "First conversation" }],
                })),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert!(view.pending_navigation.is_none());
        assert!(view.swipe_direction_available(NavigationDirection::Forward));
    });

    swipe_navigate(window, NavigationDirection::Forward, cx);
    assert_no_swipe_overlay(window, cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "an arm inside the commit cooldown must not fire");
}

#[gpui_kit::test]
fn escape_cancels_armed_swipe(cx: &mut TestAppContext) {
    let (window, _view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &_view, commands);

    scroll_swipe(window, 70., 0., cx);
    scroll_swipe(window, 70., 0., cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("escape", cx);
    })
    .unwrap();
    assert_no_swipe_overlay(window, cx);

    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "an Escape-canceled arm must never commit");
}

#[gpui_kit::test]
fn palette_open_blocks_armed_commit(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
    })
    .unwrap();

    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "a commit into an open palette must fail closed");
    assert_no_swipe_overlay(window, cx);
}

#[gpui_kit::test]
fn overlay_falls_back_to_workspace_name_for_unloaded_thread(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);
    let workspace_name = std::env::temp_dir()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .unwrap();
    view.update(cx, |view, _| {
        view.sessions.retain(|session| session.session_id != "swipe-first");
    });

    swipe_navigate(window, NavigationDirection::Back, cx);
    assert_swipe_overlay(window, &format!("Back to {workspace_name}"), cx);
}

#[gpui_kit::test]
fn overlay_falls_back_to_bare_direction_without_target(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    let workspace = std::env::temp_dir();
    view.update(cx, |view, _| {
        view.preferences.swipe_navigation = true;
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(workspace.clone());
        view.selected_session_id = Some("only-session".into());
        view.navigation_history
            .record(NavigationLocation { workspace: PathBuf::from("/"), session_id: None });
        view.navigation_history
            .record(NavigationLocation { workspace, session_id: Some("only-session".into()) });
    });

    swipe_navigate(window, NavigationDirection::Back, cx);
    assert_swipe_overlay(window, "Back", cx);
}

/// Four-thread history with the cursor on the third entry, so a direct
/// back-step still leaves history behind for a stale arm to (wrongly) take.
fn four_thread_fixture(
    cx: &mut TestAppContext,
    view: &Entity<DesktopView>,
    commands: tokio::sync::mpsc::Sender<BackendCommand>,
) {
    let workspace = std::env::temp_dir();
    let summaries = ["swipe-one", "swipe-two", "swipe-three", "swipe-four"]
        .into_iter()
        .map(|id| vtcode_desktop_protocol::SessionSummary {
            session_id: id.into(),
            title: Some(format!("Thread {id}")),
            workspace: workspace.display().to_string(),
            completed: true,
            resumable: true,
            size_bytes: 0,
            updated_at: None,
        })
        .collect::<Vec<_>>();
    view.update(cx, |view, _| {
        view.preferences.swipe_navigation = true;
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(workspace.clone());
        view.sessions.clone_from(&summaries);
        view.selected_session_id = Some(summaries[2].session_id.clone());
        for summary in &summaries {
            view.navigation_history.record(NavigationLocation {
                workspace: workspace.clone(),
                session_id: Some(summary.session_id.clone()),
            });
        }
        assert!(view.navigation_history.navigate_to(2));
    });
}

#[gpui_kit::test]
fn direct_back_mid_arm_navigates_once(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    four_thread_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    assert_swipe_overlay(window, "Back to Thread swipe-two", cx);
    // Toolbar/keyboard entries funnel through `navigate_back`, bypassing the
    // arm layer: the direct step must retire the stale arm.
    view.update(cx, |view, cx| view.navigate_back(cx));
    assert_no_swipe_overlay(window, cx);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("the direct step should load the previous thread");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-two" }));

    // Resolve the direct step before the stale timer fires: without
    // retirement, the timer would take the still-available back entry.
    view.update(cx, |view, cx| {
        let target = view.sessions.iter().find(|s| s.session_id == "swipe-two").unwrap().clone();
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({ "session": target, "messages": [] })),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "the retired arm must not commit a second step");
    view.update(cx, |view, _| {
        assert_eq!(view.selected_session_id.as_deref(), Some("swipe-two"));
    });
}

#[gpui_kit::test]
fn history_record_mid_arm_retires_the_arm(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    view.update(cx, |view, _| {
        // Settle onto a new thread first: recording the unchanged cursor
        // location is a no-op, so the mutation must move the selection.
        view.selected_session_id = Some("swipe-third".into());
        view.record_current_navigation();
        assert!(view.swipe_arming.pending_direction().is_none(), "recording must retire the arm");
        assert_eq!(view.navigation_history.current_index(), Some(2));
    });
    assert_no_swipe_overlay(window, cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "a retired arm must never commit");
    view.update(cx, |view, _| {
        assert_eq!(view.navigation_history.current_index(), Some(2), "no commit moved the cursor");
    });
}

#[gpui_kit::test]
fn tab_settle_response_mid_arm_cancels(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.load_session_from_ui("swipe-third".into(), window, cx));
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_ok(), "the tab click loads the third thread");
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({ "session": summaries[2], "messages": [] })),
                error: None,
            },
            cx,
        );
    });
    // The load settles through an async decode task; the record (and the
    // arm retirement) lands when it drains.
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert!(view.swipe_arming.pending_direction().is_none(), "the settling record must retire the arm");
        assert_eq!(view.selected_session_id.as_deref(), Some("swipe-third"));
    });
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "only the tab-click load may have issued");
}

#[gpui_kit::test]
fn failed_commit_starts_no_cooldown(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);
    let workspace_name = std::env::temp_dir()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.trim().is_empty())
        .unwrap();
    view.update(cx, |view, _| {
        view.sessions.retain(|session| session.session_id != "swipe-first");
    });

    swipe_navigate(window, NavigationDirection::Back, cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "a missing target thread must fail the commit");
    view.update(cx, |view, _| {
        assert!(view.error.is_some(), "the failed commit surfaces thread_unavailable");
    });

    // The failed commit burned no cooldown: an immediate retry re-arms.
    swipe_navigate(window, NavigationDirection::Back, cx);
    assert_swipe_overlay(window, &format!("Back to {workspace_name}"), cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "the retry fails the same way, still without cooldown");
}

#[gpui_kit::test]
fn lift_after_settle_commits_nothing(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_ok(), "the arm commits");
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({
                    "session": summaries[0],
                    "messages": [{ "role": "assistant", "text": "First conversation" }],
                })),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    scroll_swipe_with_phase(window, 0., 0., TouchPhase::Ended, cx);
    assert!(requests.try_recv().is_err(), "a lift after commit must not recommit");
    assert_no_swipe_overlay(window, cx);
}

#[gpui_kit::test]
fn handoff_link_mid_arm_cancels(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    swipe_navigate(window, NavigationDirection::Back, cx);
    view.update(cx, |view, cx| {
        view.arm_session_link(std::env::temp_dir(), "ghost-session".into(), cx);
        assert!(view.swipe_arming.pending_direction().is_none(), "a handoff must retire the arm");
    });
    assert_no_swipe_overlay(window, cx);
}

#[gpui_kit::test]
fn swipe_setting_lifecycle(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let _summaries = swipe_history_fixture(cx, &view, commands);

    assert!(!crate::desktop::preferences::Preferences::default().swipe_navigation, "swipe ships off");
    view.update(cx, |view, _| view.preferences.swipe_navigation = false);
    swipe_navigate(window, NavigationDirection::Back, cx);
    scroll_swipe(window, 140., 0., cx);
    assert_no_swipe_overlay(window, cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "disabled swipe never arms");

    view.update(cx, |view, cx| view.toggle_swipe_navigation(cx));
    swipe_navigate(window, NavigationDirection::Back, cx);
    assert_swipe_overlay(window, "Back to Thread swipe-first", cx);

    // Disabling mid-arm retires the pill immediately.
    view.update(cx, |view, cx| view.toggle_swipe_navigation(cx));
    assert_no_swipe_overlay(window, cx);
    settle_swipe_commit(cx);
    assert!(requests.try_recv().is_err(), "the retired arm never commits");

    view.update(cx, |view, cx| view.toggle_swipe_navigation(cx));
    swipe_navigate(window, NavigationDirection::Back, cx);
    assert_swipe_overlay(window, "Back to Thread swipe-first", cx);

    // Toolbar and keyboard entries bypass the gate and keep working,
    // even while swipe itself is disabled.
    view.update(cx, |view, cx| view.toggle_swipe_navigation(cx));
    view.update(cx, |view, cx| view.navigate_back(cx));
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("direct history navigation bypasses the swipe gate");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "swipe-first" }));
    assert!(requests.try_recv().is_err(), "only the direct step issued");
}

#[gpui_kit::test]
fn swipe_switch_toggles_and_persists(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        select_settings_category(window, 3, cx);
        window.render_frame(cx);
        assert!(window.try_find("swipe-navigation").is_some(), "switch renders in the shortcuts tab");
        scroll_settings_by(window, px(-6000.), cx);
        window.render_frame(cx);
        window.click("swipe-navigation", cx);
        assert!(view.read(cx).preferences.swipe_navigation, "switch turns swipe on");
        window.click("swipe-navigation", cx);
        assert!(!view.read(cx).preferences.swipe_navigation, "switch turns swipe off");
    })
    .unwrap();
    assert!(
        !crate::desktop::preferences::Preferences::load().swipe_navigation,
        "the persisted toggle survives reload"
    );
}
