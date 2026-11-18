use super::super::QueuedSubmission;
use super::*;

#[gpui_kit::test]
fn a_second_steer_is_suppressed_while_a_receipt_is_pending(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, cx| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".to_string());
        for (echo, entry, prompt) in [("echo-a", "qe-1", "first"), ("echo-b", "qe-2", "second")] {
            view.queued_submissions.push(QueuedSubmission {
                echo_id: echo.into(),
                entry_id: Some(entry.into()),
                prompt: prompt.into(),
                user_attachments: Vec::new(),
                acknowledged: true,
            });
        }
        view.steer_queued_submission("echo-a", cx);
        view.steer_queued_submission("echo-b", cx);
        assert_eq!(view.steer_pending.len(), 1, "only one steer may be in flight");
    });
    assert!(requests.try_recv().is_ok(), "the first steer request is sent");
    assert!(requests.try_recv().is_err(), "the second steer must be suppressed while pending");
}

#[gpui_kit::test]
fn steer_result_acceptance_removes_only_the_addressed_row(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-steer".into(),
            entry_id: Some("qe-1".into()),
            prompt: "ship it".into(),
            user_attachments: Vec::new(),
            acknowledged: true,
        });
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-other".into(),
            entry_id: Some("qe-2".into()),
            prompt: "stay queued".into(),
            user_attachments: Vec::new(),
            acknowledged: true,
        });
        let _ = view.steer_pending.insert("echo-steer".into(), "receipt-1".into());
        view.handle_backend_event(
            BackendEvent::TurnSteerResult(serde_json::json!({
                "sessionId": "session",
                "turnId": "turn-1",
                "receiptId": "receipt-1",
                "entryId": "qe-1",
                "accepted": true,
                "intentId": "intent-1",
                "queueDepth": 1,
            })),
            cx,
        );
        assert_eq!(view.queued_submissions.len(), 1, "only the accepted row is removed");
        assert_eq!(view.queued_submissions[0].echo_id, "echo-other");
        let ids: Vec<_> = view.transcript.items().map(|item| item.id.as_str()).collect();
        assert_eq!(ids.iter().filter(|id| **id == "echo-steer").count(), 1);
        assert!(!ids.contains(&"echo-other"));
        assert!(view.steer_pending.is_empty());
        assert_eq!(view.queue_depth, 1);
    });
}

/// The service publishes the terminal result before the queue snapshot (see
/// `resolve_steer_receipt`). Steering a non-oldest row must therefore leave the
/// client mirror holding the two survivors with their authoritative ids: the
/// addressed row is removed by echo id first, so the snapshot's depth-based
/// front-truncation has nothing left to drop.
#[gpui_kit::test]
fn steering_a_middle_row_then_the_snapshot_keeps_the_other_rows(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        for (echo, entry, prompt) in [
            ("echo-older", "qe-1", "older"),
            ("echo-middle", "qe-2", "middle"),
            ("echo-newer", "qe-3", "newer"),
        ] {
            view.queued_submissions.push(QueuedSubmission {
                echo_id: echo.into(),
                entry_id: Some(entry.into()),
                prompt: prompt.into(),
                user_attachments: Vec::new(),
                acknowledged: true,
            });
        }
        let _ = view.steer_pending.insert("echo-middle".into(), "receipt-middle".into());

        // Result first: the addressed middle row is removed by echo id.
        view.handle_backend_event(
            BackendEvent::TurnSteerResult(serde_json::json!({
                "sessionId": "session",
                "turnId": "turn-1",
                "receiptId": "receipt-middle",
                "entryId": "qe-2",
                "accepted": true,
                "intentId": "intent-middle",
                "queueDepth": 2,
            })),
            cx,
        );
        // Snapshot second: depth 2 already matches the two remaining rows.
        view.handle_backend_event(
            BackendEvent::TurnQueueStatus(serde_json::json!({
                "sessionId": "session",
                "queueDepth": 2,
                "previews": ["older", "newer"],
                "entryIds": ["qe-1", "qe-3"],
            })),
            cx,
        );

        let prompts: Vec<&str> = view.queued_submissions.iter().map(|entry| entry.prompt.as_str()).collect();
        assert_eq!(prompts, vec!["older", "newer"], "the steered middle row leaves, both others stay");
        let ids: Vec<&str> = view
            .queued_submissions
            .iter()
            .map(|entry| entry.entry_id.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(ids, vec!["qe-1", "qe-3"], "ids are rebound to the authoritative snapshot");
        assert_eq!(view.queue_depth, 2);
        assert!(view.steer_pending.is_empty());
    });
}

#[gpui_kit::test]
fn steer_result_rejection_keeps_the_row_and_reports_reason(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("session".to_string());
        view.queued_submissions.push(QueuedSubmission {
            echo_id: "echo-steer".into(),
            entry_id: Some("qe-1".into()),
            prompt: "ship it".into(),
            user_attachments: Vec::new(),
            acknowledged: true,
        });
        let _ = view.steer_pending.insert("echo-steer".into(), "receipt-2".into());
        view.handle_backend_event(
            BackendEvent::TurnSteerResult(serde_json::json!({
                "sessionId": "session",
                "turnId": "turn-1",
                "receiptId": "receipt-2",
                "entryId": "qe-1",
                "accepted": false,
                "reason": "queueFull",
                "queueDepth": 1,
            })),
            cx,
        );
        assert_eq!(view.queued_submissions.len(), 1, "a rejected steer leaves the row available");
        assert!(view.steer_pending.is_empty());
        assert!(view.error.as_deref().is_some_and(|error| error.contains("QueueFull")));
    });
}
