use super::*;
use crate::agent::runloop::unified::state::CtrlCState;
use std::sync::Arc;

struct ExecHarness {
    context: ServiceContext,
    input: mpsc::UnboundedReceiver<InlineEvent>,
    stop: StopHandleSlot,
    _runtime_task: tokio::task::JoinHandle<()>,
}

impl ExecHarness {
    async fn new() -> Self {
        let workspace = std::env::temp_dir().join("vtcode-exec-commands-test");
        let startup = test_startup_context(&workspace);
        let (context, _notifications) = test_service_context(startup, workspace);
        let (input_sender, input) = mpsc::unbounded_channel();
        let (_exit_sender, exit) = oneshot::channel();
        let runtime_task = tokio::spawn(std::future::pending::<()>());
        let stop = stop_requests::new_stop_handle_slot();
        {
            let mut state = context.state.lock().await;
            state.initialized = true;
            state.session_id = Some("session".into());
            state.runtime = Some(RuntimeSession {
                session_id: "session".into(),
                input: input_sender,
                stop: Arc::clone(&stop),
                exit,
                abort: runtime_task.abort_handle(),
                command_forwarder: std::thread::spawn(|| {}),
            });
        }
        Self { context, input, stop, _runtime_task: runtime_task }
    }

    /// Publish stop handles and mark a turn active, mirroring a runloop that
    /// owns a running turn. Returns the published stop state for assertions.
    async fn publish_active_turn(&mut self) -> Arc<CtrlCState> {
        let ctrl_c = Arc::new(CtrlCState::new());
        let notify = Arc::new(Notify::new());
        stop_requests::publish_stop_handles(&self.stop, &ctrl_c, &notify);
        stop_requests::mark_host_turns_ready(&self.stop);
        let mut state = self.context.state.lock().await;
        state.active_turn_id = Some("turn-1".into());
        ctrl_c
    }

    fn request(&self, method: &str, session_id: &str) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(1),
            method: method.into(),
            params: json!({ "sessionId": session_id }),
        }
    }
}

#[tokio::test]
async fn force_cancel_accepted_and_queued() {
    let mut harness = ExecHarness::new().await;
    let request = harness.request(protocol::method::EXEC_FORCE_CANCEL, "session");
    let (result, shutdown) = handle_request(&harness.context, &request).await.expect("accepted");
    assert!(!shutdown);
    assert_eq!(result["accepted"], true);
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::ForceCancelPtySession)));
}

/// A force stop for a mid-turn long-running process must also fire the
/// forced stop signal: the queued event alone is only consumed between
/// turns, so the runloop's cancel paths are what kill the process now.
#[tokio::test]
async fn force_cancel_with_active_turn_fires_forced_stop_signal() {
    let mut harness = ExecHarness::new().await;
    let ctrl_c = harness.publish_active_turn().await;
    let request = harness.request(protocol::method::EXEC_FORCE_CANCEL, "session");
    let (result, shutdown) = handle_request(&harness.context, &request).await.expect("accepted");
    assert!(!shutdown);
    assert_eq!(result["accepted"], true);
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::ForceCancelPtySession)));
    assert!(ctrl_c.is_cancel_requested(), "the forced fire must stop the running turn");
    assert!(ctrl_c.is_force_kill(), "the forced fire must widen the session teardown to every session");
}

/// An unhandled stop signal belongs to the next submitted turn, so the
/// idle escape hatch must not fire one: it only queues the kill event the
/// parked driver consumes immediately.
#[tokio::test]
async fn force_cancel_without_active_turn_keeps_stop_signal_clear() {
    let mut harness = ExecHarness::new().await;
    let ctrl_c = Arc::new(CtrlCState::new());
    let notify = Arc::new(Notify::new());
    stop_requests::publish_stop_handles(&harness.stop, &ctrl_c, &notify);
    stop_requests::mark_host_turns_ready(&harness.stop);
    let request = harness.request(protocol::method::EXEC_FORCE_CANCEL, "session");
    let (result, shutdown) = handle_request(&harness.context, &request).await.expect("accepted");
    assert!(!shutdown);
    assert_eq!(result["accepted"], true);
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::ForceCancelPtySession)));
    assert!(!ctrl_c.is_cancel_requested(), "no active turn must not consume the next one");
    assert!(!ctrl_c.is_force_kill());
}

#[tokio::test]
async fn background_accepted_and_queued() {
    let mut harness = ExecHarness::new().await;
    let request = harness.request(protocol::method::EXEC_BACKGROUND, "session");
    let (result, shutdown) = handle_request(&harness.context, &request).await.expect("accepted");
    assert!(!shutdown);
    assert_eq!(result["accepted"], true);
    assert!(matches!(harness.input.recv().await, Some(InlineEvent::BackgroundOperation)));
}

#[tokio::test]
async fn exec_commands_reject_unselected_session() {
    let mut harness = ExecHarness::new().await;
    for method in [protocol::method::EXEC_FORCE_CANCEL, protocol::method::EXEC_BACKGROUND] {
        let request = harness.request(method, "other-session");
        assert!(handle_request(&harness.context, &request).await.is_err(), "{method} rejected");
    }
    assert!(harness.input.try_recv().is_err(), "rejected commands queue nothing");
}

#[tokio::test]
async fn exec_commands_on_dead_runtime_bail() {
    let mut harness = ExecHarness::new().await;
    harness.input.close();
    let request = harness.request(protocol::method::EXEC_FORCE_CANCEL, "session");
    handle_request(&harness.context, &request)
        .await
        .expect_err("dead runtime bails");
}
