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
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.close_other_tabs("thread-b", window, cx);
            assert_eq!(view.open_tabs.as_slice(), ["thread-b"]);
            assert_eq!(view.selected_session_id.as_deref(), Some("thread-b"));
        });
    })
    .unwrap();
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

#[gpui_kit::test]
fn tabs_left_right_rove_paint_rail_clamp_and_heal(cx: &mut TestAppContext) {
    let (window, view, _) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.tab_strip_focus.focus(window, cx));
        assert!(view.read(cx).tab_strip_focus.is_focused(window));
        // First-open order, independent of the selected tab (thread-b).
        let tab = window.find("thread-tab-row-thread-b").bounds();
        window.press("right", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-a"));
        window.press("right", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-b"));
        window.render_frame(cx);
        assert!(window.try_find("thread-tab-focus-rail-thread-b").is_some(), "highlight paints a rail");
        assert_eq!(window.find("thread-tab-row-thread-b").bounds(), tab, "highlight must not reflow the tab");
        window.press("right", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-c"));
        window.press("right", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("thread-c"),
            "navigation clamps at the last tab"
        );
        window.press("left", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-b"));
        window.render_frame(cx);
        assert!(window.try_find("thread-tab-focus-rail-thread-c").is_none(), "rail follows the highlight");
        // A stale id (tab closed underneath) restarts from the edge.
        view.update(cx, |view, _| view.keyboard_focused_session = Some("thread-gone".into()));
        window.press("right", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-a"));
        view.update(cx, |view, _| view.keyboard_focused_session = None);
        window.press("left", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("thread-c"),
            "left from nothing starts at the last tab"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tabs_home_end_jump_and_enter_switches_exactly_once(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.tab_strip_focus.focus(window, cx));
        window.press("end", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-c"));
        window.press("home", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-a"));
        window.press("end", cx);
        window.press("enter", cx);
        assert_eq!(view.read(cx).session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
        assert_eq!(load_request_for(&mut requests), "thread-c");
        assert!(requests.try_recv().is_err(), "enter must not double-dispatch the load");
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("thread-c"),
            "the highlight survives as position memory"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tabs_click_anchors_highlight_as_position_memory(cx: &mut TestAppContext) {
    let (window, view, _) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-thread-a", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-a"));
        assert!(window.try_find("thread-tab-focus-rail-thread-a").is_some(), "click paints the rail");
        assert!(window.try_find("thread-tab-focus-rail-thread-b").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn tabs_enter_with_untabbed_highlight_is_inert(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sessions.push(summary("thread-d", "Fourth thread"));
            view.keyboard_focused_session = Some("thread-d".into());
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.tab_strip_focus.focus(window, cx));
        window.press("enter", cx);
        assert!(requests.try_recv().is_err(), "an untabbed highlight loads nothing");
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("thread-d"),
            "the position memory survives for the thread lists"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn tabs_strip_is_one_tab_stop_rows_are_skipped(cx: &mut TestAppContext) {
    let (window, view, _) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        focus_element_by_tab(window, "thread-tabs", cx);
        assert!(view.read(cx).tab_strip_focus.is_focused(window));
        // The select buttons sit between the strip and the close buttons in
        // Tab order: if any were a stop, the next tabs would land on one.
        for _ in 0..6 {
            window.press("tab", cx);
            window.render_frame(cx);
            for id in ["thread-tab-thread-a", "thread-tab-thread-b", "thread-tab-thread-c"] {
                assert!(
                    !window.try_find(id).is_some_and(|element| element.focused() == Some(true)),
                    "{id} stays out of Tab order"
                );
            }
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn palette_copy_thread_title_copies_selected_title_and_gates_on_selection(cx: &mut TestAppContext) {
    use super::super::PaletteAction;
    let (window, view, _) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find(palette_action_id(&PaletteAction::CopyThreadTitle)).label(), Some("Copy thread title"));
        view.update(cx, |view, cx| view.run_palette_action(PaletteAction::CopyThreadTitle, window, cx));
    })
    .unwrap();
    assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Second thread"));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.selected_session_id = None);
        assert!(!view.read(cx).palette_action_available(&PaletteAction::CopyThreadTitle));
        view.update(cx, |view, cx| view.run_palette_action(PaletteAction::CopyThreadTitle, window, cx));
    })
    .unwrap();
    assert_eq!(
        cx.read_from_clipboard().unwrap().text().as_deref(),
        Some("Second thread"),
        "gated runs leave the clipboard alone"
    );
}

#[gpui_kit::test]
fn palette_close_other_tabs_keeps_selected_and_gates_on_tabs(cx: &mut TestAppContext) {
    use super::super::PaletteAction;
    let (window, view, _) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find(palette_action_id(&PaletteAction::CloseOtherTabs)).label(), Some("Close other tabs"));
        assert!(view.read(cx).palette_action_available(&PaletteAction::CloseOtherTabs));
        view.update(cx, |view, cx| view.run_palette_action(PaletteAction::CloseOtherTabs, window, cx));
        assert_eq!(view.read(cx).open_tabs, vec!["thread-b".to_string()]);
        assert!(!view.read(cx).palette_action_available(&PaletteAction::CloseOtherTabs));
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_backspace_routes_highlight_to_confirm_gated_delete(cx: &mut TestAppContext) {
    use super::super::DeleteHighlightedSession;
    use crate::desktop::strings::key_context;
    let (window, view, mut requests) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        focus_element_by_tab(window, "session-list", cx);
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-a"));
        window.press("backspace", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "delete waits for the warning prompt answer");
    cx.update(|cx| {
        assert_eq!(view.read(cx).sessions.len(), 3);
        // Both physical delete keys resolve to the highlight action.
        assert_chord_resolves_to(cx, "backspace", key_context::SESSIONS, &DeleteHighlightedSession);
        assert_chord_resolves_to(cx, "delete", key_context::SESSIONS, &DeleteHighlightedSession);
    });
    // Deleting the selected thread is refused outright, like the menu flow.
    cx.update_window(window, |_, window, cx| {
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-b"));
        window.press("backspace", cx);
        window.render_frame(cx);
        assert!(view.read(cx).error.is_some(), "selected-thread delete is refused");
        assert_eq!(view.read(cx).sessions.len(), 3);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "refused deletes send nothing");
    // No highlight, no action.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.error = None;
            view.keyboard_focused_session = None;
        });
        window.press("backspace", cx);
        window.render_frame(cx);
        assert!(view.read(cx).error.is_none());
        assert_eq!(view.read(cx).sessions.len(), 3);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "highlight-free deletes send nothing");
    // A stale highlight (thread left the list) never reaches the confirm
    // prompt: no error, no request. Prompt suppression itself is only
    // observable in a native run.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.keyboard_focused_session = Some("thread-gone".into());
        });
        window.press("backspace", cx);
        window.render_frame(cx);
        assert!(view.read(cx).error.is_none());
        assert_eq!(view.read(cx).sessions.len(), 3);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "stale deletes send nothing");
}

#[gpui_kit::test]
fn tabs_vim_hl_move_and_i_focuses_composer(cx: &mut TestAppContext) {
    let (window, view, _) = open_threaded_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.tab_strip_focus.focus(window, cx));
        window.press("l", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-a"));
        window.press("l", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-b"));
        window.press("h", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-a"));
        window.press("i", cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window), "i focuses the composer");
    })
    .unwrap();
}
