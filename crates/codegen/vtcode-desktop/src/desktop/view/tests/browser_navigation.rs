use super::*;

fn browser_fixture(id: &str, workspace: &str, updated_at: Option<u64>) -> vtcode_desktop_protocol::SessionSummary {
    vtcode_desktop_protocol::SessionSummary {
        session_id: id.into(),
        title: Some(format!("Thread {id}")),
        workspace: workspace.into(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at,
    }
}

/// Asymmetric threads across two workspaces (2 vs 1), seeded out of order so
/// the test pins the browser's own sort: current workspace first, then
/// newest-first within each group.
fn seed_browser_threads(view: &Entity<DesktopView>, cx: &mut gpui_kit::App) {
    view.update(cx, |view, _| {
        view.workspace = Some(PathBuf::from("/work/beta"));
        view.sessions = vec![
            browser_fixture("b-only", "/work/beta", Some(20)),
            browser_fixture("a-old", "/work/alpha", Some(10)),
            browser_fixture("a-new", "/work/alpha", Some(30)),
        ];
    });
}

#[gpui_kit::test]
fn browser_arrows_follow_visual_order_paint_rail_and_clamp(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_browser_threads(&view, cx);
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).browser_list_focus.is_focused(window), "opening lands in the list");
        // Visual order: current workspace (/work/beta) first, then alpha newest-first.
        let row = window.find("browser-session-row-a-new").bounds();
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("b-only"));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-new"));
        window.render_frame(cx);
        assert!(window.try_find("browser-focus-rail-a-new").is_some(), "highlight paints a rail");
        assert_eq!(window.find("browser-session-row-a-new").bounds(), row, "highlight must not reflow the row");
        let rail = window.find("browser-focus-rail-a-new").bounds();
        assert_eq!(rail.origin, row.origin, "rail starts at the row's top-left");
        assert_eq!(rail.size.height, row.size.height, "rail spans exactly the row");
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-old"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("a-old"),
            "navigation clamps at the last thread"
        );
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-new"));
        window.render_frame(cx);
        assert!(window.try_find("browser-focus-rail-a-old").is_none(), "rail follows the highlight");
        window.press("up", cx);
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("b-only"));
        // A stale id (list refreshed underneath) restarts from the edge.
        view.update(cx, |view, _| view.keyboard_focused_session = Some("thread-gone".into()));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("b-only"));
        view.update(cx, |view, _| view.keyboard_focused_session = None);
        window.press("up", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("a-old"),
            "up from nothing starts at the last thread"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn browser_home_end_jump_to_edges_and_empty_browser_is_inert(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_browser_threads(&view, cx);
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        window.press("end", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-old"));
        window.press("home", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("b-only"));
        // Empty browser: no list renders, focus keeps the panel, keys no-op.
        view.update(cx, |view, _| {
            view.sessions.clear();
            view.keyboard_focused_session = None;
        });
        window.render_frame(cx);
        assert!(window.try_find("thread-browser-list").is_none());
        view.update(cx, |view, cx| view.focus_route(Route::Browser, window, cx));
        assert!(view.read(cx).browser_focus.is_focused(window));
        for key in ["down", "up", "home", "end", "enter", "backspace"] {
            window.press(key, cx);
            assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), None, "{key} on an empty browser");
        }
        assert!(view.read(cx).current_route() == Route::Browser, "enter must not dismiss");
    })
    .unwrap();
}

#[gpui_kit::test]
fn browser_enter_loads_highlighted_thread_exactly_once(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
        });
        seed_browser_threads(&view, cx);
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        window.press("down", cx);
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-new"));
        window.press("enter", cx);
        assert_eq!(view.read(cx).session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "enter dismisses the browser");
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "enter lands focus in the composer"
        );
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("a-new"),
            "the highlight survives as position memory"
        );
    })
    .unwrap();
    let first = requests.try_recv().expect("enter sends one session/load");
    match first {
        BackendCommand::Request { method, params } => {
            assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
            assert_eq!(params.get("sessionId").and_then(|id| id.as_str()), Some("a-new"));
        }
        BackendCommand::Stop => panic!("enter must send a load request, not stop"),
    }
    assert!(requests.try_recv().is_err(), "enter must not double-dispatch the load");
}

#[gpui_kit::test]
fn browser_click_anchors_highlight_as_position_memory(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
        });
        seed_browser_threads(&view, cx);
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        window.click("browser-session-a-old", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-old"));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "click loads and dismisses");
        // Reopening paints the rail from the anchored position memory.
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("browser-focus-rail-a-old").is_some(), "reopen paints the anchored rail");
        assert!(window.try_find("browser-focus-rail-a-new").is_none());
    })
    .unwrap();
    let first = requests.try_recv().expect("click sends one session/load");
    match first {
        BackendCommand::Request { method, params } => {
            assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
            assert_eq!(params.get("sessionId").and_then(|id| id.as_str()), Some("a-old"));
        }
        BackendCommand::Stop => panic!("click must send a load request, not stop"),
    }
    assert!(requests.try_recv().is_err(), "click must not double-dispatch the load");
}

#[gpui_kit::test]
fn browser_list_is_one_tab_stop_rows_are_skipped(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_browser_threads(&view, cx);
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).browser_list_focus.is_focused(window));
        for _ in 0..6 {
            window.press("tab", cx);
            window.render_frame(cx);
            for id in [
                "browser-session-b-only",
                "browser-session-a-new",
                "browser-session-a-old",
            ] {
                assert!(
                    !window.try_find(id).is_some_and(|element| element.focused() == Some(true)),
                    "{id} stays out of Tab order"
                );
            }
        }
        // The header stays reachable: Tab cycles back around to its
        // always-enabled dismiss button (New/Close thread stay disabled
        // without a backend, so they are not tab stops here).
        focus_element_by_tab(window, "browser-dismiss", cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn browser_vim_motions_follow_visual_order(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
        seed_browser_threads(&view, cx);
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        window.press("j", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("b-only"));
        window.press("j", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-new"));
        window.press("k", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("b-only"));
        window.press("shift-g", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("a-old"));
        window.press("g", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("b-only"));
    })
    .unwrap();
}
