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
fn handoff_waits_out_transient_gate_contention_then_loads(cx: &mut TestAppContext) {
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
        // Bypass the async workspace switch; simulate an in-flight
        // permission restore holding the transition gate.
        view.workspace_selection_pending = false;
        view.workspace = Some(canonical);
        view.backend = Some(BackendHandle { commands });
        view.session_request = Some(vtcode_desktop_protocol::method::CONFIG_OPTIONS);
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LIST.into(),
                result: Some(json!([target])),
                error: None,
            },
            cx,
        );
        assert!(requests.try_recv().is_err(), "contended gate must not send the load yet");
        assert!(view.pending_navigation.is_some(), "transient refusal must keep pending navigation");
        // The restore lands: the freed gate retries and fires the load.
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                result: Some(json!({
                    "provider": "probe",
                    "model": "probe",
                    "authenticated": true,
                    "credentialSetupCommand": "vtcode login",
                })),
                error: None,
            },
            cx,
        );
        assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
    });
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("freed gate should retry the linked load");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "linked-session" }));
}

#[gpui_kit::test]
fn handoff_survives_a_failed_restore_without_a_stale_banner(cx: &mut TestAppContext) {
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
        view.workspace_selection_pending = false;
        view.workspace = Some(canonical);
        view.backend = Some(BackendHandle { commands });
        view.session_request = Some(vtcode_desktop_protocol::method::CONFIG_OPTIONS);
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LIST.into(),
                result: Some(json!([target])),
                error: None,
            },
            cx,
        );
        assert!(view.pending_navigation.is_some(), "contended handoff stays armed");
        // The restore fails: the freed gate still retries the load, and the
        // load send clears the failure banner instead of leaving it stale.
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                result: None,
                error: Some("restore failed".to_string()),
            },
            cx,
        );
        assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
        assert!(view.error.is_none(), "retried load must clear the restore failure banner");
    });
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("failed restore should still retry the linked load");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "linked-session" }));
}

#[gpui_kit::test]
fn handoff_fails_fast_on_durable_blocks(cx: &mut TestAppContext) {
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
        view.workspace_selection_pending = false;
        view.workspace = Some(canonical);
        view.backend = Some(BackendHandle { commands });
        // A running turn blocks the gate with no response coming to free
        // it: the handoff must fail fast, not wait forever.
        view.running = true;
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LIST.into(),
                result: Some(json!([target])),
                error: None,
            },
            cx,
        );
        assert!(view.pending_navigation.is_none(), "durable block must fail the handoff fast");
        assert!(requests.try_recv().is_err(), "failed handoff must not send the load");
    });
}

#[gpui_kit::test]
fn handoff_waits_for_the_switch_and_the_list(cx: &mut TestAppContext) {
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
        // Mid-switch retry (e.g. a stale gated response): wait silently
        // instead of failing on the stale pre-switch workspace.
        view.workspace_selection_pending = true;
        view.continue_pending_workspace_navigation(cx);
        assert!(view.pending_navigation.is_some(), "in-flight switch must keep pending navigation");
        assert!(view.error.is_none(), "waiting must not raise a spurious error");
        // Switch done, list still in flight: wait rather than judge
        // membership against an empty cache.
        view.workspace_selection_pending = false;
        view.workspace = Some(canonical);
        view.continue_pending_workspace_navigation(cx);
        assert!(view.pending_navigation.is_some(), "missing list must keep pending navigation");
        assert!(view.error.is_none(), "waiting must not raise a spurious error");
        // The list lands and completes the handoff.
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
        panic!("landed list should complete the waiting handoff");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    assert_eq!(params, json!({ "sessionId": "linked-session" }));
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

#[gpui_kit::test]
fn windowless_handoff_reopens_a_window_instead_of_dropping(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let (window, _view) = open_desktop(cx);
    cx.update_window(window, |_, window, _| window.remove_window()).unwrap();
    cx.update(|cx| assert!(cx.windows().is_empty(), "precondition: no live window"));
    cx.update(|cx| {
        super::super::dispatch_handoff_links(
            cx,
            vec![HandoffLink {
                session_id: "linked-session".into(),
                workspace: PathBuf::from("/missing/workspace"),
            }],
        );
        assert_eq!(cx.windows().len(), 1, "handoff must reopen a window to land on");
    });
}

#[gpui_kit::test]
fn warm_handoff_reuses_the_existing_window(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::handoff_url::HandoffLink;

    let (window, _view) = open_desktop(cx);
    cx.update(|cx| {
        super::super::dispatch_handoff_links(
            cx,
            vec![HandoffLink {
                session_id: "linked-session".into(),
                workspace: PathBuf::from("/missing/workspace"),
            }],
        );
        assert_eq!(cx.windows(), vec![window], "handoff must not duplicate the window");
    });
}

#[gpui_kit::test]
fn empty_handoff_opens_no_window(cx: &mut TestAppContext) {
    let (window, _view) = open_desktop(cx);
    cx.update_window(window, |_, window, _| window.remove_window()).unwrap();
    cx.update(|cx| {
        super::super::dispatch_handoff_links(cx, Vec::new());
        assert!(cx.windows().is_empty(), "no links must not spawn a window");
    });
}
