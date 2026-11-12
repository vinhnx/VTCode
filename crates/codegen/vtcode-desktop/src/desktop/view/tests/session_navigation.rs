use super::*;

fn session_fixture(id: &str) -> vtcode_desktop_protocol::SessionSummary {
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

fn seed_threads(view: &Entity<DesktopView>, cx: &mut gpui_kit::App, ids: &[&str]) {
    view.update(cx, |view, _| {
        view.sessions = ids.iter().map(|id| session_fixture(id)).collect();
    });
}

#[gpui_kit::test]
fn session_arrows_navigate_threads_paint_rail_clamp_and_heal_stale_ids(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_threads(&view, cx, &["thread-first", "thread-second", "thread-third"]);
        window.render_frame(cx);
        focus_element_by_tab(window, "session-list", cx);
        assert!(view.read(cx).session_focus.is_focused(window));
        let row = window.find("session-row-thread-second").bounds();
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-first"));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-second"));
        window.render_frame(cx);
        assert!(window.try_find("session-focus-rail-thread-second").is_some(), "highlight paints a rail");
        assert_eq!(window.find("session-row-thread-second").bounds(), row, "highlight must not reflow the row");
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-third"));
        window.press("down", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("thread-third"),
            "navigation clamps at the last thread"
        );
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-second"));
        window.render_frame(cx);
        assert!(window.try_find("session-focus-rail-thread-third").is_none(), "rail follows the highlight");
        window.press("up", cx);
        window.press("up", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-first"));
        // A stale id (thread list refreshed underneath) restarts from the edge.
        view.update(cx, |view, _| view.keyboard_focused_session = Some("thread-gone".into()));
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("thread-first"));
        view.update(cx, |view, _| view.keyboard_focused_session = None);
        window.press("up", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("thread-third"),
            "up from nothing starts at the last thread"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_list_is_one_tab_stop_rows_are_skipped(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_threads(&view, cx, &["skip-first", "skip-second"]);
        window.render_frame(cx);
        focus_element_by_tab(window, "session-list", cx);
        for _ in 0..6 {
            window.press("tab", cx);
            window.render_frame(cx);
            for id in ["session-skip-first", "session-skip-second"] {
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
fn session_rail_aligns_to_highlighted_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_threads(&view, cx, &["geo-first", "geo-second", "geo-third"]);
        window.render_frame(cx);
        focus_element_by_tab(window, "session-list", cx);
        window.press("down", cx);
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("geo-second"));
        window.render_frame(cx);
        let row = window.find("session-row-geo-second").bounds();
        let rail = window.find("session-focus-rail-geo-second").bounds();
        assert_eq!(rail.origin, row.origin, "rail starts at the row's top-left");
        assert_eq!(rail.size.height, row.size.height, "rail spans exactly the row");
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_home_end_jump_to_edges_and_empty_list_is_inert(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_threads(&view, cx, &["edge-first", "edge-mid", "edge-last"]);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.session_focus.focus(window, cx));
        window.press("end", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("edge-last"));
        window.press("home", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("edge-first"));
        // Empty list: every key is a no-op, never a panic.
        view.update(cx, |view, _| {
            view.sessions.clear();
            view.keyboard_focused_session = None;
        });
        window.render_frame(cx);
        for key in ["down", "up", "home", "end", "enter"] {
            window.press(key, cx);
            assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), None, "{key} on an empty list");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_enter_loads_highlighted_thread_exactly_once(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(std::env::temp_dir());
        });
        seed_threads(&view, cx, &["load-first", "load-second"]);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.session_focus.focus(window, cx));
        window.press("down", cx);
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("load-second"));
        window.press("enter", cx);
        assert_eq!(view.read(cx).session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
    })
    .unwrap();
    let first = requests.try_recv().expect("enter sends one session/load");
    match first {
        BackendCommand::Request { method, params } => {
            assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
            assert_eq!(params.get("sessionId").and_then(|id| id.as_str()), Some("load-second"));
        }
        BackendCommand::Stop => panic!("enter must send a load request, not stop"),
    }
    assert!(requests.try_recv().is_err(), "enter must not double-dispatch the load");
}

#[gpui_kit::test]
fn session_click_anchors_highlight_as_position_memory(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(std::env::temp_dir());
        });
        seed_threads(&view, cx, &["click-first", "click-second"]);
        window.render_frame(cx);
        window.click("session-click-second", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("click-second"));
        assert!(window.try_find("session-focus-rail-click-second").is_some(), "click paints the rail");
        assert!(window.try_find("session-focus-rail-click-first").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn vim_keys_are_inert_when_the_toggle_is_off(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        assert!(!view.read(cx).preferences.vim_navigation, "vim ships off by default");
        seed_threads(&view, cx, &["vim-off-first", "vim-off-second"]);
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "vim-off-message".into(),
                "assistant".into(),
                "VT Code".into(),
                "Body".into(),
            ));
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.session_focus.focus(window, cx));
        for key in ["j", "k", "g", "shift-g"] {
            window.press(key, cx);
            assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), None, "{key} must not move threads");
        }
        view.update(cx, |view, cx| view.transcript_focus.focus(window, cx));
        for key in ["j", "k", "g", "shift-g"] {
            window.press(key, cx);
            assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), None, "{key} must not move transcript");
        }
        assert!(!view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.press("i", cx);
        assert!(
            !view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "i must not focus the composer"
        );
        window.press("/", cx);
        assert!(view.read(cx).palette.is_none(), "/ must not open search");
    })
    .unwrap();
}

#[gpui_kit::test]
fn vim_motions_move_threads_and_transcript(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
        seed_threads(&view, cx, &["vim-first", "vim-mid", "vim-last"]);
        view.update(cx, |view, _| {
            for id in ["vim-message-first", "vim-message-mid", "vim-message-last"] {
                view.transcript.push(TranscriptItem::new(
                    id.into(),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Body {id}"),
                ));
            }
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.session_focus.focus(window, cx));
        window.press("j", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("vim-first"));
        window.press("j", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("vim-mid"));
        window.press("k", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("vim-first"));
        window.press("shift-g", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("vim-last"));
        window.press("g", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("vim-first"));
        window.render_frame(cx);
        assert!(window.try_find("session-focus-rail-vim-first").is_some(), "vim highlight paints the rail");

        view.update(cx, |view, cx| view.transcript_focus.focus(window, cx));
        window.press("j", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("vim-message-first"));
        window.press("shift-g", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("vim-message-last"));
        window.press("g", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("vim-message-first"));
        window.press("k", cx);
        assert_eq!(
            view.read(cx).keyboard_focused_message.as_deref(),
            Some("vim-message-first"),
            "vim clamps like arrows"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn vim_hl_cycle_inspector_and_settings_tabs(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
        window.render_frame(cx);
        assert!(window.try_find("inspector-panel").is_some(), "inspector starts open");
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        let first_tab = view.read(cx).inspector_tab;
        window.press("l", cx);
        let moved = view.read(cx).inspector_tab;
        assert_ne!(moved, first_tab, "l advances the inspector tab");
        window.press("h", cx);
        assert_eq!(view.read(cx).inspector_tab, first_tab, "h returns to the starting tab");

        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        let first_page = view.read(cx).settings_page_index();
        window.press("l", cx);
        let moved_page = view.read(cx).settings_page_index();
        assert_ne!(moved_page, first_page, "l advances the settings tab");
        window.press("h", cx);
        assert_eq!(view.read(cx).settings_page_index(), first_page, "h returns to the starting tab");

        // Off state: the same keys leave both rails alone.
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
        window.press("l", cx);
        assert_eq!(view.read(cx).settings_page_index(), first_page);
    })
    .unwrap();
}

#[gpui_kit::test]
fn vim_i_focuses_composer_and_slash_opens_search(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.toggle_vim_navigation(cx);
            view.selected_session_id = Some("vim-search-thread".into());
            view.transcript.push(TranscriptItem::new(
                "searchable".into(),
                "user".into(),
                "You".into(),
                "searchable words".into(),
            ));
        });
        seed_threads(&view, cx, &["vim-search-thread"]);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.session_focus.focus(window, cx));
        assert!(view.read(cx).session_focus.is_focused(window));
        window.press("i", cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window), "i focuses the composer");
        view.update(cx, |view, cx| view.session_focus.focus(window, cx));
        window.press("/", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "/ opens conversation search");
    })
    .unwrap();
}

#[gpui_kit::test]
fn vim_keys_typed_in_composer_never_drive_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
        seed_threads(&view, cx, &["composer-first", "composer-second"]);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_composer(window, cx));
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        for key in ["j", "k", "g", "shift-g", "h", "l", "i", "/"] {
            window.press(key, cx);
        }
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), None, "threads must not move");
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), None, "transcript must not move");
        assert!(view.read(cx).palette.is_none(), "search must not open");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window), "focus stays in the composer");
        // Typing integrity with vim on: vim letters land as text, nothing hijacks them.
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("", window, cx));
        });
        window.input("vim jkl gh i/", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "vim jkl gh i/");
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), None);
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_escape_returns_to_composer_and_keeps_highlight(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        seed_threads(&view, cx, &["escape-first", "escape-second"]);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.session_focus.focus(window, cx));
        window.press("down", cx);
        window.press("down", cx);
        assert_eq!(view.read(cx).keyboard_focused_session.as_deref(), Some("escape-second"));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "escape returns to the composer"
        );
        assert_eq!(
            view.read(cx).keyboard_focused_session.as_deref(),
            Some("escape-second"),
            "escape keeps the highlight as position memory"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn vim_enabled_in_saved_preferences_binds_at_startup(cx: &mut TestAppContext) {
    use super::super::SessionNext;
    use crate::desktop::strings::key_context;

    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    std::fs::write(&prefs_path, r#"{"vimNavigation": true}"#).unwrap();
    let (_window, view) = open_desktop_with_preferences_path(cx, prefs_path);
    cx.update(|cx| {
        assert!(view.read(cx).preferences.vim_navigation, "startup loads the saved toggle");
        assert_chord_resolves_to(cx, "j", key_context::SESSIONS, &SessionNext);
    });
}

#[gpui_kit::test]
fn transcript_home_end_jump_to_first_and_last_visible_message(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for id in ["edge-first", "edge-mid", "edge-last"] {
                view.transcript.push(TranscriptItem::new(
                    id.into(),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("Body {id}"),
                ));
            }
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.transcript_focus.focus(window, cx));
        window.press("end", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("edge-last"));
        window.press("home", cx);
        assert_eq!(view.read(cx).keyboard_focused_message.as_deref(), Some("edge-first"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn vim_toggle_binds_and_retires_every_chord(cx: &mut TestAppContext) {
    use super::super::{
        ActivityFirst, ActivityLast, ActivityNext, ActivityPrevious, FocusComposer, NextInspectorTab, NextSettingsTab,
        NextThreadTab, OpenConversationSearch, PreviousInspectorTab, PreviousSettingsTab, PreviousThreadTab,
        SessionFirst, SessionLast, SessionNext, SessionPrevious, TranscriptFirst, TranscriptLast, TranscriptNext,
        TranscriptPrevious,
    };
    use crate::desktop::strings::key_context;

    // Independent oracle: every vim row the toggle must bind live and retire.
    let expected: Vec<(&str, &str, Box<dyn gpui_kit::Action>)> = vec![
        ("j", key_context::SESSIONS, Box::new(SessionNext)),
        ("k", key_context::SESSIONS, Box::new(SessionPrevious)),
        ("g", key_context::SESSIONS, Box::new(SessionFirst)),
        ("G", key_context::SESSIONS, Box::new(SessionLast)),
        ("i", key_context::SESSIONS, Box::new(FocusComposer)),
        ("/", key_context::SESSIONS, Box::new(OpenConversationSearch)),
        ("j", key_context::TRANSCRIPT, Box::new(TranscriptNext)),
        ("k", key_context::TRANSCRIPT, Box::new(TranscriptPrevious)),
        ("g", key_context::TRANSCRIPT, Box::new(TranscriptFirst)),
        ("G", key_context::TRANSCRIPT, Box::new(TranscriptLast)),
        ("i", key_context::TRANSCRIPT, Box::new(FocusComposer)),
        ("/", key_context::TRANSCRIPT, Box::new(OpenConversationSearch)),
        ("h", key_context::INSPECTOR, Box::new(PreviousInspectorTab)),
        ("l", key_context::INSPECTOR, Box::new(NextInspectorTab)),
        ("i", key_context::INSPECTOR, Box::new(FocusComposer)),
        ("/", key_context::INSPECTOR, Box::new(OpenConversationSearch)),
        ("h", key_context::SETTINGS, Box::new(PreviousSettingsTab)),
        ("l", key_context::SETTINGS, Box::new(NextSettingsTab)),
        ("i", key_context::SETTINGS, Box::new(FocusComposer)),
        ("/", key_context::SETTINGS, Box::new(OpenConversationSearch)),
        ("h", key_context::THREAD_TABS, Box::new(PreviousThreadTab)),
        ("l", key_context::THREAD_TABS, Box::new(NextThreadTab)),
        ("i", key_context::THREAD_TABS, Box::new(FocusComposer)),
        ("/", key_context::THREAD_TABS, Box::new(OpenConversationSearch)),
        ("j", key_context::ACTIVITY, Box::new(ActivityNext)),
        ("k", key_context::ACTIVITY, Box::new(ActivityPrevious)),
        ("g", key_context::ACTIVITY, Box::new(ActivityFirst)),
        ("shift-g", key_context::ACTIVITY, Box::new(ActivityLast)),
        ("i", key_context::ACTIVITY, Box::new(FocusComposer)),
        ("/", key_context::ACTIVITY, Box::new(OpenConversationSearch)),
    ];
    let (window, view) = open_desktop(cx);
    cx.update(|cx| {
        for (chord, context, _) in &expected {
            assert_chord_dead(cx, chord, context);
        }
    });
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
    })
    .unwrap();
    cx.update(|cx| {
        for (chord, context, action) in &expected {
            assert_chord_resolves_to(cx, chord, context, action.as_ref());
        }
    });
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
    })
    .unwrap();
    cx.update(|cx| {
        for (chord, context, _) in &expected {
            assert_chord_dead(cx, chord, context);
        }
    });
    // Re-enabling binds over the shadows: latest binding wins.
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.toggle_vim_navigation(cx));
    })
    .unwrap();
    cx.update(|cx| {
        for (chord, context, action) in &expected {
            assert_chord_resolves_to(cx, chord, context, action.as_ref());
        }
    });
}

#[gpui_kit::test]
fn vim_toggle_persists_and_reset_restores_keyboard_defaults(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        select_settings_category(window, 3, cx);
        window.render_frame(cx);
        assert!(window.try_find("vim-navigation").is_some(), "switch renders in the shortcuts tab");
        scroll_settings_by(window, px(-6000.), cx);
        window.render_frame(cx);
        window.click("vim-navigation", cx);
        assert!(view.read(cx).preferences.vim_navigation, "switch turns vim on");
        window.click("vim-navigation", cx);
        assert!(!view.read(cx).preferences.vim_navigation, "switch turns vim off");
        window.click("vim-navigation", cx);
        window.render_frame(cx);
        // Reset restores every keyboard default at once: staged shortcuts plus
        // the immediately-applied vim toggle.
        scroll_settings_by(window, px(6000.), cx);
        window.render_frame(cx);
        window.click("reset-shortcuts", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.vim_navigation, "reset turns vim off");
        assert!(!view.read(cx).shortcuts_editing, "a shortcuts-clean table stages nothing");
    })
    .unwrap();
    assert!(
        !crate::desktop::preferences::Preferences::load().vim_navigation,
        "the persisted toggle survives reload"
    );
}

fn session_list_response(rows: usize) -> BackendEvent {
    let sessions: Vec<_> = (0..rows).map(|n| session_fixture(&format!("thread-{n}"))).collect();
    BackendEvent::Response {
        method: vtcode_desktop_protocol::method::SESSION_LIST.into(),
        result: Some(serde_json::to_value(sessions).unwrap()),
        error: None,
    }
}

#[gpui_kit::test]
fn session_list_paging_grows_by_page_and_caps_at_max(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.session_list_limit, SESSION_LIST_PAGE_SIZE, "fresh views start paged");
            assert!(!view.session_list_has_older);
            view.show_more_sessions(cx);
            assert_eq!(view.session_list_limit, SESSION_LIST_PAGE_SIZE, "a failed send keeps the requested limit");

            view.backend = Some(BackendHandle { commands });
            // Below the page: nothing more to load.
            view.handle_backend_event(session_list_response(7), cx);
            assert!(!view.session_list_has_older);

            // An exact-limit hit below the maximum offers the next page.
            view.handle_backend_event(session_list_response(SESSION_LIST_PAGE_SIZE), cx);
            assert!(view.session_list_has_older);
            view.show_more_sessions(cx);
            assert_eq!(view.session_list_limit, 2 * SESSION_LIST_PAGE_SIZE);

            // A partial page closes the affordance again.
            view.handle_backend_event(session_list_response(60), cx);
            assert!(!view.session_list_has_older, "a short page reads as complete");

            // At the maximum the affordance stays hidden even on a full page.
            view.session_list_limit = vtcode_desktop_protocol::MAX_SESSION_LIST_LIMIT;
            view.handle_backend_event(session_list_response(vtcode_desktop_protocol::MAX_SESSION_LIST_LIMIT), cx);
            assert!(!view.session_list_has_older, "capped at the maximum");

            // Over-limit answers from older servers read as complete.
            view.session_list_limit = SESSION_LIST_PAGE_SIZE;
            view.handle_backend_event(session_list_response(SESSION_LIST_PAGE_SIZE + 10), cx);
            assert!(!view.session_list_has_older, "over-limit answers read as complete");
        });
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("show more requests the next page")
    else {
        panic!("show more must send session/list, not stop");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LIST);
    assert_eq!(params, json!({ "limit": 2 * SESSION_LIST_PAGE_SIZE }));
    assert!(requests.try_recv().is_err(), "one page request queues one command");
}

#[gpui_kit::test]
fn session_list_footer_survives_local_create_and_delete(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.session_list_limit = SESSION_LIST_PAGE_SIZE;
            view.handle_backend_event(session_list_response(SESSION_LIST_PAGE_SIZE), cx);
            assert!(view.session_list_has_older);

            // A local create grows loaded and total together: older pages stay.
            let created = serde_json::to_value(session_fixture("thread-new")).unwrap();
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
                    result: Some(created),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.sessions.len(), SESSION_LIST_PAGE_SIZE + 1);
            assert!(view.session_list_has_older, "a create preserves the older pages");

            // A local delete shrinks both together: still more below.
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_DELETE.into(),
                    result: Some(json!({ "sessionId": "thread-new", "deleted": true })),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.sessions.len(), SESSION_LIST_PAGE_SIZE);
            assert!(view.session_list_has_older, "a delete preserves the older pages");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_list_more_footer_renders_on_both_surfaces_and_requests_next_page(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        let ids: Vec<String> = (0..SESSION_LIST_PAGE_SIZE).map(|n| format!("thread-{n}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        seed_threads(&view, cx, &refs);
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.session_list_limit = SESSION_LIST_PAGE_SIZE;
            view.session_list_has_older = true;
        });
        window.render_frame(cx);
        assert!(window.try_find("session-list-more").is_some(), "a full page offers more in the sidebar");

        // The browser shares the affordance while the page is exactly full;
        // clicking grows the limit, which hides both footers until the next
        // page lands.
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("browser-list-more").is_some(), "the browser shares the affordance");
        window.click("browser-list-more", cx);
        assert_eq!(view.read(cx).session_list_limit, 2 * SESSION_LIST_PAGE_SIZE);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("footer click requests the next page")
    else {
        panic!("footer click must send session/list, not stop");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LIST);
    assert_eq!(params, json!({ "limit": 2 * SESSION_LIST_PAGE_SIZE }));
}

#[gpui_kit::test]
fn workspace_change_resets_thread_paging_while_restart_keeps_depth(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let terminal_dir = tempfile::tempdir().unwrap();
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            let alpha = PathBuf::from("/work/alpha");
            view.prepare_workspace(&alpha, cx);
            let size = portable_pty::PtySize { rows: 4, cols: 40, pixel_width: 0, pixel_height: 0 };
            view.terminal = Some(
                super::super::super::terminal_tab::TerminalTab::spawn(
                    terminal_dir.path(),
                    Path::new("/bin/sh"),
                    size,
                    super::super::super::terminal_tab::palette_for_theme(&cx.theme().colors),
                )
                .unwrap(),
            );
            assert_eq!(view.session_list_limit, SESSION_LIST_PAGE_SIZE, "a fresh workspace starts paged");
            view.session_list_limit = 2 * SESSION_LIST_PAGE_SIZE;
            view.prepare_workspace(&alpha, cx);
            assert!(view.terminal.is_some(), "a same-workspace restart retains the terminal");
            assert_eq!(
                view.session_list_limit,
                2 * SESSION_LIST_PAGE_SIZE,
                "a same-workspace restart keeps the expanded depth"
            );
            view.prepare_workspace(&PathBuf::from("/work/beta"), cx);
            assert!(view.terminal.is_none(), "a different workspace closes the terminal");
            assert_eq!(view.session_list_limit, SESSION_LIST_PAGE_SIZE, "a new workspace starts paged");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn discovery_rerequests_the_current_thread_depth(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.session_list_limit = 2 * SESSION_LIST_PAGE_SIZE;
            view.initialize_backend(BackendHandle { commands });
            let _initialize = requests.try_recv().unwrap();
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::INITIALIZE.into(),
                    result: Some(json!({
                        "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION,
                        "workspace": "/workspace",
                        "serverVersion": "test",
                    })),
                    error: None,
                },
                cx,
            );
        });
    })
    .unwrap();
    for expected in [
        vtcode_desktop_protocol::method::CONFIG_DISCOVER,
        vtcode_desktop_protocol::method::CONFIG_SETTINGS_READ,
        vtcode_desktop_protocol::method::CONFIG_MODELS,
    ] {
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("discovery")
        };
        assert_eq!(method, expected);
    }
    let BackendCommand::Request { method, params } = requests.try_recv().expect("discovery lists threads") else {
        panic!("discovery must send session/list, not stop");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LIST);
    assert_eq!(params, json!({ "limit": 2 * SESSION_LIST_PAGE_SIZE }));
}
