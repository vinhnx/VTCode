use super::*;

#[gpui_kit::test]
fn session_link_arms_navigation_and_loads_on_discovery(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().unwrap();
    let canonical = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonicalize");
    let target = vtcode_desktop_protocol::SessionSummary {
        session_id: "linked-session".into(),
        title: Some("Linked".into()),
        workspace: canonical.display().to_string(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    };
    let (_window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, cx| {
        view.arm_session_link(canonical.clone(), "linked-session".into(), cx);
        assert!(view.workspace_selection_pending, "arming starts the workspace switch");
        let pending = view.pending_navigation.expect("startup link arms pending navigation");
        assert!(pending.workspace_switch);
        let recorded = view
            .navigation_history
            .location(pending.target_index)
            .expect("startup link records its target");
        assert_eq!(recorded.workspace, canonical);
        assert_eq!(recorded.session_id.as_deref(), Some("linked-session"));
        // Bypass the async workspace switch: selection completed and the
        // backend already matches the target.
        view.workspace_selection_pending = false;
        view.workspace = Some(canonical);
        view.backend = Some(BackendHandle { commands });
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LIST.into(),
                result: Some(json!([target])),
                error: None,
            },
            cx,
        );
        assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
    });
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("session discovery should be followed by the linked load");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "linked-session" }));
}

#[gpui_kit::test]
fn handoff_link_in_current_workspace_loads_directly(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let workspace = tempfile::TempDir::new().unwrap();
    let canonical = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonicalize");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(canonical.clone());
            view.backend = Some(BackendHandle { commands });
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "linked-session".into(),
                    workspace: canonical,
                },
                window,
                cx,
            );
            assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
            assert!(view.pending_navigation.is_none(), "no list wait on the fast path");
        });
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("same-workspace handoff should load immediately");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "linked-session" }));
}

#[gpui_kit::test]
fn contended_handoff_load_surfaces_the_close_and_reload_error(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let workspace = tempfile::TempDir::new().unwrap();
    let canonical = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonicalize");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(canonical.clone());
            view.backend = Some(BackendHandle { commands });
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "live-session".into(),
                    workspace: canonical,
                },
                window,
                cx,
            );
            assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
        });
    })
    .unwrap();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("same-workspace handoff should load immediately");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);

    // The service fail-fast probe rejects the load while the CLI holds the
    // session; the banner must carry the remedy, not a generic failure.
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: None,
                error: Some(
                    "session live-session is already open in another process; close it there and reload".into(),
                ),
            },
            cx,
        );
        assert_eq!(
            view.error.as_deref(),
            Some("session live-session is already open in another process; close it there and reload")
        );
        assert!(view.session_request.is_none(), "a failed load must not block retry");
        assert!(view.backend.is_some(), "the service connection survives a rejected load");
    });
}

#[gpui_kit::test]
fn handoff_link_in_other_workspace_arms_pending_navigation(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let current = tempfile::TempDir::new().unwrap();
    let other = tempfile::TempDir::new().unwrap();
    let current_canonical = vtcode_commons::paths::canonicalize(current.path()).expect("canonicalize");
    let other_canonical = vtcode_commons::paths::canonicalize(other.path()).expect("canonicalize");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(current_canonical);
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "linked-session".into(),
                    workspace: other_canonical.clone(),
                },
                window,
                cx,
            );
            let pending = view.pending_navigation.expect("cross-workspace handoff arms navigation");
            assert!(pending.workspace_switch);
            let target = view
                .navigation_history
                .location(pending.target_index)
                .expect("handoff records its target");
            assert_eq!(target.workspace, other_canonical);
            assert_eq!(view.session_request, None, "load waits for discovery after the switch");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn handoff_link_with_missing_workspace_reports_invalid_folder(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_handoff_link(
                HandoffLink {
                    session_id: "linked-session".into(),
                    workspace: PathBuf::from("/missing/workspace"),
                },
                window,
                cx,
            );
            assert_eq!(view.error.as_deref(), Some("Choose an existing workspace folder."));
            assert!(view.pending_navigation.is_none());
        });
    })
    .unwrap();
}
