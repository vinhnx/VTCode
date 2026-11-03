use super::super::{QueuedSubmission, SubmissionDraft};
use super::*;
use vtcode_desktop_protocol::SessionSummary;

fn summary(session_id: &str, title: &str) -> SessionSummary {
    SessionSummary {
        session_id: session_id.into(),
        title: Some(title.into()),
        workspace: std::env::temp_dir().display().to_string(),
        completed: false,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    }
}

fn open_running_desktop(
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
        view.running = true;
        view.active_turn_id = Some("turn-1".into());
    });
    (window, view, requests)
}

fn turn_completed(session_id: &str, turn_id: &str) -> BackendEvent {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};

    BackendEvent::ThreadEvent(ThreadEventNotification {
        session_id: session_id.into(),
        turn_id: Some(turn_id.into()),
        event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
            },
            in_progress_exec_sessions: Vec::new(),
        })),
    })
}

fn take_request(requests: &mut tokio::sync::mpsc::Receiver<BackendCommand>) -> (String, serde_json::Value) {
    let BackendCommand::Request { method, params } = requests.try_recv().expect("a backend request was sent") else {
        panic!("expected a session request");
    };
    (method, params)
}

#[gpui_kit::test]
fn running_thread_marks_every_surface_and_keeps_new_thread_enabled(cx: &mut TestAppContext) {
    let (window, view, _) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Tab strip: only the running tab carries the marker and label.
        assert!(window.try_find("thread-tab-running-thread-b").is_some());
        assert!(window.try_find("thread-tab-running-thread-a").is_none());
        assert!(window.try_find("thread-tab-running-thread-c").is_none());
        assert_eq!(window.find("thread-tab-thread-b").label(), Some("Second thread (running)"));
        assert_eq!(window.find("thread-tab-thread-a").label(), Some("Switch to thread: First thread"));
        assert_ne!(window.find("thread-tab-new").disabled(), Some(true), "new thread stays enabled mid-turn");

        // Sidebar rows mirror the strip; the selected row is not a load target.
        assert!(window.try_find("session-running-thread-b").is_some());
        assert!(window.try_find("session-running-thread-a").is_none());
        assert_eq!(window.find("session-thread-b").label(), Some("Second thread (running)"));
        assert_ne!(window.find("session-thread-a").disabled(), Some(true), "idle threads stay loadable mid-turn");
        assert_eq!(window.find("session-thread-b").disabled(), Some(true), "the running thread is not a switch target");

        // Browser rows mirror the sidebar.
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-browser").is_some());
        assert!(window.try_find("browser-session-running-thread-b").is_some());
        assert!(window.try_find("browser-session-running-thread-a").is_none());
        assert_eq!(window.find("browser-session-thread-b").label(), Some("Second thread (running)"));

        // The lone-tab header keeps its own marker and new-thread action.
        view.update(cx, |view, _| view.open_tabs = vec!["thread-b".into()]);
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        assert!(window.try_find("thread-tabs").is_none(), "a lone tab hides the strip");
        assert!(window.try_find("thread-header-running").is_some());
        assert_eq!(window.find("thread-header").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("thread-header").label(), Some("Second thread (running)"));
        assert_ne!(window.find("thread-header-new").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn new_thread_while_running_confirms_then_stops_and_proceeds(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err(), "new thread waits for the confirm answer");
        assert_eq!(window.find("running-confirm-dialog").role(), Some(gpui_kit::Role::Dialog));
        assert!(window.try_find("running-confirm-body").is_some());
        assert!(window.try_find("running-confirm-queue-warning").is_none(), "empty queue shows no warning");

        window.click("running-confirm-stop", cx);
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
        assert_eq!(params["sessionId"].as_str(), Some("thread-b"));
        assert_eq!(params["turnId"].as_str(), Some("turn-1"));
        assert!(requests.try_recv().is_err(), "the close waits for the turn to finish");
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_none(), "confirming closes the modal");
        assert!(view.read(cx).armed_running_switch.is_some(), "the new-thread intent arms");

        // The deferred close fires on the running-to-idle transition.
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
        assert_eq!(params["sessionId"].as_str(), Some("thread-b"));
        assert!(requests.try_recv().is_err(), "one finish drains one intent");
        view.update(cx, |view, _| {
            assert!(view.armed_running_switch.is_none(), "the drain consumes the armed intent");
            assert!(view.pending_new_thread, "the deferred close chains into a new thread");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn declining_running_confirm_sends_nothing_and_keeps_state(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-thread-a", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err(), "switching waits for the confirm answer");
        assert!(window.try_find("running-confirm-dialog").is_some());

        window.click("running-confirm-cancel", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err(), "declining sends nothing");
        assert!(window.try_find("running-confirm-dialog").is_none());
        view.update(cx, |view, _| {
            assert_eq!(view.selected_session_id.as_deref(), Some("thread-b"));
            assert!(view.running, "declining never touches the turn");
            assert!(view.armed_running_switch.is_none());
            assert_eq!(view.open_tabs.as_slice(), ["thread-a", "thread-b", "thread-c"]);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn running_confirm_warns_about_queued_prompts_only_when_queued(cx: &mut TestAppContext) {
    let (window, view, _) = open_running_desktop(cx);
    view.update(cx, |view, _| {
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-queued".into(),
            prompt: "queued behind the turn".into(),
            acknowledged: false,
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-queue-warning").is_some(), "a queued prompt warns");
        window.click("running-confirm-cancel", cx);
        window.render_frame(cx);

        view.update(cx, |view, _| view.queued_submissions.clear());
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some());
        assert!(window.try_find("running-confirm-queue-warning").is_none(), "an empty queue stays quiet");
    })
    .unwrap();
}

#[gpui_kit::test]
fn switch_thread_while_running_defers_load_until_turn_finishes(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("session-thread-a", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err(), "the load waits for the confirm answer");
        assert!(window.try_find("running-confirm-dialog").is_some());

        window.click("running-confirm-stop", cx);
        let (method, _) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
        assert!(requests.try_recv().is_err(), "the load waits for the turn to finish");

        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
        assert_eq!(params["sessionId"].as_str(), Some("thread-a"), "the deferred load targets the picked thread");
        assert!(requests.try_recv().is_err(), "one finish drains one intent");
        view.update(cx, |view, _| assert!(view.armed_running_switch.is_none()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn confirm_after_natural_finish_proceeds_without_cancel(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some());

        // The turn finishes on its own before the user answers.
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        assert!(requests.try_recv().is_err(), "nothing is armed, so nothing drains");
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some(), "the modal outlives the turn");

        window.click("running-confirm-stop", cx);
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
        assert_eq!(params["sessionId"].as_str(), Some("thread-b"));
        assert!(requests.try_recv().is_err(), "no cancel follows a finished turn");
        view.update(cx, |view, _| assert!(view.armed_running_switch.is_none()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn armed_switch_dropped_on_service_exit(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        window.click("running-confirm-stop", cx);
        assert!(take_request(&mut requests).0 == vtcode_desktop_protocol::method::TURN_CANCEL);
        view.update(cx, |view, _| assert!(view.armed_running_switch.is_some()));

        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::Exited { status: None, authentication_required: false }, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, _| {
            assert!(view.armed_running_switch.is_none(), "exit clears the armed wait");
            assert!(view.running_confirm.is_none(), "exit clears the modal");
        });
        assert!(window.try_find("running-confirm-dialog").is_none());
        assert!(requests.try_recv().is_err(), "a dead service receives no deferred send");
    })
    .unwrap();
}

#[gpui_kit::test]
fn close_session_action_while_running_opens_confirm_and_escapes_cleanly(cx: &mut TestAppContext) {
    use super::super::CloseSession;

    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_action(Box::new(CloseSession), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some(), "the widened action opens the modal");
        assert!(requests.try_recv().is_err());

        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_none(), "escape declines the modal");
        assert!(requests.try_recv().is_err(), "escaping sends nothing");
        view.update(cx, |view, _| {
            assert_eq!(view.selected_session_id.as_deref(), Some("thread-b"));
            assert!(view.running);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn armed_switch_dropped_when_the_turn_dies_before_starting(cx: &mut TestAppContext) {
    for event in [
        BackendEvent::TurnBlockedByStartupInteraction { session_id: "thread-b".into() },
        BackendEvent::Response {
            method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
            result: None,
            error: Some("submit rejected".into()),
        },
    ] {
        let (window, view, mut requests) = open_running_desktop(cx);
        view.update(cx, |view, _| {
            view.active_turn_id = None;
            view.pending_submission = Some(SubmissionDraft {
                prompt: "in flight".into(),
                attachments: Vec::new(),
                echo_id: "echo-pending".into(),
                delivery_uncertain: false,
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("thread-tab-new", cx);
            window.render_frame(cx);
            window.click("running-confirm-stop", cx);
            assert!(requests.try_recv().is_err(), "cancel without a turn id only queues");
            view.update(cx, |view, _| assert!(view.armed_running_switch.is_some()));

            view.update(cx, |view, cx| view.handle_backend_event(event, cx));
            assert!(requests.try_recv().is_err(), "a dead turn sends no deferred switch");
            view.update(cx, |view, _| {
                assert!(view.armed_running_switch.is_none(), "the dead turn drops the armed wait");
                assert!(!view.running);
                assert!(view.error.is_some(), "the arm's own error explains the stall");
            });
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn slash_new_and_exit_route_through_running_confirm_mid_turn(cx: &mut TestAppContext) {
    use super::super::composer_controls::DesktopSlashAction;

    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("/new", window, cx));
            assert!(view.handle_desktop_slash_action(DesktopSlashAction::NewSession, window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some(), "/new opens the modal mid-turn");
        assert!(requests.try_recv().is_err());
        assert!(view.read(cx).composer.read(cx).value().is_empty(), "the modal captures the command");
        window.press("escape", cx);
        window.render_frame(cx);

        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("/exit", window, cx));
            assert!(view.handle_desktop_slash_action(DesktopSlashAction::CloseSession, window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some(), "/exit opens the modal mid-turn");
        window.click("running-confirm-stop", cx);
        let (method, _) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
        assert_eq!(params["sessionId"].as_str(), Some("thread-b"));
    })
    .unwrap();

    // Idle slash behavior is unchanged: /new sends the close immediately.
    let (window, view, mut requests) = open_running_desktop(cx);
    view.update(cx, |view, _| {
        view.running = false;
        view.active_turn_id = None;
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("/new", window, cx));
            assert!(view.handle_desktop_slash_action(DesktopSlashAction::NewSession, window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_none(), "idle /new skips the modal");
        let (method, _) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    })
    .unwrap();
}

#[gpui_kit::test]
fn toolbar_new_thread_stays_enabled_mid_turn_when_rail_hidden(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    view.update(cx, |view, _| view.preferences.navigation_visible = Some(false));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("top-new-thread").is_some(), "hidden rail moves New thread to the toolbar");
        assert_ne!(window.find("top-new-thread").disabled(), Some(true), "toolbar New stays enabled mid-turn");
        window.click("top-new-thread", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some());
        assert!(requests.try_recv().is_err());
    })
    .unwrap();

    // Without a backend the same button stays disabled: only a running turn
    // reroutes, nothing else relaxes the gate.
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| view.preferences.navigation_visible = Some(false));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("top-new-thread").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn running_switch_funnel_blocks_reentrant_intents(cx: &mut TestAppContext) {
    // An armed wait shuts the funnel: entries go dark and direct routing
    // refuses, so the original intent survives to the drain intact.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        window.click("running-confirm-stop", cx);
        let _ = take_request(&mut requests);
        window.render_frame(cx);
        assert_eq!(window.find("thread-tab-new").disabled(), Some(true), "armed wait darkens New");
        assert_eq!(window.find("session-thread-a").disabled(), Some(true), "armed wait darkens switches");
        view.update(cx, |view, cx| {
            assert!(!view.load_session_from_ui("thread-a".into(), window, cx), "armed wait refuses routing");
            assert!(view.running_confirm.is_none(), "no second modal over an armed wait");
        });
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, _) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE, "original New intent drains");
    })
    .unwrap();

    // An open modal cannot be swapped by a second open: the first intent wins.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-thread-a", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.open_running_confirm(super::super::RunningSwitchIntent::NewThread, window, cx);
        });
        window.click("running-confirm-stop", cx);
        let _ = take_request(&mut requests);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD, "reopen never swaps the intent");
        assert_eq!(params["sessionId"].as_str(), Some("thread-a"));
    })
    .unwrap();

    // A modal that outlives its turn still owns the funnel: no idle
    // direct-send slips past it, but answering still proceeds.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("thread-tab-new").disabled(), Some(true), "stale modal keeps the funnel");
        view.update(cx, |view, cx| {
            assert!(!view.create_session_from_ui(window, cx), "no direct send past a stale modal");
        });
        assert!(requests.try_recv().is_err());
        window.click("running-confirm-stop", cx);
        let (method, _) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE, "answering still proceeds");
    })
    .unwrap();
}

#[gpui_kit::test]
fn tab_close_mid_turn_confirms_then_untabs_on_drain(cx: &mut TestAppContext) {
    // X on the running tab confirms the neighbor switch, then untabs.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-close-thread-b", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some(), "closing the running tab confirms");
        assert!(requests.try_recv().is_err());
        view.update(cx, |view, _| assert_eq!(view.open_tabs.as_slice(), ["thread-a", "thread-b", "thread-c"]));
        window.click("running-confirm-stop", cx);
        let _ = take_request(&mut requests);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
        assert_eq!(params["sessionId"].as_str(), Some("thread-c"), "next-first neighbor wins");
        view.update(cx, |view, _| {
            assert_eq!(view.open_tabs.as_slice(), ["thread-a", "thread-c"], "closed tab untabs at drain");
        });
    })
    .unwrap();

    // Declining keeps the tab; closing a non-selected tab never confirms.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-close-thread-b", cx);
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err(), "declining sends nothing");
        view.update(cx, |view, _| assert_eq!(view.open_tabs.as_slice(), ["thread-a", "thread-b", "thread-c"]));
        window.click("thread-tab-close-thread-a", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_none(), "idle tabs untab freely mid-turn");
        view.update(cx, |view, _| assert_eq!(view.open_tabs.as_slice(), ["thread-b", "thread-c"]));
    })
    .unwrap();

    // Close-others mid-turn confirms the selection switch, then untabs all.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.close_other_tabs("thread-a", window, cx));
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some());
        window.click("running-confirm-stop", cx);
        let _ = take_request(&mut requests);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
        assert_eq!(params["sessionId"].as_str(), Some("thread-a"));
        view.update(cx, |view, _| assert_eq!(view.open_tabs.as_slice(), ["thread-a"]));
    })
    .unwrap();
}

#[gpui_kit::test]
fn close_thread_while_running_confirms_then_closes_on_drain(cx: &mut TestAppContext) {
    use super::super::CloseSession;

    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_action(Box::new(CloseSession), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some());
        window.click("running-confirm-stop", cx);
        let (method, _) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
        assert_eq!(params["sessionId"].as_str(), Some("thread-b"));
        view.update(cx, |view, _| {
            assert!(!view.pending_new_thread, "Close drains without chaining a new thread");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn armed_switch_fails_loud_when_blocked_at_drain(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        window.click("running-confirm-stop", cx);
        let _ = take_request(&mut requests);
        // A decision lands while the turn stops: the deferred send refuses.
        view.update(cx, |view, _| {
            view.interaction = Some(vtcode_desktop_protocol::InteractionRequest {
                session_id: "thread-b".into(),
                turn_id: "turn-1".into(),
                interaction_id: "decision-1".into(),
                kind: vtcode_desktop_protocol::InteractionKind::Approval,
                wizard: false,
                title: "Approve?".into(),
                details: Vec::new(),
                choices: Vec::new(),
                steps: Vec::new(),
                selected_choice_id: None,
                selected_choice_ids: Vec::new(),
            });
        });
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        assert!(requests.try_recv().is_err(), "a blocked drain sends nothing");
        view.update(cx, |view, _| {
            assert!(view.armed_running_switch.is_none(), "the failed intent is consumed, not stuck");
            assert_eq!(view.error.as_deref(), Some("The turn stopped, but the thread switch failed. Try again."));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn load_after_natural_finish_proceeds_without_cancel(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("thread-tab-thread-a", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        assert!(requests.try_recv().is_err(), "nothing is armed, so nothing drains");
        window.click("running-confirm-stop", cx);
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
        assert_eq!(params["sessionId"].as_str(), Some("thread-a"));
        assert!(requests.try_recv().is_err(), "no cancel follows a finished turn");
    })
    .unwrap();
}

#[gpui_kit::test]
fn browser_row_cancel_returns_focus_to_browser(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("nav-browse-threads", cx);
        window.render_frame(cx);
        window.click("browser-session-thread-a", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some());
        window.click("running-confirm-cancel", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err());
        assert!(window.try_find("thread-browser").is_some(), "the browser stays open behind the modal");
        assert!(
            view.read(cx).browser_list_focus.is_focused(window),
            "cancel returns focus to the visible route's list"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_dismisses_running_confirm_before_full_access(cx: &mut TestAppContext) {
    let (window, view, _) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.open_full_access_confirm(
                vtcode_desktop_protocol::SessionOptionsSelectParams {
                    permission_mode: Some(vtcode_desktop_protocol::PermissionMode::FullAccess),
                    ..Default::default()
                },
                window,
                cx,
            );
            view.open_running_confirm(super::super::RunningSwitchIntent::NewThread, window, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_some());
        assert!(window.try_find("full-access-confirm-dialog").is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("running-confirm-dialog").is_none(), "escape takes the topmost modal first");
        assert!(window.try_find("full-access-confirm-dialog").is_some(), "the modal below survives");
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find("full-access-confirm-dialog").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn stop_cancel_flag_follows_armed_wait(cx: &mut TestAppContext) {
    // A plain stop never discards the queue.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.cancel_turn(cx));
        let (method, params) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
        assert_eq!(params["clearQueue"].as_bool(), Some(false), "plain stops keep the queue");
    })
    .unwrap();

    // A confirmed switch flags its cancel even when a plain stop went first.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.cancel_turn(cx));
        let (_, plain) = take_request(&mut requests);
        assert_eq!(plain["clearQueue"].as_bool(), Some(false));
        window.click("thread-tab-new", cx);
        window.render_frame(cx);
        window.click("running-confirm-stop", cx);
        let (method, flagged) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL, "the flagged cancel re-sends");
        assert_eq!(flagged["clearQueue"].as_bool(), Some(true), "the switch discards the queue");
        assert!(requests.try_recv().is_err());
    })
    .unwrap();
}

#[gpui_kit::test]
fn delete_selected_thread_refused_in_every_state_but_idle_other_still_prompts(cx: &mut TestAppContext) {
    use super::super::tabs::DeleteThread;

    // The server rejects selected-session deletes, so the selected thread
    // refuses with guidance instead of prompting — running or idle.
    for running in [true, false] {
        let (window, view, mut requests) = open_running_desktop(cx);
        view.update(cx, |view, _| {
            view.running = running;
            view.active_turn_id = running.then(|| "turn-1".into());
        });
        cx.update_window(window, |_, window, cx| {
            window.dispatch_action(Box::new(DeleteThread { session_id: "thread-b".into() }), cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(requests.try_recv().is_err(), "the selected delete sends nothing");
        view.update(cx, |view, _| {
            assert_eq!(view.sessions.len(), 3);
            assert_eq!(
                view.error.as_deref(),
                Some("Switch to another thread before deleting this one."),
                "the refusal names the remedy (running = {running})"
            );
        });
    }

    // A non-selected thread still takes the confirm-gated path, even mid-turn.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(DeleteThread { session_id: "thread-a".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "delete waits for the warning prompt answer");
    view.update(cx, |view, _| {
        assert_eq!(view.sessions.len(), 3);
        assert!(view.error.is_none(), "the non-selected delete raises no refusal");
    });
}

#[gpui_kit::test]
fn repeated_cancel_presses_resend_turn_cancel(cx: &mut TestAppContext) {
    // A first stop signal can be lost when the runloop resets its latch
    // while processing the Submit; every press must re-send so repeats
    // re-assert the out-of-band signal instead of wedging silently.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        for press in 1..=2 {
            view.update(cx, |view, cx| view.cancel_turn(cx));
            let (method, params) = take_request(&mut requests);
            assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL, "press {press} sends");
            assert_eq!(
                params,
                json!({ "sessionId": "thread-b", "turnId": "turn-1", "clearQueue": false }),
                "press {press} names the live turn"
            );
        }
        assert!(view.read(cx).cancel_requested);
        assert!(view.read(cx).cancel_request_sent);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "no duplicate send for the same presses");
    view.update(cx, |view, cx| {
        assert!(!view.running);
        view.cancel_turn(cx);
    });
    assert!(requests.try_recv().is_err(), "an idle press sends nothing");
}

#[gpui_kit::test]
fn stale_cancel_error_reconciles_running_state(cx: &mut TestAppContext) {
    // The service rejected our cancel for the live turn, so the turn is
    // already gone server-side (e.g. a dropped terminal event): clear the
    // stale running state instead of spinning forever.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.cancel_turn(cx));
        let (method, _) = take_request(&mut requests);
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
        view.update(cx, |view, _| {
            assert_eq!(view.cancel_sent_turn_id.as_deref(), Some("turn-1"));
        });
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_CANCEL.into(),
                    result: None,
                    error: Some("turn is no longer active".into()),
                },
                cx,
            );
        });
        view.update(cx, |view, _| {
            assert!(!view.running, "stale running state reconciles");
            assert!(view.active_turn_id.is_none());
            assert!(!view.cancel_requested);
            assert!(!view.cancel_request_sent);
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "reconcile sends nothing further");
}

#[gpui_kit::test]
fn late_cancel_error_ignores_newer_turn(cx: &mut TestAppContext) {
    // Asymmetric guard: an error for an older cancel must not clear a
    // newer turn that started since the send.
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.cancel_turn(cx));
        let _ = take_request(&mut requests);
        view.update(cx, |view, cx| {
            view.handle_backend_event(turn_completed("thread-b", "turn-1"), cx);
        });
        // A newer turn starts before the stale error lands.
        view.update(cx, |view, _| {
            view.running = true;
            view.active_turn_id = Some("turn-2".into());
        });
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_CANCEL.into(),
                    result: None,
                    error: Some("turn is no longer active".into()),
                },
                cx,
            );
        });
        view.update(cx, |view, _| {
            assert!(view.running, "the newer turn keeps running");
            assert_eq!(view.active_turn_id.as_deref(), Some("turn-2"));
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "the late error sends nothing");
}

#[gpui_kit::test]
fn cancel_button_click_sends_turn_cancel(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("cancel-turn", cx);
    })
    .unwrap();
    let (method, params) = take_request(&mut requests);
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
    assert_eq!(
        params,
        json!({ "sessionId": "thread-b", "turnId": "turn-1", "clearQueue": false }),
        "the button names the live turn"
    );
    assert!(requests.try_recv().is_err(), "one click sends once");
    cx.update(|cx| {
        assert!(view.read(cx).cancel_requested);
        assert!(view.read(cx).cancel_request_sent);
    });
}
