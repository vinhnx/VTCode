//! Handoff notifications: a thread arriving from the CLI announces itself,
//! and clicking the notice lands on that thread.

use super::*;

/// A thread summary for the sidebar, standing in for the listed session the
/// handoff names.
fn handoff_thread_fixture(id: &str) -> vtcode_desktop_protocol::SessionSummary {
    vtcode_desktop_protocol::SessionSummary {
        session_id: id.into(),
        title: Some(format!("Thread {id}")),
        workspace: std::env::temp_dir().display().to_string(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    }
}

/// Deliver the one posted system banner and settle the click it carries.
fn click_delivered_system_banner(cx: &mut TestAppContext) {
    let delivered = cx.delivered_system_notifications();
    assert_eq!(delivered.len(), 1, "one banner under test");
    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag: delivered[0].tag.clone(),
        action_id: None,
    });
    cx.run_until_parked();
}

/// A CLI handoff announces itself: a backgrounded window gets a system
/// banner, and clicking it lands on the handed-off thread.
#[gpui_kit::test]
fn handoff_link_announces_the_thread_and_the_click_lands_on_it(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let workspace = tempfile::TempDir::new().unwrap();
    let canonical = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonicalize");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        assert!(!window.is_window_active(), "a backgrounded window gets the system banner");
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(canonical.clone());
            view.sessions = vec![handoff_thread_fixture("thread-a")];
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "thread-a".into(),
                    workspace: canonical,
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    assert_eq!(cx.shown_system_notifications().len(), 1, "the handoff posts one banner");

    // The handoff already asked for the thread, so the click must not
    // dispatch a second load; it only brings the thread surface forward.
    let BackendCommand::Request { method, params } = requests.try_recv().expect("the handoff loads the thread") else {
        panic!("the handoff must send session/load");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "thread-a" }));

    click_delivered_system_banner(cx);
    assert!(requests.try_recv().is_err(), "the in-flight handoff load already covers the clicked thread");
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).current_route() == Route::Workspace, "click lands on the thread surface");
    })
    .unwrap();
}

/// A handoff archive carries a fresh identifier the sidebar does not know
/// yet, so the notice must not leak that opaque id into user-facing text.
#[gpui_kit::test]
fn handoff_notice_names_no_session_id(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let workspace = tempfile::TempDir::new().unwrap();
    let canonical = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonicalize");
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    let fork_id = "session-vtcode-20261111T100433Z_574120-86184";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(canonical.clone());
            // The fork is not in the sidebar list yet, exactly as it is on
            // arrival.
            view.sessions = Vec::new();
            view.handle_handoff_link(HandoffLink { session_id: fork_id.into(), workspace: canonical }, window, cx);
        });
    })
    .unwrap();
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 1, "the handoff posts one banner");
    assert!(!shown[0].body.contains(fork_id), "the notice must not show an opaque session id: {}", shown[0].body);
}

/// A focused window toasts the handoff in-app instead of escaping to the
/// OS center, where the click target lives.
#[gpui_kit::test]
fn handoff_link_toasts_in_app_when_the_window_is_focused(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let workspace = tempfile::TempDir::new().unwrap();
    let canonical = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonicalize");
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    // A banner click activates the window headless, so the first handoff
    // runs unfocused and the second one toasts in-app.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(canonical.clone());
            view.sessions = vec![handoff_thread_fixture("thread-a")];
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "thread-a".into(),
                    workspace: canonical.clone(),
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    click_delivered_system_banner(cx);
    cx.update_window(window, |_, window, _| {
        assert!(window.is_window_active());
    })
    .unwrap();

    let shown_before = cx.shown_system_notifications().len();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "thread-a".into(),
                    workspace: canonical,
                },
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.notifications(cx).len(), 1, "a focused handoff toasts in-app where its click target lives");
    })
    .unwrap();
    assert_eq!(
        cx.shown_system_notifications().len(),
        shown_before,
        "a focused handoff must not also escape to the OS center"
    );
}

/// The handoff notice follows the master notification toggle: a user who
/// turned notifications off must not be interrupted by a CLI handoff.
#[gpui_kit::test]
fn handoff_link_stays_silent_when_notifications_are_disabled(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let workspace = tempfile::TempDir::new().unwrap();
    let canonical = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonicalize");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.notifications.enabled = Some(false);
            view.backend = Some(BackendHandle { commands: tokio::sync::mpsc::channel(4).0 });
            view.workspace = Some(canonical.clone());
            view.sessions = vec![handoff_thread_fixture("thread-a")];
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "thread-a".into(),
                    workspace: canonical,
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    assert!(
        cx.shown_system_notifications().is_empty(),
        "a disabled notification master toggle silences the handoff"
    );
}

/// The gate is a pure predicate: a handoff is user-initiated, so only the
/// master toggle silences it (focus never does).
#[test]
fn handoff_delivery_follows_the_master_toggle_only() {
    use crate::desktop::notifications::handoff_allowed;
    use crate::desktop::preferences::NotificationPreferences;

    assert!(handoff_allowed(&NotificationPreferences::default()));
    let disabled = NotificationPreferences { enabled: Some(false), ..Default::default() };
    assert!(!handoff_allowed(&disabled));
    // Per-category toggles govern agent activity, not handoffs.
    let quiet = NotificationPreferences {
        permission_prompts: Some(false),
        questions: Some(false),
        updates: Some(false),
        ..Default::default()
    };
    assert!(handoff_allowed(&quiet));
}
