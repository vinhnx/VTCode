use super::*;

#[test]
fn progress_scope_transfers_identity_and_only_clears_its_operation() {
    use vtcode_commons::ui_protocol::ProgressPhase;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let first = handle.begin_progress(ProgressPhase::PreparingContext);
    let operation = first.operation();
    first.transfer();
    let resumed = handle.resume_progress(ProgressPhase::SavingCheckpoint);
    assert_eq!(resumed.operation(), operation);
    let replacement = handle.begin_progress(ProgressPhase::Initializing);
    drop(resumed);
    assert_eq!(handle.current_progress_operation(), Some(replacement.operation()));
    drop(replacement);
    assert_eq!(handle.current_progress_operation(), None);
    let updates: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(updates.len(), 5);
}

#[tokio::test]
async fn cancellation_drops_progress_scope_and_emits_matching_finish() {
    use vtcode_commons::ui_protocol::{ProgressPhase, ProgressUpdate};
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let worker_handle = handle.clone();
    let worker = tokio::spawn(async move {
        let _guard = worker_handle.begin_progress(ProgressPhase::WaitingForModel);
        std::future::pending::<()>().await;
    });
    let operation = match rx.recv().await.unwrap() {
        InlineCommand::UpdateProgress(ProgressUpdate::Begin { operation, .. }) => operation,
        _ => panic!("expected progress begin"),
    };
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert!(
        matches!(rx.recv().await, Some(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: finished })) if finished == operation)
    );
    assert!(handle.current_progress_operation().is_none());
}

#[test]
fn evidence_navigation_records_capture_before_focus_and_reports_closed_ui() {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    assert!(handle.review_evidence(vec!["recorded event".into()]));
    let InlineCommand::RecordToolOutput { id, lines } = receiver.try_recv().unwrap() else {
        panic!("capture must precede focus")
    };
    assert_eq!(lines, ["recorded event"]);
    assert!(
        matches!(receiver.try_recv().unwrap(), InlineCommand::FocusTranscriptReview { id: focused } if focused == id)
    );
    assert!(receiver.try_recv().is_err(), "navigation must not submit input");
    drop(receiver);
    assert!(!handle.review_evidence(vec!["expired receiver".into()]));
}

fn test_session() -> (InlineHandle, InlineSession, UnboundedSender<InlineEvent>) {
    let (command_sender, _command_receiver) = tokio::sync::mpsc::unbounded_channel();
    let (event_sender, event_receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_sender);
    let session = InlineSession {
        handle: handle.clone(),
        events: event_receiver,
        worker: None,
    };
    (handle, session, event_sender)
}

#[tokio::test]
async fn deferred_event_waits_until_transient_closes() {
    let (handle, mut session, event_sender) = test_session();

    handle
        .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
        .expect("defer bridge event");
    handle.show_list_modal("test".into(), Vec::new(), Vec::new(), None, None);
    event_sender.send(InlineEvent::Cancel).expect("send modal result");

    let modal_event = session.next_event().await;
    assert!(matches!(modal_event, Some(InlineEvent::Cancel)));

    handle.close_transient();
    let deferred_event = session.next_event().await;
    assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
}

#[tokio::test]
async fn transient_close_wakes_waiting_deferred_event_consumer() {
    let (handle, session, _event_sender) = test_session();

    handle
        .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
        .expect("defer bridge event");
    handle.show_local_agents();

    let waiter = tokio::spawn(async move {
        let mut session = session;
        session.next_event().await
    });
    tokio::task::yield_now().await;

    // A captured-input surface can close without sending a user event.
    handle.transient_activity.set_active(false);

    let deferred_event = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
        .await
        .expect("deferred consumer should wake after transient close")
        .expect("deferred consumer task should finish");
    assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
}

#[tokio::test]
async fn closed_event_stream_waits_for_transient_close_before_deferred_event() {
    let (handle, session, event_sender) = test_session();

    handle
        .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
        .expect("defer bridge event");
    handle.show_local_agents();
    drop(event_sender);

    let mut waiter = tokio::spawn(async move {
        let mut session = session;
        session.next_event().await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut waiter)
            .await
            .is_err(),
        "closed UI channel must not bypass an active captured-input surface"
    );

    handle.transient_activity.set_active(false);
    let deferred_event = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
        .await
        .expect("deferred consumer should wake after transient close")
        .expect("deferred consumer task should finish");
    assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
}

#[tokio::test]
async fn palette_preload_does_not_hold_deferred_input() {
    let (handle, mut session, _event_sender) = test_session();

    handle
        .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
        .expect("defer bridge event");
    handle.configure_agent_palette(Vec::new());

    let deferred_event = session.next_event().await;
    assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
}

#[tokio::test]
async fn passive_task_panel_updates_do_not_hold_deferred_input() {
    let (handle, mut session, _event_sender) = test_session();

    handle
        .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
        .expect("defer bridge event");
    handle.show_task_panel();
    handle.update_task_panel(Vec::new());

    let deferred_event = session.next_event().await;
    assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
}

#[tokio::test]
async fn captured_local_agents_drawer_releases_deferred_input_when_hidden() {
    let (handle, mut session, event_sender) = test_session();

    handle
        .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
        .expect("defer bridge event");
    handle.show_local_agents();
    event_sender.send(InlineEvent::Cancel).expect("send drawer event");

    let drawer_event = session.next_event().await;
    assert!(matches!(drawer_event, Some(InlineEvent::Cancel)));

    handle.hide_local_agents();
    let deferred_event = session.next_event().await;
    assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
}

#[tokio::test]
async fn deferred_queue_overflow_fails_closed_without_dropping() {
    let (handle, _session, _event_sender) = test_session();

    for index in 0..MAX_DEFERRED_EVENTS {
        handle
            .defer_event(InlineEvent::WebmcpSubmit(format!("deferred-{index}").into()))
            .expect("queue should accept up to the bound");
    }
    assert!(handle.has_deferred_event());

    let overflow = handle.defer_event(InlineEvent::WebmcpSubmit("overflow".into()));
    assert!(overflow.is_err(), "32-event bound must fail closed, not silently drop");
    assert!(handle.has_deferred_event(), "overflow must not drain retained events");
}
