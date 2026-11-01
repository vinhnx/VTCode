use super::*;
use vtcode_desktop_protocol::SessionSummary;

fn summary(session_id: &str, title: &str) -> SessionSummary {
    SessionSummary {
        session_id: session_id.into(),
        title: Some(title.into()),
        workspace: std::env::temp_dir().display().to_string(),
        completed: true,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    }
}

fn open_threaded_desktop(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<DesktopView>, tokio::sync::mpsc::Receiver<BackendCommand>) {
    let (window, view) = open_desktop(cx);
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(std::env::temp_dir());
        view.sessions = vec![
            summary("thread-a", "First thread"),
            summary("thread-b", "Second thread"),
            summary("thread-c", "Third thread"),
        ];
        view.selected_session_id = Some("thread-b".into());
        view.open_tabs = vec!["thread-a".into(), "thread-b".into(), "thread-c".into()];
    });
    (window, view, requests)
}

fn load_request_for(requests: &mut tokio::sync::mpsc::Receiver<BackendCommand>) -> String {
    let BackendCommand::Request { method, params } = requests.try_recv().expect("tab click loads the thread") else {
        panic!("expected a session request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    params["sessionId"].as_str().expect("load carries a session id").to_string()
}

#[gpui_kit::test]
fn thread_tabs_render_and_switch_on_click(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_some());
        assert_eq!(window.find("thread-tab-thread-a").label(), Some("Switch to thread: First thread"));
        assert_eq!(window.find("thread-tab-thread-b").label(), Some("Current thread: Second thread"));
        assert_eq!(window.find("thread-tab-thread-b").checked(), Some(true), "the selected tab reads as current");
        assert_eq!(window.find("thread-tab-thread-a").checked(), Some(false));
        assert!(window.try_find("thread-tab-new").is_some());

        window.click("thread-tab-thread-a", cx);
        assert_eq!(load_request_for(&mut requests), "thread-a");
        assert!(requests.try_recv().is_err(), "one click sends one load");
        assert_eq!(view.read(cx).open_tabs.as_slice(), ["thread-a", "thread-b", "thread-c"]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn closing_tabs_untabs_without_closing_sessions(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Closing the selected tab loads the next neighbor first.
        window.click("thread-tab-close-thread-b", cx);
        assert_eq!(load_request_for(&mut requests), "thread-c");
        assert_eq!(view.read(cx).open_tabs.as_slice(), ["thread-a", "thread-c"]);
        assert_eq!(view.read(cx).sessions.len(), 3, "untabbing keeps the session");

        // Non-selected tabs close without a load (the first load is still in flight).
        window.click("thread-tab-close-thread-c", cx);
        assert!(requests.try_recv().is_err(), "closing an unselected tab sends no load");
        assert_eq!(view.read(cx).open_tabs.as_slice(), ["thread-a"]);
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_none(), "a lone tab hides the strip");

        // The last tab closes through its action (its button left with the
        // strip); the session stays selected.
        window.dispatch_action(Box::new(super::super::tabs::CloseThreadTab { session_id: "thread-a".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).open_tabs.is_empty());
        assert!(window.try_find("thread-tabs").is_none(), "empty tabs hide the strip");
        assert_eq!(view.read(cx).selected_session_id.as_deref(), Some("thread-b"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn refused_load_keeps_the_selected_tab(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_threaded_desktop(cx);
    view.update(cx, |view, _| {
        view.session_request = Some(vtcode_desktop_protocol::method::SESSION_LOAD);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-close-thread-b", cx);
        assert!(requests.try_recv().is_err(), "a busy session refuses the neighbor load");
        assert_eq!(
            view.read(cx).open_tabs.as_slice(),
            ["thread-a", "thread-b", "thread-c"],
            "the selected tab stays reachable when its load is refused"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tab_context_menu_offers_close_and_copy(cx: &mut TestAppContext) {
    use super::super::tabs::{CloseOtherThreadTabs, CloseThreadTab, CopyThreadTitle};

    let (window, view, _) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.right_click("thread-tab-thread-b", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("popup-menu").role(), Some(gpui_kit::Role::Menu), "right-click opens the tab menu");
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();

    // Menu items dispatch tab-scoped payload actions.
    view.update(cx, |view, cx| {
        view.close_other_tabs("thread-b", cx);
        assert_eq!(view.open_tabs.as_slice(), ["thread-b"]);
        assert_eq!(view.selected_session_id.as_deref(), Some("thread-b"));
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(CopyThreadTitle { session_id: "thread-b".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Second thread"));

    view.update(cx, |view, _| {
        view.open_tabs = vec!["thread-a".into(), "thread-b".into()];
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(CloseThreadTab { session_id: "thread-a".into() }), cx);
        window.dispatch_action(Box::new(CloseOtherThreadTabs { session_id: "ghost".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert_eq!(view.open_tabs.as_slice(), ["thread-b"]);
    });
}

#[gpui_kit::test]
fn session_row_menu_offers_copy_close_and_confirm_gated_delete(cx: &mut TestAppContext) {
    use super::super::tabs::{CopyThreadTitle, DeleteThread};

    let (window, view, mut requests) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.right_click("session-row-thread-a", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("popup-menu").role(), Some(gpui_kit::Role::Menu), "right-click opens the session menu");
        window.press("escape", cx);
        window.render_frame(cx);
        window.dispatch_action(Box::new(CopyThreadTitle { session_id: "thread-a".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("First thread"));

    // Delete stays confirm-gated: dispatching alone sends nothing.
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(DeleteThread { session_id: "thread-a".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "delete waits for the warning prompt answer");
    cx.update(|cx| {
        assert_eq!(view.read(cx).sessions.len(), 3);
    });
}

#[gpui_kit::test]
fn single_tab_hides_the_strip_and_keeps_a_compact_header(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_threaded_desktop(cx);
    view.update(cx, |view, _| {
        view.open_tabs = vec!["thread-b".into()];
        view.transcript.push(TranscriptItem::new(
            "header-message".into(),
            "assistant".into(),
            "VT Code".into(),
            "Conversation remains visible.".into(),
        ));
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_none(), "a lone tab hides the strip");
        assert!(window.try_find("thread-tab-new").is_none(), "the strip takes its new-thread button with it");
        assert!(window.try_find("thread-header").is_some(), "the compact header identifies the thread");
        assert!(window.try_find("thread-title").is_some());
        assert!(window.try_find("thread-header-new").is_some(), "the header keeps a new-thread action");
        window.right_click("session-row-thread-b", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("popup-menu").role(),
            Some(gpui_kit::Role::Menu),
            "lone-tab sidebar menu stays reachable"
        );
        window.press("escape", cx);
        window.render_frame(cx);

        window.click("thread-header-new", cx);
        let BackendCommand::Request { method, params } =
            requests.try_recv().expect("header new-thread starts a thread")
        else {
            panic!("expected a session request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
        assert_eq!(params["sessionId"].as_str(), Some("thread-b"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn single_tab_empty_thread_shows_header_for_its_selection(cx: &mut TestAppContext) {
    let (window, view, _) = open_threaded_desktop(cx);
    view.update(cx, |view, _| {
        view.open_tabs = vec!["thread-b".into()];
        assert!(view.transcript.is_empty(), "no transcript pushed: the empty case");
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_none(), "a lone tab hides the strip");
        assert!(window.try_find("thread-header").is_some(), "the selection keeps its header");
        assert!(window.try_find("thread-title").is_some());
        assert!(window.try_find("thread-header-new").is_some(), "empty threads keep a new-thread action");
    })
    .unwrap();
}

#[gpui_kit::test]
fn fresh_launch_without_selection_keeps_no_identity_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.zen_mode = false;
        assert!(view.selected_session_id.is_none(), "no selection yet");
        assert!(view.transcript.is_empty(), "no content yet");
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_none());
        assert!(window.try_find("thread-header").is_none(), "welcome owns the empty state");
    })
    .unwrap();
}

#[gpui_kit::test]
fn multi_tab_strip_replaces_the_header_and_zero_tabs_restore_it(cx: &mut TestAppContext) {
    let (window, view, _) = open_threaded_desktop(cx);
    view.update(cx, |view, _| {
        view.transcript.push(TranscriptItem::new(
            "header-message".into(),
            "assistant".into(),
            "VT Code".into(),
            "Conversation remains visible.".into(),
        ));
    });
    cx.update_window(window, |_, window, cx| {
        // Three tabs: the strip owns thread identity, so the header stays out.
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_some());
        assert!(window.try_find("thread-header").is_none(), "the strip replaces the header");
        assert!(window.try_find("thread-header-new").is_none());

        // Zero tabs: the header returns for the still-selected thread.
        view.update(cx, |view, _| view.open_tabs.clear());
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_none());
        assert!(window.try_find("thread-header").is_some(), "zero tabs restore the header");
        assert!(window.try_find("thread-header-new").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn browser_row_menu_matches_sidebar_session_menu(cx: &mut TestAppContext) {
    let (window, _view, _requests) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());
        window.right_click("browser-session-row-thread-a", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("popup-menu").role(),
            Some(gpui_kit::Role::Menu),
            "right-click opens the browser-row menu"
        );
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("popup-menu").is_none(), "escape dismisses the browser-row menu");
        assert!(window.try_find("thread-browser").is_some(), "browser stays open under the dismissed menu");
    })
    .unwrap();
}
