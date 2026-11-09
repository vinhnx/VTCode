use super::super::DesktopView;
use super::super::PaletteKind;
use super::*;
use gpui_kit::Entity;

fn focus_empty_composer(window: &AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update_window(*window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_w_deletes_word_backward_and_undo_restores(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    focus_empty_composer(&window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.input("hello world", cx);
        window.press("ctrl-w", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "hello ");
        // Headless `press` does not deliver the ctrl-z chord; cmd-z drives the same Undo action.
        window.press("cmd-z", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "hello world");
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_u_and_ctrl_k_delete_to_line_edges(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    focus_empty_composer(&window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.input("hello world", cx);
        window.press("ctrl-u", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        window.input("hello world", cx);
        window.press("cmd-left", cx);
        window.press("ctrl-k", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
    })
    .unwrap();
}

#[gpui_kit::test]
fn alt_d_deletes_word_forward(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    focus_empty_composer(&window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.input("hello world", cx);
        window.press("cmd-left", cx);
        window.press("alt-d", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), " world");
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_f_b_move_by_char_and_ctrl_j_alt_enter_insert_newlines(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    focus_empty_composer(&window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.input("ab", cx);
        window.press("ctrl-b", cx);
        window.input("X", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "aXb");
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("cd", window, cx));
        });
        // Programmatic writes leave the caret at the start; ctrl-f steps over 'c'.
        window.press("ctrl-f", cx);
        window.input("Y", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "cYd");
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("", window, cx));
        });
        window.input("a", cx);
        window.press("ctrl-j", cx);
        window.input("b", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "a\nb");
        window.press("alt-enter", cx);
        window.input("d", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "a\nb\nd");
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_m_opens_and_toggles_model_options(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    focus_empty_composer(&window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let (commands, _) = tokio::sync::mpsc::channel(2);
            view.backend = Some(BackendHandle { commands });
        });
        window.press("ctrl-m", cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, PaletteKind::ModelOptions));
        window.press("ctrl-m", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn history_recall_traverses_newest_first_restores_stash_and_resets_on_submit(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("history-session".into());
            for (id, text) in [("history-user-1", "first prompt"), ("history-user-2", "second prompt")] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "user".into(), "You".into(), text.into()));
            }
            view.composer.update(cx, |state, cx| {
                state.set_value("wip", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
    })
    .unwrap();
    // One press per update: `Change` effects flush at update boundaries in
    // production (each keypress is its own platform event), so batching
    // presses would hide a self-resetting traversal.
    for (key, expected, message) in [
        ("up", "second prompt", ""),
        ("up", "first prompt", ""),
        ("up", "first prompt", "recall clamps at the oldest"),
        ("down", "second prompt", ""),
        ("down", "wip", "down past newest restores the stash"),
        ("down", "wip", "live draft has nothing newer"),
        ("up", "second prompt", ""),
    ] {
        cx.update_window(window, |_, window, cx| {
            window.press(key, cx);
            assert_eq!(view.read(cx).composer.read(cx).value(), expected, "{message}");
        })
        .unwrap();
    }
    cx.update_window(window, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected recalled submit");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["prompt"], "second prompt");
    // Submit reset the cursor: recall starts from the newest again, not one past it.
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        window.press("up", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "second prompt");
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "recall never submits");
}

#[gpui_kit::test]
fn up_past_oldest_pushes_no_phantom_undo(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            for (id, text) in [("history-user-1", "first prompt"), ("history-user-2", "second prompt")] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "user".into(), "You".into(), text.into()));
            }
            view.composer.update(cx, |state, cx| {
                state.set_value("wip", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
    })
    .unwrap();
    for (key, expected) in [("up", "second prompt"), ("up", "first prompt"), ("up", "first prompt")] {
        cx.update_window(window, |_, window, cx| {
            window.press(key, cx);
            assert_eq!(view.read(cx).composer.read(cx).value(), expected);
        })
        .unwrap();
    }
    // The clamped press wrote nothing, so undo reaches the real prior step.
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-z", cx);
    })
    .unwrap();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "second prompt");
    })
    .unwrap();
}

#[gpui_kit::test]
fn enter_with_empty_history_picker_submits_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("fresh-session".into());
            view.composer.update(cx, |state, cx| {
                state.set_value("hello", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "empty picker still opens");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    expect_recent_prompts_request(&mut requests, super::super::HistoryScope::Everywhere);
    let BackendCommand::Request { method, params } = requests.try_recv().expect("draft submits") else {
        panic!("expected submit");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["prompt"], "hello");
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_none(), "failed accept closes the picker");
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "single submit");
}

#[gpui_kit::test]
fn ctrl_p_recalls_from_multiline_while_arrows_move_caret(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.transcript.push(TranscriptItem::new(
                "history-user-1".into(),
                "user".into(),
                "You".into(),
                "only prompt".into(),
            ));
            view.composer.update(cx, |state, cx| {
                state.set_value("line one\nline two", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("up", cx);
        assert_eq!(
            view.read(cx).composer.read(cx).value(),
            "line one\nline two",
            "arrows keep native caret moves on multiline drafts"
        );
        window.press("ctrl-p", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "only prompt");
        window.press("ctrl-n", cx);
        assert_eq!(
            view.read(cx).composer.read(cx).value(),
            "line one\nline two",
            "ctrl-n restores the stashed multiline draft"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn typing_after_recall_and_escape_clear_abandon_the_recall_cursor(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            for (id, text) in [("history-user-1", "first prompt"), ("history-user-2", "second prompt")] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "user".into(), "You".into(), text.into()));
            }
            view.composer.update(cx, |state, cx| {
                state.set_value("wip", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("up", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "second prompt");
    })
    .unwrap();
    // Separate updates: emitted `Change` effects flush at update boundaries,
    // matching production where each keystroke is its own platform event.
    cx.update_window(window, |_, window, cx| {
        // Typing abandons recall: Down moves the caret instead of restoring the stash.
        window.input(" xyz", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "second prompt xyz", "typing lands in the composer");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.press("down", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "second prompt xyz");
        // Clearing abandons recall: Down cannot resurrect the stash.
        window.press("up", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "second prompt");
        window.press("escape", cx);
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.press("down", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "", "cleared draft stays cleared");
    })
    .unwrap();
}

#[gpui_kit::test]
fn double_escape_clear_is_undoable(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.input("hello", cx);
        window.press("escape", cx);
        window.press("escape", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        // Headless `press` does not deliver the ctrl-z chord; cmd-z drives the same Undo action.
        window.press("cmd-z", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "hello");
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_p_n_navigate_open_suggestions_before_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            for (id, text) in [
                ("history-user-1", "Earlier prompt"),
                ("history-user-2", "Latest prompt"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "user".into(), "You".into(), text.into()));
            }
            view.composer.update(cx, |state, cx| {
                state.set_value("prompt", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        let before = view.read(cx).palette.as_ref().and_then(|state| state.read(cx).selected_index());
        window.press("ctrl-p", cx);
        let after = view.read(cx).palette.as_ref().and_then(|state| state.read(cx).selected_index());
        assert_ne!(before, after, "ctrl-p moves the suggestion highlight");
        assert_eq!(
            view.read(cx).composer.read(cx).value(),
            "prompt",
            "history must not clobber the draft under an open picker"
        );
        window.press("ctrl-n", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "prompt");
    })
    .unwrap();
}

#[gpui_kit::test]
fn suggestion_nav_steps_within_filtered_matches(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            for (id, text) in [
                ("history-user-1", "Review stale diffs"),
                ("history-user-2", "Implement current changes"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "user".into(), "You".into(), text.into()));
            }
            view.composer.update(cx, |state, cx| {
                state.set_value("stale", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        // The seed matches only unfiltered row 1; Down must not collapse the
        // highlight onto the filtered-out row 0.
        window.press("down", cx);
        assert!(
            view.read(cx)
                .palette
                .as_ref()
                .and_then(|state| state.read(cx).selected_index())
                .is_some(),
            "filtered nav keeps a live highlight"
        );
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "Review stale diffs");
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn recall_refreshes_trigger_suggestions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.transcript.push(TranscriptItem::new(
                "history-user-1".into(),
                "user".into(),
                "You".into(),
                "Inspect @src/main.rs".into(),
            ));
            view.composer.update(cx, |state, cx| {
                state.set_value("wip", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("up", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Inspect @src/main.rs");
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        // Recall's silent write emits no Change, so the next-frame refresh is
        // what surfaces the mention palette for Tab completion.
        assert!(view.read(cx).palette.is_some());
        assert!(matches!(view.read(cx).palette_kind, PaletteKind::FileMentions));
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_stays_true_borderless_in_both_focus_states(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let assert_composer_geometry = |window: &gpui_kit::Window| {
            // No separator element: spacing alone separates the text row from
            // the controls in the true-borderless design.
            assert!(window.try_find("composer-controls-divider").is_none());
            let frame = window.find("message-composer-frame").bounds();
            let actions = window.find("composer-actions").bounds();
            let send = window.find("send-prompt").bounds();
            assert!(frame.size.height > px(0.));
            assert!(frame.contains(&actions.center()), "actions must sit inside the composer frame");
            assert!(frame.contains(&send.center()), "send must sit inside the composer frame");
            assert!(actions.top() > frame.top(), "text row must reserve space above the actions");
            // One quiet option group: attach, policy, and mode share a row in
            // leading-to-trailing order.
            let attach = window.find("attach-files").bounds();
            let policy = window.find("composer-policy").bounds();
            let mode = window.find("composer-mode").bounds();
            assert_eq!(attach.top(), policy.top(), "option cluster shares one row");
            assert_eq!(policy.top(), mode.top(), "option cluster shares one row");
            assert!(attach.right() <= policy.left(), "attach precedes policy");
            assert!(policy.right() <= mode.left(), "policy precedes mode");
        };
        // Unfocused side.
        let root_focus = view.read(cx).focus_handle.clone();
        window.focus(&root_focus, cx);
        window.render_frame(cx);
        assert!(!view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        assert_composer_geometry(window);
        // Focused side: geometry must not move.
        view.update(cx, |view, cx| view.activate_composer(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        assert_composer_geometry(window);
    })
    .unwrap();
}

#[gpui_kit::test]
fn focus_composer_shortcut_lands_from_every_route(cx: &mut TestAppContext) {
    use gpui_kit::Focusable;
    let (window, view) = open_desktop(cx);
    // Settings route: focus inside the page, then jump back to the chat input.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        view.update(cx, |view, cx| view.focus_settings_selection(window, cx));
        window.render_frame(cx);
        window.press("cmd-l", cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "cmd-l leaves settings");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window), "cmd-l lands in the chat input");
    })
    .unwrap();
    // Thread browser route.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_thread_browser(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Browser);
        window.press("cmd-l", cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "cmd-l leaves the browser");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
    // Working-tree review route.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.show_review(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Review);
        window.press("cmd-l", cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "cmd-l leaves review");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

fn scroll_transcript_up(window: &AnyWindowHandle, view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update_window(*window, |_, window, cx| {
        // Positive pixel delta scrolls up, away from the tail.
        window.scroll("transcript-scroll", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(120.))), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(250));
    cx.update_window(*window, |_, window, cx| {
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(!scroller.read(cx).is_following_tail(), "fixture must leave the tail");
    })
    .unwrap();
}

fn push_tall_transcript(view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..20 {
                view.transcript.push(TranscriptItem::new(
                    format!("message-{index}"),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("Message {index}\n\n{}", "wrapped words ".repeat(index + 1)),
                ));
            }
        });
    });
}

#[gpui_kit::test]
fn enter_with_draft_while_scrolled_jumps_to_latest_before_submitting(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    push_tall_transcript(&view, cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("enter-jump".into());
            view.composer.update(cx, |state, cx| {
                state.set_value("keep this draft", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
    })
    .unwrap();
    scroll_transcript_up(&window, &view, cx);
    // First Enter reconciles the viewport: jump, keep the draft, send nothing.
    cx.update_window(window, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(scroller.read(cx).is_following_tail(), "first Enter jumps to latest");
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window), "jump keeps composer focus");
        assert!(!view.read(cx).running);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "jump sends nothing");
    // Second Enter submits now that the tail is visible.
    cx.update_window(window, |_, window, cx| {
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_ok(), "second Enter submits");
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
    })
    .unwrap();
}

#[gpui_kit::test]
fn enter_on_empty_transcript_submits_first_message_immediately(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("enter-empty".into());
            view.composer.update(cx, |state, cx| {
                state.set_value("first message", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_ok(), "empty-transcript Enter submits immediately");
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
    })
    .unwrap();
}

#[gpui_kit::test]
fn enter_at_tail_submits_and_empty_enter_stays_put(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    push_tall_transcript(&view, cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("enter-tail".into());
            view.composer.update(cx, |state, cx| {
                state.set_value("send me", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(scroller.read(cx).is_following_tail(), "fixture starts at the tail");
        // At the tail a draft submits on the first Enter.
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_ok(), "tail Enter submits immediately");
    // Scrolled up with an empty draft: Enter neither jumps nor submits.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("", window, cx));
        });
        window.render_frame(cx);
    })
    .unwrap();
    scroll_transcript_up(&window, &view, cx);
    cx.update_window(window, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(!scroller.read(cx).is_following_tail(), "empty Enter stays scrolled");
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "empty Enter sends nothing");
    // Whitespace-only counts as empty, matching the submit guard.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("   ", window, cx));
        });
        window.press("enter", cx);
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(!scroller.read(cx).is_following_tail(), "blank Enter stays scrolled");
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "blank Enter sends nothing");
}
