use super::*;

#[tokio::test]
async fn session_install_delivers_a_queued_startup_interaction() {
    use vtcode_ui::tui::app::{ListOverlayRequest, TransientRequest};
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, mut notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let (commands, command_receiver) = mpsc::channel(2);
    commands
        .send(InlineCommand::ShowTransient {
            request: Box::new(TransientRequest::List(ListOverlayRequest {
                title: "Startup question".into(),
                lines: Vec::new(),
                footer_hint: None,
                items: Vec::new(),
                selected: None,
                search: None,
                hotkeys: Vec::new(),
                status: None,
            })),
        })
        .await
        .expect("queue startup question");
    let (_events, event_receiver) = protocol::live_event_channel(2);
    let (input, _input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    install_session_runtime(
        &context,
        RuntimeSession {
            session_id: "new-session".into(),
            input,
            stop: stop_requests::new_stop_handle_slot(),
            exit,
            abort: runtime_task.abort_handle(),
            command_forwarder: std::thread::spawn(|| {}),
        },
        command_receiver,
        event_receiver,
    )
    .await;
    let notification = timeout(Duration::from_secs(1), notifications.recv())
        .await
        .expect("pump timeout")
        .expect("notification");
    assert_eq!(notification.method, protocol::method::INTERACTION_REQUESTED);
    let interaction: InteractionRequest = serde_json::from_value(notification.params).expect("interaction");
    assert_eq!(interaction.session_id, "new-session");
    assert_eq!(interaction.turn_id, "session-start");
    assert_eq!(context.state.lock().await.pending_interactions.len(), 1);
    drop(commands);
    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn restored_queue_waits_for_the_startup_interaction_response() {
    use crate::agent::runloop::unified::state::CtrlCState;
    use vtcode_ui::tui::app::{ListOverlayRequest, TransientRequest};

    let workspace = hermetic_workspace();
    let (context, mut notifications) =
        test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
    context
        .state
        .lock()
        .await
        .queue
        .push_back(QueuedInput::new("restored prompt".into(), None));
    let (commands, command_receiver) = mpsc::channel(2);
    let (_events, event_receiver) = protocol::live_event_channel(1);
    let (input, mut input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    let stop = stop_requests::new_stop_handle_slot();
    install_session_runtime(
        &context,
        RuntimeSession {
            session_id: "session".into(),
            input,
            stop: stop.clone(),
            exit,
            abort: runtime_task.abort_handle(),
            command_forwarder: std::thread::spawn(|| {}),
        },
        command_receiver,
        event_receiver,
    )
    .await;
    commands
        .send(InlineCommand::ShowTransient {
            request: Box::new(TransientRequest::List(ListOverlayRequest {
                title: "Startup question".into(),
                lines: Vec::new(),
                footer_hint: None,
                items: Vec::new(),
                selected: None,
                search: None,
                hotkeys: Vec::new(),
                status: None,
            })),
        })
        .await
        .unwrap();
    let notification = timeout(Duration::from_secs(1), notifications.recv()).await.unwrap().unwrap();
    let interaction: InteractionRequest = serde_json::from_value(notification.params).unwrap();
    stop_requests::publish_stop_handles(
        &stop,
        &std::sync::Arc::new(CtrlCState::new()),
        &std::sync::Arc::new(Notify::new()),
    );
    stop_requests::mark_host_turns_ready(&stop);
    tokio::task::yield_now().await;
    assert!(input_receiver.try_recv().is_err(), "the question must settle before the queue dispatches");
    respond_to_interaction(
        &context,
        protocol::InteractionResponse {
            session_id: "session".into(),
            turn_id: interaction.turn_id,
            interaction_id: interaction.interaction_id,
            choice_id: None,
            choice_ids: Vec::new(),
            text: None,
            answer_texts: Vec::new(),
            cancelled: true,
        },
    )
    .await
    .expect("answer startup interaction");
    assert!(matches!(input_receiver.recv().await, Some(InlineEvent::Transient(_))));
    let event = timeout(Duration::from_secs(1), input_receiver.recv()).await.unwrap();
    assert!(matches!(event, Some(InlineEvent::Submit(input)) if input.text == "restored prompt"));
    drop(commands);
    drop(exit_sender);
    runtime_task.abort();
}
#[tokio::test]
async fn startup_interaction_blocks_turns_and_session_changes_until_answered() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, mut notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let adapter = DesktopWebmcpRuntime {
        workspace: FilesystemWorkspace::new(&context.workspace, [], false)
            .await
            .expect("workspace adapter"),
        state: std::sync::Arc::clone(&context.state),
        notifications: context.notifications.clone(),
    };
    let (input, mut input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    {
        let mut state = context.state.lock().await;
        state.initialized = true;
        state.session_id = Some("session".into());
        state.runtime = Some(RuntimeSession {
            session_id: "session".into(),
            input,
            stop: stop_requests::new_stop_handle_slot(),
            exit,
            abort: runtime_task.abort_handle(),
            command_forwarder: std::thread::spawn(|| {}),
        });
    }
    publish_interaction(
        "session",
        HostInteraction {
            kind: vtcode_ui::tui::app::HostInteractionKind::Question,
            wizard: false,
            title: "Startup question".into(),
            details: Vec::new(),
            steps: Vec::new(),
            selected_choice_ids: Vec::new(),
        },
        &context.state,
        &context.notifications,
    )
    .await
    .expect("publish question");
    let notification = notifications.recv().await.expect("question notification");
    let interaction: InteractionRequest = serde_json::from_value(notification.params).expect("question");
    assert_eq!(interaction.turn_id, "session-start");
    assert!(!webmcp_status(&context).await.turns_available);
    assert!(!adapter.status().await.expect("browser status").turns_available);
    let error = enqueue_turn(
        &context.state,
        &context.notifications,
        "session",
        "hello".into(),
        Vec::new(),
        EnqueueMode::Immediate,
    )
    .await
    .expect_err("pending question blocks turns");
    assert!(error.downcast_ref::<StartupInteractionPending>().is_some());
    let turn = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("turn".into()),
        method: protocol::method::TURN_SUBMIT.into(),
        params: json!({ "sessionId": "session", "prompt": "hello", "attachments": [] }),
    };
    let mut output = Vec::new();
    assert!(
        process_frame(&context, &mut output, &mut notifications, &serde_json::to_vec(&turn).unwrap())
            .await
            .unwrap()
    );
    let rejection: protocol::ErrorResponse = serde_json::from_slice(&output).expect("turn rejection");
    assert_eq!(rejection.error.code, protocol::STARTUP_INTERACTION_PENDING_ERROR);
    assert!(input_receiver.try_recv().is_err(), "rejected prompts must not reach the runtime");
    let close = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("close".into()),
        method: protocol::method::SESSION_CLOSE.into(),
        params: json!({ "sessionId": "session" }),
    };
    assert!(
        handle_request(&context, &close)
            .await
            .expect_err("pending question blocks close")
            .to_string()
            .contains("pending CLI interaction")
    );
    respond_to_interaction(
        &context,
        protocol::InteractionResponse {
            session_id: "session".into(),
            turn_id: interaction.turn_id,
            interaction_id: interaction.interaction_id,
            choice_id: None,
            choice_ids: Vec::new(),
            text: None,
            answer_texts: Vec::new(),
            cancelled: true,
        },
    )
    .await
    .expect("answer question");
    assert!(matches!(input_receiver.recv().await, Some(InlineEvent::Transient(_))));
    assert!(webmcp_status(&context).await.turns_available);
    assert!(adapter.status().await.expect("browser status").turns_available);
    enqueue_turn(
        &context.state,
        &context.notifications,
        "session",
        "hello".into(),
        Vec::new(),
        EnqueueMode::Immediate,
    )
    .await
    .expect("turn accepted after answer");
    assert!(matches!(input_receiver.recv().await, Some(InlineEvent::Submit(_))));
    drop(exit_sender);
    runtime_task.abort();
}
