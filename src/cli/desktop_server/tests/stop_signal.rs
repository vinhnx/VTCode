use super::super::dispatch::signal_runloop_stop_with_timeout;
use super::*;
use crate::agent::runloop::unified::state::CtrlCState;
use std::sync::Arc;

/// An active desktop session ("session"/"turn-1") whose runloop stop handles
/// the test controls directly.
struct StopHarness {
    context: ServiceContext,
    input: mpsc::UnboundedReceiver<InlineEvent>,
    notifications: mpsc::Receiver<protocol::Notification>,
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
        let (context, notifications) = test_service_context(startup, workspace);
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
            notifications,
            ctrl_c: Arc::new(CtrlCState::new()),
            notify: Arc::new(Notify::new()),
            stop,
            exit_sender: Some(exit_sender),
            _runtime_task: runtime_task,
        }
    }

    fn publish(&self) {
        stop_requests::publish_stop_handles(&self.stop, &self.ctrl_c, &self.notify);
        stop_requests::mark_host_turns_ready(&self.stop);
    }

    fn cancel_request(&self, session_id: &str, turn_id: &str) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(1),
            method: protocol::method::TURN_CANCEL.into(),
            params: json!({ "sessionId": session_id, "turnId": turn_id }),
        }
    }

    fn cancel_and_clear_request(&self, session_id: &str, turn_id: &str) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(2),
            method: protocol::method::TURN_CANCEL.into(),
            params: json!({ "sessionId": session_id, "turnId": turn_id, "clearQueue": true }),
        }
    }

    fn queue_depths(&mut self) -> Vec<usize> {
        let mut depths = Vec::new();
        while let Ok(notification) = self.notifications.try_recv() {
            if notification.method == protocol::method::TURN_QUEUE_STATUS
                && let Some(depth) = notification.params.get("queueDepth").and_then(Value::as_u64)
            {
                depths.push(depth as usize);
            }
        }
        depths
    }
}

async fn wait_for_cancel(ctrl_c: &Arc<CtrlCState>) {
    timeout(Duration::from_secs(5), async {
        while !ctrl_c.is_cancel_requested() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stop signal observed");
}

#[test]
#[serial_test::serial]
fn host_runloop_cancels_startup_turn_and_accepts_the_next_turn() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(16 * 1024 * 1024)
                .build()
                .expect("test runtime")
                .block_on(async {
                    let _store = IsolatedSessionStore::new();
                    let workspace = hermetic_workspace();
                    let (context, mut notifications) =
                        test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
                    context.state.lock().await.initialized = true;
                    start_session(&context, "host-cancel-test".into(), None)
                        .await
                        .expect("start real host runloop");
                    for prompt in ["first startup prompt", "second prompt"] {
                        let accepted = enqueue_turn(
                            &context.state,
                            &context.notifications,
                            "host-cancel-test",
                            prompt.into(),
                            Vec::new(),
                            EnqueueMode::Queueable { batchable: false },
                        )
                        .await
                        .expect("turn accepted");
                        let request = Request {
                            jsonrpc: "2.0".into(),
                            id: RequestId::Number(1),
                            method: protocol::method::TURN_CANCEL.into(),
                            params: json!({ "sessionId": "host-cancel-test", "turnId": accepted.turn_id }),
                        };
                        handle_request(&context, &request).await.expect("stop accepted");
                        timeout(Duration::from_secs(30), async {
                            let mut saw_terminal_event = false;
                            loop {
                                let notification = notifications.recv().await.expect("service remains connected");
                                if notification.method == protocol::method::THREAD_EVENT {
                                    let event: ThreadEventNotification =
                                        serde_json::from_value(notification.params.clone()).expect("thread event");
                                    if event.turn_id.as_deref() == Some(accepted.turn_id.as_str())
                                        && matches!(
                                            event.event.event(),
                                            vtcode_core::exec::events::ThreadEvent::TurnFailed(_)
                                        )
                                    {
                                        saw_terminal_event = true;
                                    }
                                }
                                if notification.method == protocol::method::TURN_STATUS
                                    && notification.params["turnId"] == accepted.turn_id
                                    && notification.params["status"] == "failed"
                                {
                                    assert!(saw_terminal_event, "the runloop emitted the terminal ThreadEvent");
                                    break;
                                }
                            }
                        })
                        .await
                        .expect("cancelled turn emits a terminal outcome");
                        assert!(context.state.lock().await.active_turn_id.is_none());
                    }
                    shutdown_current_session(&context, false).await.expect("shutdown");
                });
        })
        .expect("spawn test thread")
        .join()
        .expect("host runloop test");
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
        stop_requests::mark_host_turns_ready(&stop);
    });
    let request = harness.cancel_request("session", "turn-1");
    // The ack no longer blocks on the publish wait; the signal follows
    // from the detached waiter once the runloop publishes.
    let started = Instant::now();
    let (result, _) = timeout(Duration::from_secs(10), handle_request(&harness.context, &request))
        .await
        .expect("cancel must not hang")
        .expect("cancel accepted");
    assert!(started.elapsed() < Duration::from_secs(1), "the ack stays fast while handles publish late");
    assert_eq!(result["cancelRequested"], true);
    wait_for_cancel(&harness.ctrl_c).await;
    assert!(harness.ctrl_c.is_cancel_requested(), "a cancel during runloop startup must still stop the turn");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
}

#[tokio::test]
async fn startup_cancel_waits_past_old_deadline_until_turns_are_ready() {
    let mut harness = StopHarness::new().await;
    stop_requests::publish_stop_handles(&harness.stop, &harness.ctrl_c, &harness.notify);
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("cancel accepted");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert!(!harness.ctrl_c.is_cancel_requested(), "startup must not enter the pause state");
    stop_requests::mark_host_turns_ready(&harness.stop);
    wait_for_cancel(&harness.ctrl_c).await;
    assert!(harness.ctrl_c.is_cancel_requested(), "the accepted stop survives slow startup");
}

#[tokio::test]
async fn late_stop_handles_do_not_cancel_a_replacement_turn() {
    let mut harness = StopHarness::new().await;
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("cancel accepted");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    {
        let mut state = harness.context.state.lock().await;
        state.active_turn_id = Some("turn-2".into());
        state.cancel_requested_turn = None;
    }
    harness.publish();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!harness.ctrl_c.is_cancel_requested(), "the old stop must not reach the replacement turn");
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
    assert!(started.elapsed() < Duration::from_secs(1), "the ack never waits out the publish bound");
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
async fn duplicate_turn_cancel_refires_stop_signal() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("first cancel accepted");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    // The runloop observed the first stop but the turn is still active; a
    // repeated press must re-assert the signal instead of wedging silently.
    harness.ctrl_c.mark_cancel_handled();
    assert!(!harness.ctrl_c.is_cancel_requested());
    let (result, _) = handle_request(&harness.context, &request)
        .await
        .expect("duplicate cancel accepted");
    assert_eq!(result["cancelRequested"], true);
    assert!(harness.input.try_recv().is_err(), "a duplicate cancel must not re-queue");
    assert!(
        harness.ctrl_c.is_cancel_requested(),
        "a repeated stop must re-assert the stop signal while the turn is active"
    );
}

#[tokio::test]
async fn turn_cancel_refires_stop_signal_after_latch_reset() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("cancel accepted");
    assert!(harness.ctrl_c.is_cancel_requested(), "first signal fires synchronously");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    // Simulate the runloop resetting its latch while processing the Submit:
    // the first signal is lost, and the delayed re-fire must re-assert it
    // while the same turn is still active.
    harness.ctrl_c.reset();
    assert!(!harness.ctrl_c.is_cancel_requested());
    wait_for_cancel(&harness.ctrl_c).await;
    assert!(harness.input.try_recv().is_err(), "the re-fire sends no duplicate event");
}

#[tokio::test]
async fn cancel_refire_skips_released_turn() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("cancel accepted");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    // The turn ends before the re-fire delay elapses: terminal cleanup
    // releases the slot, so the late task must stay quiet.
    {
        let mut state = harness.context.state.lock().await;
        state.active_turn_id = None;
        state.cancel_requested_turn = None;
    }
    harness.ctrl_c.reset();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!harness.ctrl_c.is_cancel_requested(), "no re-fire after the turn is released");
    assert!(harness.input.try_recv().is_err());
}

#[tokio::test]
async fn cancel_refire_skips_replaced_turn() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("cancel accepted");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    // A drained batch takes the slot before the re-fire lands: the late
    // task targets turn-1 and must not stop turn-2.
    {
        let mut state = harness.context.state.lock().await;
        state.active_turn_id = Some("turn-2".into());
        state.cancel_requested_turn = None;
    }
    harness.ctrl_c.reset();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!harness.ctrl_c.is_cancel_requested(), "no re-fire into a newer turn");
    assert!(harness.input.try_recv().is_err());
}

#[tokio::test]
async fn flagged_cancel_discards_queued_turns_and_reports_empty_queue() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    for prompt in ["queued one", "queued two"] {
        let accepted = enqueue_turn(
            &harness.context.state,
            &harness.context.notifications,
            "session",
            prompt.into(),
            Vec::new(),
            EnqueueMode::Queueable { batchable: false },
        )
        .await
        .expect("queued submit accepted");
        assert!(accepted.queued);
    }
    assert_eq!(harness.queue_depths(), [1, 2]);
    let request = harness.cancel_and_clear_request("session", "turn-1");
    let (result, _) = handle_request(&harness.context, &request)
        .await
        .expect("flagged cancel accepted");
    assert_eq!(result["cancelRequested"], true);
    assert!(harness.ctrl_c.is_cancel_requested(), "the discard rides a real stop");
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::Cancel)));
    assert!(harness.context.state.lock().await.queue.is_empty(), "flagged cancel clears the queue");
    assert_eq!(harness.queue_depths(), [0], "clients reconcile from the empty snapshot");
    let mut state = harness.context.state.lock().await;
    state.active_turn_id = None;
    assert!(state.pop_next_queued_turn().is_none(), "no auto-dispatch follows the discard");
}

#[tokio::test]
async fn plain_cancel_preserves_queued_turns_for_auto_dispatch() {
    let mut harness = StopHarness::new().await;
    harness.publish();
    let accepted = enqueue_turn(
        &harness.context.state,
        &harness.context.notifications,
        "session",
        "queued prompt".into(),
        Vec::new(),
        EnqueueMode::Queueable { batchable: false },
    )
    .await
    .expect("queued submit accepted");
    assert!(accepted.queued);
    let request = harness.cancel_request("session", "turn-1");
    handle_request(&harness.context, &request).await.expect("plain cancel accepted");
    {
        let mut state = harness.context.state.lock().await;
        assert_eq!(state.queue.len(), 1, "a plain stop keeps the queue (TUI parity)");
        assert!(state.pop_next_queued_turn().is_none(), "the live turn still owns the slot");
        state.active_turn_id = None;
        let drained = state.pop_next_queued_turn().expect("queued batch dispatches after release");
        assert_eq!(state.active_turn_id.as_deref(), Some(drained.turn_id.as_str()));
    }
    assert_eq!(harness.queue_depths(), [1], "no discard snapshot on a plain stop");
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
async fn queued_submit_preserves_active_turn_cancellation_without_dispatching() {
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
    assert!(harness.ctrl_c.is_cancel_handled(), "a queued submission must not reset the active turn");
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
