use super::*;

fn storage_summary_fixture() -> vtcode_desktop_protocol::StorageSummary {
    vtcode_desktop_protocol::StorageSummary {
        state_dir: "/state".into(),
        workspace: "/workspace".into(),
        sessions_bytes: 30,
        sessions_count: 2,
        worktrees_bytes: 5,
        worktrees_count: 1,
        memory_bytes: 7,
        other_bytes: 40,
        total_bytes: 82,
    }
}

fn review_session_fixture(
    id: &str,
    size_bytes: u64,
    updated_at: Option<u64>,
) -> vtcode_desktop_protocol::SessionSummary {
    vtcode_desktop_protocol::SessionSummary {
        session_id: id.into(),
        title: Some(format!("Thread {id}")),
        workspace: "/workspace".into(),
        completed: true,
        resumable: true,
        size_bytes,
        updated_at,
    }
}

fn open_storage_tab(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<DesktopView>, tokio::sync::mpsc::Receiver<BackendCommand>) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.press("cmd-f", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("Storage", cx);
        window.render_frame(cx);
        let action = super::super::PaletteAction::SelectSettingsTab(super::super::settings::SettingsTab::Storage);
        window.within("command-palette").click(palette_action_id(&action), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| settle_settings_page(window, cx))
        .unwrap();
    (window, view, requests)
}

/// Guard that the reopened settings page sits fully on-screen after one
/// frame settles fades and scroll geometry.
fn settle_settings_page(window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    window.render_frame(cx);
    let viewport = window.viewport_size();
    let panel = window.find("settings-panel").bounds();
    assert!(
        panel.origin.x + panel.size.width <= viewport.width,
        "reopened page sits fully on-screen: {panel:?} in {viewport:?}"
    );
}

#[gpui_kit::test]
fn storage_keyboard_navigation_requests_summary_once(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        for _ in 0..6 {
            window.dispatch_action(Box::new(super::super::NextSettingsTab), cx);
            window.render_frame(cx);
        }
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Storage);
    });
    drain_summary_request(&mut requests);
    assert!(requests.try_recv().is_err(), "summary requested once while loading");
}

#[gpui_kit::test]
fn storage_reopened_page_settles_fully_on_screen(cx: &mut TestAppContext) {
    let (window, view, _requests) = open_storage_tab(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Storage);
        let viewport = window.viewport_size();
        let review = window.find("storage-review").bounds();
        assert!(
            review.origin.x + review.size.width <= viewport.width,
            "right-edge controls stay clickable: {review:?} in {viewport:?}"
        );
    })
    .unwrap();
}

fn drain_request(requests: &mut tokio::sync::mpsc::Receiver<BackendCommand>, method: &str) {
    let BackendCommand::Request { method: actual, .. } =
        requests.try_recv().unwrap_or_else(|_| panic!("expected {method} request"))
    else {
        panic!("expected {method} request");
    };
    assert_eq!(actual, method);
}

fn drain_summary_request(requests: &mut tokio::sync::mpsc::Receiver<BackendCommand>) {
    drain_request(requests, vtcode_desktop_protocol::method::STORAGE_SUMMARY);
}

#[gpui_kit::test]
fn storage_tab_requests_summary_and_renders_usage(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_storage_tab(cx);
    drain_summary_request(&mut requests);
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.storage.loading);
        assert!(view.storage.summary.is_none());
    });

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::STORAGE_SUMMARY.into(),
                result: Some(serde_json::to_value(storage_summary_fixture()).unwrap()),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("storage-refresh").label(), Some("Refresh"));
        let bar_snapshot = window.find("storage-usage-bar");
        let bar = bar_snapshot.label().expect("usage bar label");
        assert!(bar.contains("Sessions 30 bytes"), "{bar}");
        assert!(bar.contains("Worktrees 5 bytes"), "{bar}");
        assert!(bar.contains("Session memory 7 bytes"), "{bar}");
        assert!(bar.contains("Other 40 bytes"), "{bar}");
        assert_eq!(window.find("storage-review").label(), Some("Clean up old chats"));
        assert_eq!(window.find("storage-manage-worktrees").label(), Some("Manage worktrees"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn storage_refresh_resends_the_summary_request(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_storage_tab(cx);
    drain_summary_request(&mut requests);
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::STORAGE_SUMMARY.into(),
                result: Some(serde_json::to_value(storage_summary_fixture()).unwrap()),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("storage-refresh").label(), Some("Refresh"));
        window.click("storage-refresh", cx);
        window.render_frame(cx);
    })
    .unwrap();
    drain_summary_request(&mut requests);
    cx.update(|cx| assert!(view.read(cx).storage.loading));
}

#[gpui_kit::test]
fn storage_review_lists_oldest_first_and_marks_the_current_session(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_storage_tab(cx);
    drain_summary_request(&mut requests);
    view.update(cx, |view, _| {
        view.sessions = vec![
            review_session_fixture("new", 20, Some(200)),
            review_session_fixture("current", 10, Some(150)),
            review_session_fixture("old", 30, Some(100)),
        ];
        view.selected_session_id = Some("current".into());
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("storage-review", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).storage.view,
            super::super::storage::StorageView::ReviewSessions,
            "review click switched views"
        );
    });
    drain_request(&mut requests, vtcode_desktop_protocol::method::SESSION_LIST);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("delete-session-old").is_some());
        assert!(window.try_find("delete-session-new").is_some());
        assert!(window.try_find("delete-session-current").is_none());
        assert_eq!(window.find("delete-session-old").label(), Some("Delete \"Thread old\"?"));
        let old_top = window.find("delete-session-old").bounds().origin.y;
        let new_top = window.find("delete-session-new").bounds().origin.y;
        assert!(old_top < new_top, "oldest session leads the review list");
    })
    .unwrap();
}

#[gpui_kit::test]
fn storage_delete_response_removes_the_row_and_refreshes_usage(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_storage_tab(cx);
    drain_summary_request(&mut requests);
    view.update(cx, |view, _| {
        view.sessions = vec![
            review_session_fixture("gone", 30, Some(100)),
            review_session_fixture("kept", 20, Some(200)),
        ];
    });
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_DELETE.into(),
                result: Some(json!({ "sessionId": "gone", "deleted": true })),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let ids: Vec<_> = view
            .read(cx)
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(ids, ["kept"]);
    });
    drain_summary_request(&mut requests);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn storage_worktrees_list_and_remove_entries(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_storage_tab(cx);
    drain_summary_request(&mut requests);
    cx.update_window(window, |_, window, cx| {
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-3000.))), cx);
        window.render_frame(cx);
        window.click("storage-manage-worktrees", cx);
        window.render_frame(cx);
    })
    .unwrap();
    drain_request(&mut requests, vtcode_desktop_protocol::method::WORKTREE_LIST);

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WORKTREE_LIST.into(),
                result: Some(json!([
                    { "name": "loop-dirty", "head": "abc1234567890", "isDirty": true },
                    { "name": "loop-clean", "head": "def9876", "isDirty": false },
                ])),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("remove-worktree-loop-dirty").label(), Some("Remove \"loop-dirty\"?"));
        assert_eq!(window.find("remove-worktree-loop-clean").label(), Some("Remove \"loop-clean\"?"));
    })
    .unwrap();

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WORKTREE_REMOVE.into(),
                result: Some(json!({ "name": "loop-dirty", "removed": true })),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let names: Vec<_> = view
            .read(cx)
            .storage
            .worktrees
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["loop-clean"]);
    });
    drain_summary_request(&mut requests);
}

#[gpui_kit::test]
fn storage_error_shows_retry_and_recovers_on_demand(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_storage_tab(cx);
    drain_summary_request(&mut requests);
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::STORAGE_SUMMARY.into(),
                result: None,
                error: Some("disk walk failed".into()),
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(!view.storage.loading);
        assert_eq!(view.storage.error.as_deref(), Some("disk walk failed"));
        assert!(view.error.is_none(), "storage errors stay scoped to the tab");
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("storage-retry").label(), Some("Retry"));
        assert!(requests.try_recv().is_err(), "no second request before the retry click");
        window.click("storage-retry", cx);
        window.render_frame(cx);
    })
    .unwrap();
    drain_summary_request(&mut requests);
}

#[gpui_kit::test]
fn storage_back_returns_to_the_summary(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_storage_tab(cx);
    drain_summary_request(&mut requests);
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::STORAGE_SUMMARY.into(),
                result: Some(serde_json::to_value(storage_summary_fixture()).unwrap()),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("storage-usage-bar").is_some());
        window.click("storage-review", cx);
        window.render_frame(cx);
        assert!(window.try_find("storage-review-view").is_some());
        assert!(window.try_find("storage-usage-bar").is_none());
    })
    .unwrap();
    drain_request(&mut requests, vtcode_desktop_protocol::method::SESSION_LIST);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("storage-back", cx);
        window.render_frame(cx);
        assert!(window.try_find("storage-settings").is_some());
        assert!(window.try_find("storage-usage-bar").is_some());
    })
    .unwrap();
}
