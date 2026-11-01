use super::super::{QueuePanelState, QueuedSubmission, SubmissionDraft};
use super::*;

fn queued_draft(prompt: &str) -> SubmissionDraft {
    SubmissionDraft {
        prompt: prompt.to_string(),
        attachments: Vec::new(),
        echo_id: format!("echo-{prompt}"),
        delivery_uncertain: false,
    }
}

fn submit_response(method: &str, result: serde_json::Value) -> BackendEvent {
    BackendEvent::Response {
        method: method.to_string(),
        result: Some(result),
        error: None,
    }
}

#[gpui_kit::test]
fn submit_while_running_queues_with_batchable_flag(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.composer
                .update(cx, |state, cx| state.set_value("queued prompt", window, cx));
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.submit_prompt(window, cx);
        });
        let view = view.read(cx);
        assert_eq!(view.composer.read(cx).value(), "");
        assert_eq!(view.queued_submissions.len(), 1);
        assert_eq!(view.queued_submissions[0].prompt, "queued prompt");
        assert!(!view.queued_submissions[0].acknowledged);
        assert!(view.pending_submission.is_none(), "queue path leaves the immediate slot alone");
        assert!(view.transcript.items().any(|item| item.text == "queued prompt"));
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["batchable"], false, "Enter queues a standalone turn");
}

#[gpui_kit::test]
fn submit_stays_blocked_without_backend_or_during_transitions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.composer.update(cx, |state, cx| state.set_value("held draft", window, cx));
            view.submit_prompt(window, cx);
            assert_eq!(view.composer.read(cx).value(), "held draft", "no backend drops the submit");
        });
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.session_request = Some("session/load");
            view.submit_prompt(window, cx);
            assert_eq!(view.composer.read(cx).value(), "held draft", "transition drops the submit");
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn queued_ack_confirms_oldest_first_and_ignores_stale_acks(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".to_string());
        view.running = true;
        view.enqueue_queued_submission(queued_draft("first"), false, cx)
            .expect("queue first");
        view.enqueue_queued_submission(queued_draft("second"), false, cx)
            .expect("queue second");

        let ack = || {
            submit_response(
                vtcode_desktop_protocol::method::TURN_SUBMIT,
                serde_json::json!({ "sessionId": "session", "turnId": "", "queued": true, "queueDepth": 2 }),
            )
        };
        view.handle_backend_event(ack(), cx);
        assert!(view.queued_submissions[0].acknowledged);
        assert!(!view.queued_submissions[1].acknowledged, "FIFO confirmation");
        view.handle_backend_event(ack(), cx);
        assert!(view.queued_submissions.iter().all(|entry| entry.acknowledged));
        view.handle_backend_event(ack(), cx);
        assert_eq!(view.queued_submissions.len(), 2, "stale ack changes nothing");
        assert!(view.pending_submission.is_none());
    });
}

#[gpui_kit::test]
fn queued_ack_demotes_an_in_flight_immediate_submit(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.running = true;
        view.pending_submission = Some(queued_draft("gap submit"));
        view.handle_backend_event(
            submit_response(
                vtcode_desktop_protocol::method::TURN_SUBMIT,
                serde_json::json!({ "sessionId": "session", "turnId": "", "queued": true, "queueDepth": 1 }),
            ),
            cx,
        );
        assert!(view.pending_submission.is_none());
        assert_eq!(view.queued_submissions.len(), 1);
        assert_eq!(view.queued_submissions[0].prompt, "gap submit");
        assert!(view.queued_submissions[0].acknowledged);
        assert!(view.running, "demotion keeps the running state");
    });
}

#[gpui_kit::test]
fn immediate_ack_promotes_a_queued_entry_when_idle_first(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-promoted".to_string(),
            prompt: "promoted".to_string(),
            acknowledged: false,
        });
        view.handle_backend_event(
            submit_response(
                vtcode_desktop_protocol::method::TURN_SUBMIT,
                serde_json::json!({ "sessionId": "session", "turnId": "turn-1", "queued": false }),
            ),
            cx,
        );
        assert!(view.queued_submissions[0].acknowledged);
        assert_eq!(view.active_turn_id.as_deref(), Some("turn-1"));
        assert!(view.running);
    });
}

#[gpui_kit::test]
fn delayed_immediate_ack_leaves_newer_queue_entries_unconfirmed(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        // TurnStarted already cleared the pending submit and set the turn;
        // the queued entry arrived after and awaits its own ack.
        view.active_turn_id = Some("turn-1".to_string());
        view.running = true;
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-newer".to_string(),
            prompt: "newer".to_string(),
            acknowledged: false,
        });
        view.handle_backend_event(
            submit_response(
                vtcode_desktop_protocol::method::TURN_SUBMIT,
                serde_json::json!({ "sessionId": "session", "turnId": "turn-1", "queued": false }),
            ),
            cx,
        );
        assert!(!view.queued_submissions[0].acknowledged, "newer entry keeps awaiting its own ack");
        assert_eq!(view.active_turn_id.as_deref(), Some("turn-1"));
        assert!(view.running);
        view.handle_backend_event(
            submit_response(
                vtcode_desktop_protocol::method::TURN_SUBMIT,
                serde_json::json!({ "sessionId": "session", "turnId": "", "queued": true, "queueDepth": 1 }),
            ),
            cx,
        );
        assert!(view.queued_submissions[0].acknowledged, "its own ack confirms it");
    });
}

#[gpui_kit::test]
fn immediate_ack_without_pending_or_queue_stays_ignored(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.handle_backend_event(
            submit_response(
                vtcode_desktop_protocol::method::TURN_SUBMIT,
                serde_json::json!({ "sessionId": "session", "turnId": "stale", "queued": false }),
            ),
            cx,
        );
        assert!(view.active_turn_id.is_none());
        assert!(!view.running);
    });
}

#[gpui_kit::test]
fn queue_status_reconciles_dispatched_and_ignores_foreign_sessions(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".to_string());
        view.running = true;
        for prompt in ["dispatched", "waiting", "in-flight"] {
            view.enqueue_queued_submission(queued_draft(prompt), false, cx).expect("queue");
        }
        assert!(view.confirm_oldest_queued_submission());
        assert!(view.confirm_oldest_queued_submission());
        let rows_before = view.transcript.len();

        view.handle_backend_event(
            BackendEvent::TurnQueueStatus(serde_json::json!({
                "sessionId": "other",
                "queueDepth": 0,
                "previews": [],
                "droppedTotal": 9,
            })),
            cx,
        );
        assert_eq!(view.queued_submissions.len(), 3, "foreign session ignored");
        assert_eq!(view.queue_dropped_total, 0);

        view.handle_backend_event(
            BackendEvent::TurnQueueStatus(serde_json::json!({
                "sessionId": "session",
                "queueDepth": 1,
                "previews": ["waiting"],
                "droppedTotal": 2,
            })),
            cx,
        );
        assert_eq!(view.queue_depth, 1);
        assert_eq!(view.queue_previews, vec!["waiting".to_string()]);
        assert_eq!(view.queue_dropped_total, 2);
        let prompts: Vec<&str> = view.queued_submissions.iter().map(|entry| entry.prompt.as_str()).collect();
        assert_eq!(prompts, vec!["waiting", "in-flight"], "dispatched confirmed entry leaves from the front");
        assert!(!view.queued_submissions[1].acknowledged, "unconfirmed entry is never truncated");
        assert_eq!(view.transcript.len(), rows_before, "dispatched echo rows stay as history");

        view.handle_backend_event(BackendEvent::TurnQueueStatus(serde_json::json!({"bogus": true})), cx);
        assert_eq!(view.queue_depth, 1, "malformed status ignored");
    });
}

#[gpui_kit::test]
fn submit_error_drops_oldest_unconfirmed_and_keeps_the_turn(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".to_string());
        view.running = true;
        view.active_turn_id = Some("turn-1".to_string());
        view.enqueue_queued_submission(queued_draft("doomed"), false, cx)
            .expect("queue first");
        view.enqueue_queued_submission(queued_draft("spared"), false, cx)
            .expect("queue second");
        assert!(view.confirm_oldest_queued_submission());
        let rows_before = view.transcript.len();

        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::TURN_SUBMIT.to_string(),
                result: None,
                error: Some("rejected".to_string()),
            },
            cx,
        );
        let prompts: Vec<&str> = view.queued_submissions.iter().map(|entry| entry.prompt.as_str()).collect();
        assert_eq!(prompts, vec!["doomed"], "only the oldest unconfirmed entry is dropped");
        assert_eq!(view.transcript.len(), rows_before - 1, "dropped echo row is removed");
        assert!(view.running, "running turn is untouched");
        assert_eq!(view.active_turn_id.as_deref(), Some("turn-1"));
        assert!(view.error.is_some());
    });
}

#[gpui_kit::test]
fn take_latest_restores_newest_text_into_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.enqueue_queued_submission(queued_draft("older"), false, cx)
                .expect("queue older");
            view.enqueue_queued_submission(queued_draft("newer"), false, cx)
                .expect("queue newer");
            assert!(view.confirm_oldest_queued_submission());
            assert!(view.confirm_oldest_queued_submission());
            view.take_latest_queued(cx);
            assert!(view.take_latest_pending);
            view.take_latest_queued(cx);
        });
        // Two queued submits first, then exactly one take-latest (the duplicate
        // take is suppressed).
        for _ in 0..2 {
            let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
                panic!("expected queued submit");
            };
            assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        }
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("expected take-latest request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST);
        assert_eq!(params, serde_json::json!({ "sessionId": "session" }));
        assert!(requests.try_recv().is_err());
        view.update(cx, |view, cx| {
            view.event_tx
                .try_send(submit_response(
                    vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST,
                    serde_json::json!({ "sessionId": "session", "text": "newer", "queueDepth": 1 }),
                ))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(!view.take_latest_pending);
            assert_eq!(view.queue_depth, 1);
            let prompts: Vec<&str> = view.queued_submissions.iter().map(|entry| entry.prompt.as_str()).collect();
            assert_eq!(prompts, vec!["older"]);
            assert_eq!(view.composer.read(cx).value(), "newer", "popped text restores into the composer");
            assert!(!view.transcript.items().any(|item| item.text == "newer"), "popped echo row is removed");
            assert!(view.transcript.items().any(|item| item.text == "older"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn take_latest_keeps_draft_and_retains_popped_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.enqueue_queued_submission(queued_draft("queued"), false, cx)
                .expect("queue");
            assert!(view.confirm_oldest_queued_submission());
            view.composer.update(cx, |state, cx| state.set_value("fresh draft", window, cx));
            view.take_latest_queued(cx);
            assert!(!view.take_latest_pending, "non-empty composer blocks take-latest");
        });
        assert!(requests.try_recv().is_ok(), "queued submit");
        assert!(requests.try_recv().is_err(), "no take-latest request sent");
        view.update(cx, |view, cx| {
            // A draft typed during the round trip retains the popped text instead.
            view.composer.update(cx, |state, cx| state.set_value("", window, cx));
            view.take_latest_queued(cx);
            assert!(view.take_latest_pending);
            view.composer
                .update(cx, |state, cx| state.set_value("typed meanwhile", window, cx));
            view.handle_backend_event(
                submit_response(
                    vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST,
                    serde_json::json!({ "sessionId": "session", "text": "queued", "queueDepth": 0 }),
                ),
                cx,
            );
            assert!(!view.take_latest_pending);
            assert!(view.queued_submissions.is_empty());
            assert_eq!(view.composer.read(cx).value(), "typed meanwhile", "live draft is never clobbered");
            assert_eq!(view.retained_submission.as_ref().map(|draft| draft.prompt.as_str()), Some("queued"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn take_latest_empty_and_error_responses_clear_pending(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".to_string());
        view.queue_depth = 1;
        view.take_latest_queued(cx);
        assert!(view.take_latest_pending);
        view.handle_backend_event(
            submit_response(
                vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST,
                serde_json::json!({ "sessionId": "session", "text": null, "queueDepth": 0 }),
            ),
            cx,
        );
        assert!(!view.take_latest_pending);
        assert_eq!(view.queue_depth, 0);
        assert!(view.pending_composer_restore.is_none());

        view.take_latest_pending = true;
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST.to_string(),
                result: None,
                error: Some("gone".to_string()),
            },
            cx,
        );
        assert!(!view.take_latest_pending, "error releases the in-flight guard");
    });
}

#[gpui_kit::test]
fn take_latest_skips_empty_queue_without_a_request(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".to_string());
        view.take_latest_queued(cx);
        assert!(!view.take_latest_pending);
        view.queue_depth = 1;
        view.backend = None;
        view.take_latest_queued(cx);
        assert!(!view.take_latest_pending, "no backend sends nothing and reports no error");
        assert!(view.error.is_none());
    });
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn queue_panel_state_tracks_entries_depth_and_running(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    view.update(cx, |view, _| {
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden);
        view.running = true;
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden, "no backend means Enter cannot queue");
        view.backend = Some(BackendHandle { commands });
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hint, "running with nothing queued shows the hint");
        view.session_request = Some("session/load");
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden, "transitions hide the hint");
        view.session_request = None;
        view.retained_submission = Some(queued_draft("retained"));
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden, "a retained draft hides the hint");
        view.retained_submission = None;
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-1".to_string(),
            prompt: "queued".to_string(),
            acknowledged: false,
        });
        assert_eq!(view.queue_panel_state(), QueuePanelState::Entries);
        view.queued_submissions.clear();
        view.running = false;
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden);
        view.queue_depth = 2;
        assert_eq!(view.queue_panel_state(), QueuePanelState::Entries, "authoritative depth shows previews");
    });
}

#[gpui_kit::test]
fn edit_newest_button_sends_take_latest(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.enqueue_queued_submission(queued_draft("older"), false, cx)
                .expect("queue older");
            view.enqueue_queued_submission(queued_draft("newer"), false, cx)
                .expect("queue newer");
            assert!(view.confirm_oldest_queued_submission());
            assert!(view.confirm_oldest_queued_submission());
        });
        window.render_frame(cx);
        window.click("edit-newest-queued", cx);
        view.update(cx, |view, _| {
            assert!(view.take_latest_pending);
        });
    })
    .unwrap();
    for _ in 0..2 {
        assert!(requests.try_recv().is_ok(), "queued submits first");
    }
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected take-latest request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST);
    assert_eq!(params, serde_json::json!({ "sessionId": "session" }));
}

#[gpui_kit::test]
fn edit_newest_button_stays_inert_with_a_live_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.enqueue_queued_submission(queued_draft("queued"), false, cx)
                .expect("queue");
            view.composer.update(cx, |state, cx| state.set_value("live draft", window, cx));
        });
        window.render_frame(cx);
        window.click("edit-newest-queued", cx);
        view.update(cx, |view, _| {
            assert!(!view.take_latest_pending);
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_ok(), "queued submit");
    assert!(requests.try_recv().is_err(), "no take-latest request sent");
}

#[gpui_kit::test]
fn queue_drops_surface_a_cumulative_error_once_per_increase(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.handle_backend_event(
            BackendEvent::TurnQueueStatus(serde_json::json!({
                "sessionId": "session",
                "queueDepth": 256,
                "previews": [],
                "droppedTotal": 3,
            })),
            cx,
        );
        let error = view.error.clone().expect("drop error");
        assert!(error.contains('3'), "{error}");
        assert_eq!(view.queue_dropped_total, 3);
        view.handle_backend_event(
            BackendEvent::TurnQueueStatus(serde_json::json!({
                "sessionId": "session",
                "queueDepth": 255,
                "previews": [],
                "droppedTotal": 3,
            })),
            cx,
        );
        assert_eq!(view.error.as_deref(), Some(error.as_str()), "unchanged total does not re-report");
    });
}

#[gpui_kit::test]
fn batched_submit_marks_queued_but_never_immediate(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.composer.update(cx, |state, cx| state.set_value("idle prompt", window, cx));
            view.submit_batched_prompt(window, cx);
        });
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert!(params.get("batchable").is_none(), "immediate submits carry no batchable flag");

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.running = true;
            view.composer
                .update(cx, |state, cx| state.set_value("batched prompt", window, cx));
            view.submit_batched_prompt(window, cx);
            assert_eq!(view.queued_submissions.len(), 1);
        });
    })
    .unwrap();
    let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
        panic!("expected queued request");
    };
    assert_eq!(params["batchable"], true);
}

#[test]
fn queue_batched_shortcut_registers_with_default_binding() {
    let command = super::super::shortcuts::COMMANDS
        .iter()
        .find(|command| command.id == super::super::shortcuts::CommandId::QueueBatched)
        .expect("queue-batched command");
    assert_eq!(command.label_key, "app.shortcuts.queue_batched_label");
    assert_eq!(command.description_key, "app.shortcuts.queue_batched_description");
    assert_eq!(command.default_binding, "cmd-enter");
}

#[gpui_kit::test]
fn queue_state_clears_on_session_reset_and_service_exit(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-1".to_string(),
            prompt: "queued".to_string(),
            acknowledged: true,
        });
        view.queue_depth = 1;
        view.queue_previews = vec!["queued".to_string()];
        view.queue_dropped_total = 3;
        view.take_latest_pending = true;
        view.pending_composer_restore = Some("restoring".to_string());
        view.reset_session_state();
        assert!(view.queued_submissions.is_empty());
        assert_eq!(view.queue_depth, 0);
        assert!(view.queue_previews.is_empty());
        assert_eq!(view.queue_dropped_total, 0);
        assert!(!view.take_latest_pending);
        assert!(view.pending_composer_restore.is_none());

        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-2".to_string(),
            prompt: "queued".to_string(),
            acknowledged: false,
        });
        view.take_latest_pending = true;
        view.handle_backend_event(BackendEvent::Exited { status: None, authentication_required: false }, cx);
        assert!(view.queued_submissions.is_empty());
        assert!(!view.take_latest_pending);
    });
}
