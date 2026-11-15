use super::*;

#[gpui_kit::test]
fn tab_keypress_queues_batchable_turn_while_running(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.composer.update(cx, |state, cx| {
                state.set_value("tabbed follow-up", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("tab", cx);
        let view = view.read(cx);
        assert_eq!(view.composer.read(cx).value(), "");
        assert_eq!(view.queued_submissions.len(), 1);
        assert_eq!(view.queued_submissions[0].prompt, "tabbed follow-up");
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected queued turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["batchable"], true, "tab queues a batchable turn like cmd-enter");
    assert!(requests.try_recv().is_err(), "one keypress sends exactly one turn");
}

#[gpui_kit::test]
fn tab_keypress_submits_immediately_while_idle(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("idle tab submit", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("tab", cx);
        let view = view.read(cx);
        assert_eq!(view.composer.read(cx).value(), "");
        assert!(view.running, "idle tab submits the draft");
        assert!(view.pending_submission.is_some());
        assert!(view.queued_submissions.is_empty());
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert!(params.get("batchable").is_none(), "immediate submits carry no batchable flag");
}

#[gpui_kit::test]
fn tab_with_empty_draft_keeps_focus_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window), "fixture focuses the composer");
        window.press("tab", cx);
        window.render_frame(cx);
        assert!(
            !view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "empty tab moves focus instead of queueing"
        );
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| {
                state.set_value("   ", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("tab", cx);
        window.render_frame(cx);
        assert!(
            !view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "whitespace-only tab moves focus too"
        );
        assert!(view.read(cx).queued_submissions.is_empty());
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "navigation tabs send nothing");
}

#[gpui_kit::test]
fn running_draft_swaps_stop_for_queue_button_and_back(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
        });
        window.render_frame(cx);
        assert!(window.try_find("cancel-turn").is_some(), "empty draft keeps stop");
        assert!(window.try_find("queue-prompt").is_none());

        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("   ", window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("cancel-turn").is_some(), "whitespace keeps stop");

        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("queue me", window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("cancel-turn").is_none(), "draft swaps stop out");
        let queue = window.find("queue-prompt");
        assert_eq!(queue.label(), Some("Queue message"));
        assert_ne!(queue.disabled(), Some(true), "queue stays actionable");
        window.click("queue-prompt", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        assert_eq!(view.read(cx).queued_submissions.len(), 1);
        window.render_frame(cx);
        assert!(window.try_find("cancel-turn").is_some(), "sending restores stop");
        assert!(window.try_find("queue-prompt").is_none());
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected queued turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["batchable"], false, "the queue button queues a standalone turn like enter");
}
