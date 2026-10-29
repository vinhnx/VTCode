use super::*;

#[tokio::test]
async fn shutdown_drains_full_notifications_while_waiting_for_turn_cleanup() {
    let workspace = tempfile::tempdir().expect("workspace");
    let startup = test_startup_context(workspace.path());
    let (mut context, _) = test_service_context(startup, workspace.path().to_path_buf());
    let (notifications, mut receiver) = mpsc::channel(1);
    context.notifications = notifications.clone();
    notifications
        .try_send(protocol::Notification {
            jsonrpc: "2.0".into(),
            method: "first".into(),
            params: json!({}),
        })
        .unwrap();
    let (input, mut input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let state = std::sync::Arc::clone(&context.state);
    let finished = std::sync::Arc::clone(&context.turn_finished);
    let runtime_task = tokio::spawn(async move {
        assert!(matches!(input_receiver.recv().await, Some(InlineEvent::Cancel)));
        notifications
            .send(protocol::Notification {
                jsonrpc: "2.0".into(),
                method: "second".into(),
                params: json!({}),
            })
            .await
            .unwrap();
        state.lock().await.active_turn_id = None;
        finished.notify_waiters();
        assert!(matches!(input_receiver.recv().await, Some(InlineEvent::Exit)));
        let _ = exit_sender.send(Ok(()));
    });
    {
        let mut state = context.state.lock().await;
        state.session_id = Some("session".into());
        state.active_turn_id = Some("turn".into());
        state.runtime = Some(RuntimeSession {
            session_id: "session".into(),
            input,
            exit,
            abort: runtime_task.abort_handle(),
            command_forwarder: std::thread::spawn(|| {}),
        });
    }
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(1),
        method: protocol::method::SHUTDOWN.into(),
        params: json!({}),
    };
    let mut output = Vec::new();
    let keep_running = timeout(
        Duration::from_secs(2),
        process_frame(&context, &mut output, &mut receiver, &serde_json::to_vec(&request).unwrap()),
    )
    .await
    .expect("shutdown must drain notifications")
    .expect("shutdown response");
    assert!(!keep_running);
    let frames: Vec<Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(frames[0]["method"], "first");
    assert_eq!(frames[1]["method"], "second");
    assert_eq!(frames.last().unwrap()["result"]["shutdown"], true);
    runtime_task.await.unwrap();
}
#[tokio::test]
async fn accepted_turn_is_not_rejected_or_blocked_by_status_queue() {
    for closed in [false, true] {
        let (input, mut input_receiver) = mpsc::unbounded_channel();
        let (notifications, receiver) = mpsc::channel(1);
        notifications
            .try_send(protocol::Notification {
                jsonrpc: "2.0".into(),
                method: "existing".into(),
                params: json!({}),
            })
            .unwrap();
        let _receiver = if closed {
            drop(receiver);
            None
        } else {
            Some(receiver)
        };
        let (exit_sender, exit) = oneshot::channel();
        let runtime_task = tokio::spawn(std::future::pending::<()>());
        let state = Mutex::new(ServerState {
            session_id: Some("session".into()),
            runtime: Some(RuntimeSession {
                session_id: "session".into(),
                input,
                exit,
                abort: runtime_task.abort_handle(),
                command_forwarder: std::thread::spawn(|| {}),
            }),
            ..ServerState::default()
        });
        let accepted = timeout(
            Duration::from_secs(1),
            enqueue_turn(&state, &notifications, "session", "only once".into(), Vec::new()),
        )
        .await
        .expect("a full notification queue must not block the request")
        .expect("delivered turn is accepted");
        assert_eq!(accepted.session_id, "session");
        let Some(InlineEvent::Submit(submission)) = input_receiver.recv().await else {
            panic!("delivered prompt")
        };
        assert_eq!(submission.text, "only once");
        assert!(input_receiver.try_recv().is_err());
        assert_eq!(state.lock().await.active_turn_id.as_deref(), Some(accepted.turn_id.as_str()));
        drop(exit_sender);
        runtime_task.abort();
    }
}
#[tokio::test]
async fn webmcp_turns_use_the_selected_desktop_runtime_and_respect_busy_state() {
    let workspace_root = tempfile::tempdir().expect("workspace");
    let workspace = FilesystemWorkspace::new(workspace_root.path(), [], false)
        .await
        .expect("workspace adapter");
    let (input, mut input_receiver) = mpsc::unbounded_channel();
    let (notifications, mut notification_receiver) = mpsc::channel(2);
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    let abort = runtime_task.abort_handle();
    let command_forwarder = std::thread::spawn(|| {});
    let server_state = ServerState {
        session_id: Some("desktop-session".into()),
        runtime: Some(RuntimeSession {
            session_id: "desktop-session".into(),
            input,
            exit,
            abort,
            command_forwarder,
        }),
        ..ServerState::default()
    };
    let state = std::sync::Arc::new(Mutex::new(server_state));
    let adapter = DesktopWebmcpRuntime {
        workspace,
        state: std::sync::Arc::clone(&state),
        notifications,
    };

    assert!(adapter.status().await.expect("status").turns_available);
    let accepted = adapter
        .request_turn("  inspect the workspace  ", None)
        .await
        .expect("accepted turn");
    assert!(accepted.accepted);
    let Some(InlineEvent::Submit(submission)) = input_receiver.recv().await else {
        panic!("WebMCP must enqueue through the desktop session input");
    };
    assert_eq!(submission.text, "inspect the workspace");
    assert_eq!(notification_receiver.recv().await.expect("turn status").method, protocol::method::TURN_STATUS);
    assert!(!adapter.status().await.expect("busy status").turns_available);
    assert!(adapter.request_turn("a second turn", None).await.is_err());
    assert!(input_receiver.try_recv().is_err(), "a busy runtime must not receive another turn");

    drop(exit_sender);
    runtime_task.abort();
}
#[tokio::test]
async fn stopped_webmcp_listener_is_reported_and_can_be_repaired() {
    let workspace = tempfile::tempdir().expect("workspace");
    let mut startup = test_startup_context(workspace.path());
    startup.config.webmcp.host = "127.0.0.1".into();
    startup.config.webmcp.port = 0;
    startup.config.webmcp.allowed_origins = vec!["https://example.test".into()];
    let (context, _notifications) = test_service_context(startup, workspace.path().to_path_buf());
    let server = test_webmcp_server(&context).await;
    let listener = server.bind().await.expect("initial listener");
    let address = listener.local_addr().expect("listener address");
    let task_server = server.clone();
    let task = tokio::spawn(async move {
        let _ = task_server.serve_listener(listener).await;
    });
    context.state.lock().await.webmcp_event_hub = Some(server.event_hub());
    *context.webmcp.lock().await = Some(DesktopWebmcpBridge {
        server,
        task,
        endpoint: format!("ws://{address}/webmcp"),
    });

    {
        let mut slot = context.webmcp.lock().await;
        let bridge = slot.as_mut().expect("active bridge");
        bridge.task.abort();
        let _ = (&mut bridge.task).await;
    }

    let stopped = webmcp_status(&context).await;
    assert!(!stopped.running, "a finished listener must not be reported as running");
    assert!(stopped.endpoint.is_none());
    assert!(context.state.lock().await.webmcp_event_hub.is_none());

    let pairing = pair_webmcp(
        &context,
        protocol::WebmcpPairParams {
            origin: "https://example.test".into(),
            replace: false,
        },
    )
    .await
    .expect("pairing restarts a stopped listener");
    let connected = webmcp_status(&context).await;
    assert!(connected.running);
    assert_eq!(connected.endpoint.as_deref(), Some(pairing.endpoint.as_str()));

    stop_webmcp_bridge(&context).await;
    let stopped_address = pairing
        .endpoint
        .strip_prefix("ws://")
        .expect("local WebSocket endpoint")
        .strip_suffix("/webmcp")
        .expect("WebMCP endpoint path");
    let _rebound = tokio::net::TcpListener::bind(stopped_address)
        .await
        .expect("stop waits until the listener releases its port");
}
