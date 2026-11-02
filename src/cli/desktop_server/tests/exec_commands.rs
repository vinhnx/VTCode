use super::*;

struct ExecHarness {
    context: ServiceContext,
    input: mpsc::UnboundedReceiver<InlineEvent>,
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
        {
            let mut state = context.state.lock().await;
            state.initialized = true;
            state.session_id = Some("session".into());
            state.runtime = Some(RuntimeSession {
                session_id: "session".into(),
                input: input_sender,
                stop: stop_requests::new_stop_handle_slot(),
                exit,
                abort: runtime_task.abort_handle(),
                command_forwarder: std::thread::spawn(|| {}),
            });
        }
        Self { context, input, _runtime_task: runtime_task }
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
