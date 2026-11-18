use super::super::{QueuePanelState, QueuedSubmission, SubmissionDraft};
use super::*;

fn queue_response(method: &str, result: serde_json::Value) -> BackendEvent {
    BackendEvent::Response {
        method: method.to_string(),
        result: Some(result),
        error: None,
    }
}

#[gpui_kit::test]
fn take_latest_empty_result_clears_confirmed_rows_to_depth(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.running = true;
        for (echo, acknowledged) in [("echo-acked", true), ("echo-flight", false)] {
            view.queued_submissions.push(QueuedSubmission {
                echo_id: echo.to_string(),
                entry_id: None,
                prompt: echo.to_string(),
                user_attachments: Vec::new(),
                acknowledged,
            });
        }
        view.handle_backend_event(
            queue_response(
                vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST,
                serde_json::json!({ "sessionId": "session", "text": null, "queueDepth": 0 }),
            ),
            cx,
        );
        // The service queue is empty and no snapshot follows an empty
        // take: confirmed rows clear now, while the unconfirmed entry
        // keeps awaiting its own ack.
        let prompts: Vec<&str> = view.queued_submissions.iter().map(|entry| entry.prompt.as_str()).collect();
        assert_eq!(prompts, vec!["echo-flight"]);
        assert_eq!(view.queue_depth, 0);
        assert_eq!(view.queue_panel_state(), QueuePanelState::Entries, "in-flight row keeps the panel");
    });
}

#[gpui_kit::test]
fn take_latest_result_truncates_concurrently_dispatched_rows(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.running = true;
        for echo in ["echo-older", "echo-newer"] {
            view.queued_submissions.push(QueuedSubmission {
                echo_id: echo.to_string(),
                entry_id: None,
                prompt: echo.to_string(),
                user_attachments: Vec::new(),
                acknowledged: true,
            });
        }
        // The service popped the newest while the older entry dispatched
        // concurrently (its snapshot never landed): depth 0 clears both.
        view.handle_backend_event(
            queue_response(
                vtcode_desktop_protocol::method::TURN_QUEUE_TAKE_LATEST,
                serde_json::json!({ "sessionId": "session", "text": "echo-newer", "queueDepth": 0 }),
            ),
            cx,
        );
        assert!(view.queued_submissions.is_empty());
        assert_eq!(view.queue_depth, 0);
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden);
        assert_eq!(view.pending_composer_restore.as_deref(), Some("echo-newer"));
    });
}

#[gpui_kit::test]
fn take_latest_waits_for_pending_acks(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".to_string());
            view.running = true;
            view.composer.update(cx, |state, cx| state.set_value("", window, cx));
            view.enqueue_queued_submission(
                SubmissionDraft {
                    prompt: "awaiting ack".to_string(),
                    attachments: Vec::new(),
                    echo_id: "echo-flight".to_string(),
                    delivery_uncertain: false,
                },
                false,
                cx,
            )
            .expect("queue");
            // Ack still in flight: the service pop cannot map to a row.
            view.take_latest_queued(cx);
            assert!(!view.take_latest_pending, "unacked entries block the pop");
        });
        assert!(requests.try_recv().is_ok(), "queued submit sent");
        assert!(requests.try_recv().is_err(), "no take-latest while unacked");
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            // Editing is now inline per row: the unacked (unbound) row has no
            // wire id, so it cannot open an editor until a snapshot binds it.
            view.start_queue_edit("echo-flight", window, cx);
            assert!(view.editing_queue_echo_id.is_none(), "unbound row has no inline editor");
            assert!(view.confirm_oldest_queued_submission(None));
            view.take_latest_queued(cx);
            assert!(view.take_latest_pending, "acked entries pop");
        });
        assert!(requests.try_recv().is_ok(), "take-latest sent after the ack");
    })
    .unwrap();
}

#[gpui_kit::test]
fn remove_response_converges_depth_and_clears_panel(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".to_string());
        view.running = true;
        for (echo, entry_id) in [("echo-older", "qe-1"), ("echo-newer", "qe-2")] {
            view.queued_submissions.push(QueuedSubmission {
                echo_id: echo.to_string(),
                entry_id: Some(entry_id.to_string()),
                prompt: echo.to_string(),
                user_attachments: Vec::new(),
                acknowledged: true,
            });
        }
        view.queue_depth = 2;
        view.queue_previews = vec!["echo-older".to_string(), "echo-newer".to_string()];
        // Remove the newest: the response converges depth without a snapshot.
        view.remove_queued_submission("echo-newer", cx);
        view.handle_backend_event(
            queue_response(
                vtcode_desktop_protocol::method::TURN_QUEUE_REMOVE,
                serde_json::json!({ "sessionId": "session", "removed": true, "queueDepth": 1 }),
            ),
            cx,
        );
        assert_eq!(view.queue_depth, 1);
        assert_eq!(view.queue_panel_state(), QueuePanelState::Entries);
        // Remove the last entry (lost here to a dispatch race): the panel
        // still clears on the response alone.
        view.remove_queued_submission("echo-older", cx);
        view.handle_backend_event(
            queue_response(
                vtcode_desktop_protocol::method::TURN_QUEUE_REMOVE,
                serde_json::json!({ "sessionId": "session", "removed": false, "queueDepth": 0 }),
            ),
            cx,
        );
        assert_eq!(view.queue_depth, 0);
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden, "empty queue clears without a snapshot");
    });
    for _ in 0..2 {
        let BackendCommand::Request { method, .. } = requests.try_recv().expect("remove request") else {
            panic!("expected remove request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_QUEUE_REMOVE);
    }
}

#[gpui_kit::test]
fn explicit_queue_outcomes_only_echo_the_dispatched_entry(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".into());
        for entry_id in ["discarded", "sent", "waiting"] {
            view.queued_submissions.push(QueuedSubmission {
                echo_id: entry_id.into(),
                entry_id: None,
                prompt: "identical text".into(),
                user_attachments: Vec::new(),
                acknowledged: false,
            });
            view.handle_backend_event(
                queue_response(
                    vtcode_desktop_protocol::method::TURN_SUBMIT,
                    serde_json::json!({
                        "sessionId": "session", "turnId": "", "queued": true, "entryId": entry_id,
                    }),
                ),
                cx,
            );
        }
        // A removal that raced with dispatch cannot delete the local echo
        // payload before the dispatch outcome arrives.
        view.handle_backend_event(
            queue_response(
                vtcode_desktop_protocol::method::TURN_QUEUE_REMOVE,
                serde_json::json!({
                    "sessionId": "session", "removed": false, "entryId": "sent", "queueDepth": 1,
                }),
            ),
            cx,
        );
        assert_eq!(view.queued_submissions.len(), 3);
        view.handle_backend_event(
            BackendEvent::TurnQueueStatus(serde_json::json!({
                "sessionId": "session", "queueDepth": 1, "entryIds": ["waiting"],
                "dispatchedEntryIds": ["sent"], "discardedEntryIds": ["discarded"],
            })),
            cx,
        );

        assert_eq!(view.queued_submissions.len(), 1);
        assert_eq!(view.queued_submissions[0].entry_id.as_deref(), Some("waiting"));
        let ids: Vec<_> = view.transcript.items().map(|item| item.id.as_str()).collect();
        assert_eq!(ids.iter().filter(|id| **id == "sent").count(), 1);
        assert!(!ids.contains(&"discarded"));
        assert!(!ids.contains(&"waiting"));

        for snapshot in [
            serde_json::json!({
                "sessionId": "session", "queueDepth": 0, "dispatchedEntryIds": ["waiting"],
            }),
            serde_json::json!({
                "sessionId": "session", "queueDepth": 1, "entryIds": ["waiting"],
                "previews": ["identical text"], "dispatchedEntryIds": [],
            }),
        ] {
            view.handle_backend_event(BackendEvent::TurnQueueStatus(snapshot), cx);
        }
        assert_eq!(view.queue_panel_state(), QueuePanelState::Hidden);
        assert_eq!(view.transcript.items().filter(|item| item.id == "waiting").count(), 1);
    });
}
