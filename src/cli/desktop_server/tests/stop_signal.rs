use super::super::dispatch::signal_runloop_stop_with_timeout;
use super::*;
use crate::agent::runloop::unified::state::CtrlCState;
use std::sync::Arc;

/// An active desktop session ("session"/"turn-1") whose runloop stop handles
/// the test controls directly.
struct StopHarness {
    context: ServiceContext,
    input: mpsc::UnboundedReceiver<InlineEvent>,
    ctrl_c: Arc<CtrlCState>,
    notify: Arc<Notify>,
    stop: StopHandleSlot,
    exit_sender: Option<oneshot::Sender<std::result::Result<(), String>>>,
    _runtime_task: tokio::task::JoinHandle<()>,
}

impl StopHarness {
    async fn new() -> Self {
        // The harness path is stored, never resolved: these tests never touch
        // the filesystem, so no tempdir guard is needed.
        let workspace = std::env::temp_dir().join("vtcode-stop-signal-test");
        let startup = test_startup_context(&workspace);
        let (context, _) = test_service_context(startup, workspace);
        let (input_sender, input) = mpsc::unbounded_channel();
        let (exit_sender, exit) = oneshot::channel();
        let runtime_task = tokio::spawn(std::future::pending::<()>());
        let stop = stop_requests::new_stop_handle_slot();
        {
            let mut state = context.state.lock().await;
            state.initialized = true;
            state.session_id = Some("session".into());
            state.active_turn_id = Some("turn-1".into());
            state.runtime = Some(RuntimeSession {
                session_id: "session".into(),
                input: input_sender,
                stop: stop.clone(),
                exit,
                abort: runtime_task.abort_handle(),
                command_forwarder: std::thread::spawn(|| {}),
            });
        }
        Self {
            context,
            input,
            ctrl_c: Arc::new(CtrlCState::new()),
            notify: Arc::new(Notify::new()),
            stop,
            exit_sender: Some(exit_sender),
            _runtime_task: runtime_task,
        }
    }

    fn publish(&self) {
        stop_requests::publish_stop_handles(&self.stop, &self.ctrl_c, &self.notify);
    }

    fn cancel_request(&self, session_id: &str, turn_id: &str) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(1),
            method: protocol::method::TURN_CANCEL.into(),
            params: json!({ "sessionId": session_id, "turnId": turn_id }),
        }
    }
}

#[tokio::test]
async fn turn_cancel_fires_stop_signal_and_queues_cancel_event() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let request = harness.cancel_request("session", "turn-1");
    let (result, shutdown) = timeout(Duration::from_secs(5), handle_request(&harness.context, &request))
        .await
        .expect("cancel must not hang")
        .expect("cancel accepted");
    assert!(!shutdown);
    assert_eq!(result["cancelRequested"], true);
    assert!(harness.ctrl_c.is_cancel_requested(), "the stop button must reach the running turn out-of-band");
    assert!(
        matches!(harness.input.recv().await, Some(InlineEvent::Cancel)),
        "the queued event still dismisses overlay waits"
    );
    assert_eq!(harness.context.state.lock().await.cancel_requested_turn.as_deref(), Some("turn-1"));
}

#[tokio::test]
async fn turn_cancel_waits_for_late_stop_handles() {
    let mut harness = StopHarness::new().await;
    let stop = harness.stop.clone();
    let ctrl_c = harness.ctrl_c.clone();
    let notify = harness.notify.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        stop_requests::publish_stop_handles(&stop, &ctrl_c, &notify);
    });
    let request = harness.cancel_request("session", "turn-1");
    let (result, _) = timeout(Duration::from_secs(10), handle_request(&harness.context, &request))
        .await
        .expect("cancel must not hang")
        .expect("cancel accepted");
    assert_eq!(result["cancelRequested"], true);
    assert!(harness.ctrl_c.is_cancel_requested(), "a cancel during runloop startup must still stop the turn");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
}

#[tokio::test]
async fn turn_cancel_without_handles_still_queues_cancel_event() {
    let mut harness = StopHarness::new().await;
    // The runloop never publishes: the request falls back to the queued
    // event after a bounded wait instead of hanging or failing.
    let request = harness.cancel_request("session", "turn-1");
    let started = Instant::now();
    let (result, _) = timeout(Duration::from_secs(10), handle_request(&harness.context, &request))
        .await
        .expect("cancel must stay bounded without handles")
        .expect("cancel accepted");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert_eq!(result["cancelRequested"], true);
    assert!(!harness.ctrl_c.is_cancel_requested());
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
}

#[tokio::test]
async fn turn_cancel_on_dead_runtime_bails_without_waiting() {
    let mut harness = StopHarness::new().await;
    // Never published, and the closed channel proves the runtime is gone.
    harness.input.close();
    let request = harness.cancel_request("session", "turn-1");
    let started = Instant::now();
    handle_request(&harness.context, &request)
        .await
        .expect_err("dead runtime bails");
    assert!(started.elapsed() < Duration::from_secs(2), "a dead runtime must skip the publish wait");
    assert!(harness.context.state.lock().await.cancel_requested_turn.is_none());
}

#[tokio::test]
async fn stop_signal_helper_returns_quickly_without_handles() {
    let harness = StopHarness::new().await;
    let started = Instant::now();
    signal_runloop_stop_with_timeout(&harness.stop, Duration::from_millis(20)).await;
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!stop_requests::fire_stop_signal(&harness.stop));
}

#[tokio::test]
async fn duplicate_turn_cancel_sends_nothing_more() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("first cancel accepted");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    let (result, _) = handle_request(&harness.context, &request)
        .await
        .expect("duplicate cancel accepted");
    assert_eq!(result["cancelRequested"], true);
    assert!(harness.input.try_recv().is_err(), "a duplicate cancel must not re-queue");
}

#[tokio::test]
async fn stale_turn_cancel_fires_no_stop_signal() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    for (session_id, turn_id) in [("session", "turn-2"), ("other-session", "turn-1")] {
        let request = harness.cancel_request(session_id, turn_id);
        assert!(handle_request(&harness.context, &request).await.is_err(), "stale cancel rejected");
    }
    assert!(!harness.ctrl_c.is_cancel_requested(), "a stale cancel must not stop the live turn");
    assert!(harness.input.try_recv().is_err(), "a stale cancel must not queue");
    assert!(harness.context.state.lock().await.cancel_requested_turn.is_none());
}

#[tokio::test]
async fn immediate_submit_clears_handled_cancellation() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    harness.context.state.lock().await.active_turn_id = None;
    harness.ctrl_c.request_local_cancel();
    harness.ctrl_c.mark_cancel_handled();
    let accepted = enqueue_turn(
        &harness.context.state,
        &harness.context.notifications,
        "session",
        "fresh prompt".into(),
        Vec::new(),
        EnqueueMode::Queueable { batchable: false },
    )
    .await
    .expect("submit accepted");
    assert!(!accepted.queued);
    assert!(!harness.ctrl_c.is_cancel_requested());
    assert!(!harness.ctrl_c.is_cancel_handled(), "a fresh submission resumes a stop-paused initialization");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Submit(_))));
}

#[tokio::test]
async fn queued_submit_clears_handled_cancellation_without_dispatching() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    harness.ctrl_c.request_local_cancel();
    harness.ctrl_c.mark_cancel_handled();
    let accepted = enqueue_turn(
        &harness.context.state,
        &harness.context.notifications,
        "session",
        "queued prompt".into(),
        Vec::new(),
        EnqueueMode::Queueable { batchable: true },
    )
    .await
    .expect("queued submit accepted");
    assert!(accepted.queued);
    assert_eq!(accepted.queue_depth, 1);
    assert!(!harness.ctrl_c.is_cancel_handled(), "a queued submission also resumes a stop-paused initialization");
    assert!(harness.input.try_recv().is_err(), "a queued prompt dispatches only after the active turn");
}

#[tokio::test]
async fn shutdown_requests_exit_through_stop_handles() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    harness.context.state.lock().await.active_turn_id = None;
    let exit_sender = harness.exit_sender.take().expect("exit channel");
    let StopHarness { context, mut input, ctrl_c, .. } = harness;
    let responder = tokio::spawn(async move {
        assert!(matches!(input.recv().await, Some(InlineEvent::Exit)));
        let _ = exit_sender.send(Ok(()));
    });
    timeout(Duration::from_secs(10), shutdown_current_session(&context, false))
        .await
        .expect("shutdown must not hang")
        .expect("shutdown");
    assert!(ctrl_c.is_exit_requested(), "shutdown must reach even a runloop paused in initialization");
    assert!(context.state.lock().await.session_id.is_none());
    responder.await.expect("responder");
}

#[tokio::test]
async fn shutdown_fires_stop_signal_before_waiting_for_turn_cleanup() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let exit_sender = harness.exit_sender.take().expect("exit channel");
    let StopHarness { context, mut input, ctrl_c, .. } = harness;
    let state = Arc::clone(&context.state);
    let finished = Arc::clone(&context.turn_finished);
    let stopped = ctrl_c.clone();
    let responder = tokio::spawn(async move {
        assert!(matches!(input.recv().await, Some(InlineEvent::Cancel)));
        assert!(stopped.is_cancel_requested(), "shutdown must stop the active turn out-of-band");
        state.lock().await.active_turn_id = None;
        finished.notify_waiters();
        assert!(matches!(input.recv().await, Some(InlineEvent::Exit)));
        let _ = exit_sender.send(Ok(()));
    });
    timeout(Duration::from_secs(10), shutdown_current_session(&context, true))
        .await
        .expect("shutdown must not hang")
        .expect("shutdown");
    assert!(ctrl_c.is_exit_requested(), "shutdown must end the runloop through its stop handles");
    responder.await.expect("responder");
}
