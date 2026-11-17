use super::*;
use vtcode_core::core::agent::steering::SteerOutcome;

#[tokio::test]
async fn steer_rejection_keeps_the_addressed_row_available_for_dispatch() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, mut notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let (input, _input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    let (steer_tx, mut steer_rx) = mpsc::unbounded_channel();
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
    let queueable = EnqueueMode::Queueable { batchable: false };
    enqueue_turn(&context.state, &context.notifications, "session", "running".into(), Vec::new(), queueable)
        .await
        .expect("idle submit");
    enqueue_turn(&context.state, &context.notifications, "session", "waiting".into(), Vec::new(), queueable)
        .await
        .expect("queue behind turn");
    for _ in 0..2 {
        notifications.recv().await.expect("drain setup notifications");
    }
    let entry_id = {
        let mut state = context.state.lock().await;
        state.active_turn_id = Some("turn-1".into());
        let stop = &state.runtime.as_ref().expect("runtime").stop;
        stop_requests::publish_steering_sender(stop, &steer_tx);
        state.queue.front().expect("queued entry").entry_id()
    };

    let steer = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(1),
        method: protocol::method::TURN_STEER.into(),
        params: serde_json::json!({ "sessionId": "session", "text": "waiting", "entryId": entry_id }),
    };
    let (result, _) = dispatch::handle_request_inner(&context, &steer).await.expect("steer");
    let steered: protocol::TurnSteerResult = serde_json::from_value(result).expect("steer result");
    assert!(steered.steered && steered.pending);
    let message = steer_rx.try_recv().expect("steering message");
    let vtcode_core::core::agent::steering::SteeringMessage::FollowUpInput(steer_msg) = message else {
        panic!("expected follow-up steer");
    };
    steer_msg.reject(vtcode_core::core::agent::steering::SteerRejection::QueueFull);

    let first = notifications.recv().await.expect("steer result notification");
    assert_eq!(first.method, protocol::method::TURN_STEER_RESULT);
    let outcome: protocol::TurnSteerResultNotification =
        serde_json::from_value(first.params).expect("steer result DTO");
    assert!(!outcome.accepted);
    assert_eq!(outcome.reason, Some(protocol::TurnSteerRejection::QueueFull));
    assert_eq!(outcome.receipt_id, steered.receipt_id);
    let state = context.state.lock().await;
    assert_eq!(state.queue.len(), 1, "a rejected steer leaves the row for normal dispatch");
    assert_eq!(state.queue.front().expect("queued entry").entry_id(), entry_id);
    drop(state);

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn steer_ack_after_turn_replacement_keeps_the_row_queued() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, mut notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let (input, _input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    let (steer_tx, mut steer_rx) = mpsc::unbounded_channel();
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
    let queueable = EnqueueMode::Queueable { batchable: false };
    enqueue_turn(&context.state, &context.notifications, "session", "running".into(), Vec::new(), queueable)
        .await
        .expect("idle submit");
    enqueue_turn(&context.state, &context.notifications, "session", "waiting".into(), Vec::new(), queueable)
        .await
        .expect("queue behind turn");
    for _ in 0..2 {
        notifications.recv().await.expect("drain setup notifications");
    }
    let entry_id = {
        let mut state = context.state.lock().await;
        state.active_turn_id = Some("turn-1".into());
        let stop = &state.runtime.as_ref().expect("runtime").stop;
        stop_requests::publish_steering_sender(stop, &steer_tx);
        state.queue.front().expect("queued entry").entry_id()
    };

    let steer = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(1),
        method: protocol::method::TURN_STEER.into(),
        params: serde_json::json!({ "sessionId": "session", "text": "waiting", "entryId": entry_id }),
    };
    let (result, _) = dispatch::handle_request_inner(&context, &steer).await.expect("steer");
    let steered: protocol::TurnSteerResult = serde_json::from_value(result).expect("steer result");
    let message = steer_rx.try_recv().expect("steering message");
    let vtcode_core::core::agent::steering::SteeringMessage::FollowUpInput(steer_msg) = message else {
        panic!("expected follow-up steer");
    };

    context.state.lock().await.active_turn_id = Some("turn-2".into());
    steer_msg.resolve(SteerOutcome::Accepted { intent_id: "stale-intent".into() });

    let notification = notifications.recv().await.expect("steer result notification");
    assert_eq!(notification.method, protocol::method::TURN_STEER_RESULT);
    let outcome: protocol::TurnSteerResultNotification =
        serde_json::from_value(notification.params).expect("steer result DTO");
    assert!(!outcome.accepted, "a stale acceptance must not remove the row");
    assert_eq!(outcome.reason, Some(protocol::TurnSteerRejection::Superseded));
    assert_eq!(steered.receipt_id, outcome.receipt_id);
    let state = context.state.lock().await;
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue.front().expect("queued entry").entry_id(), entry_id);
    drop(state);

    drop(exit_sender);
    runtime_task.abort();
}
