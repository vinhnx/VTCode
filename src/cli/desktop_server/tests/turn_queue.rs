use super::*;

type RuntimeStateFixture = (
    Mutex<ServerState>,
    mpsc::UnboundedReceiver<InlineEvent>,
    tokio::task::JoinHandle<()>,
    oneshot::Sender<Result<(), String>>,
);

fn runtime_state(session: &str) -> RuntimeStateFixture {
    let (input, input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    let state = Mutex::new(ServerState {
        session_id: Some(session.into()),
        runtime: Some(RuntimeSession {
            session_id: session.into(),
            input,
            stop: stop_requests::new_stop_handle_slot(),
            exit,
            abort: runtime_task.abort_handle(),
            command_forwarder: std::thread::spawn(|| {}),
        }),
        ..ServerState::default()
    });
    (state, input_receiver, runtime_task, exit_sender)
}

fn completed_turn() -> vtcode_core::exec::events::VersionedThreadEvent {
    use vtcode_core::exec::events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};
    VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
        final_response_item_id: None,
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    }))
}

#[tokio::test]
async fn busy_desktop_submits_queue_behind_the_running_turn() {
    let (state, mut input_receiver, runtime_task, exit_sender) = runtime_state("session");
    let (notifications, mut received) = mpsc::channel(8);

    let immediate = enqueue_turn(
        &state,
        &notifications,
        "session",
        "first".into(),
        Vec::new(),
        EnqueueMode::Queueable { batchable: false },
    )
    .await
    .expect("idle submit starts a turn");
    assert!(!immediate.queued);
    assert!(!immediate.turn_id.is_empty());
    let Some(InlineEvent::Submit(submission)) = input_receiver.recv().await else {
        panic!("idle prompt crosses the runtime boundary");
    };
    assert_eq!(submission.text, "first");
    assert_eq!(received.recv().await.expect("turn status").method, protocol::method::TURN_STATUS);

    for (prompt, depth) in [("second", 1), ("third", 2)] {
        let accepted = enqueue_turn(
            &state,
            &notifications,
            "session",
            prompt.into(),
            Vec::new(),
            EnqueueMode::Queueable { batchable: false },
        )
        .await
        .expect("busy submit queues");
        assert!(accepted.queued, "{prompt}");
        assert!(accepted.turn_id.is_empty(), "queued prompt owns no turn yet: {prompt}");
        assert_eq!(accepted.queue_depth, depth, "{prompt}");
        assert_eq!(accepted.dropped, 0, "{prompt}");
        let status = received.recv().await.expect("queue status");
        assert_eq!(status.method, protocol::method::TURN_QUEUE_STATUS);
        assert_eq!(status.params["queueDepth"], depth);
    }
    assert!(input_receiver.try_recv().is_err(), "queued prompts wait for the turn to end");
    assert_eq!(state.lock().await.queue.len(), 2);

    let error = enqueue_turn(&state, &notifications, "session", "web".into(), Vec::new(), EnqueueMode::Immediate)
        .await
        .expect_err("immediate mode still rejects a busy session");
    assert!(error.to_string().contains("another turn is already running"));

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn terminal_event_drains_one_batch_and_keeps_fifo_order() {
    let (state, mut input_receiver, runtime_task, exit_sender) = runtime_state("session");
    let (notifications, mut received) = mpsc::channel(16);
    let finished = Notify::new();

    enqueue_turn(&state, &notifications, "session", "running".into(), Vec::new(), EnqueueMode::Immediate)
        .await
        .expect("idle submit");
    let _ = input_receiver.recv().await;
    let _ = received.recv().await;
    for (prompt, batchable) in [("a", true), ("b", true), ("c", false)] {
        enqueue_turn(
            &state,
            &notifications,
            "session",
            prompt.into(),
            Vec::new(),
            EnqueueMode::Queueable { batchable },
        )
        .await
        .expect("queue behind the turn");
        let _ = received.recv().await;
    }

    publish_thread_event("session", completed_turn(), &state, &notifications, &finished)
        .await
        .expect("terminal publish drains");
    let Some(InlineEvent::Submit(batch)) = input_receiver.recv().await else {
        panic!("drained batch crosses the runtime boundary");
    };
    assert_eq!(batch.text, "a\n\nb", "batchable run coalesces into one turn");
    assert_eq!(state.lock().await.queue.len(), 1, "plain prompt waits for the next drain");
    assert_eq!(received.recv().await.expect("thread event").method, protocol::method::THREAD_EVENT);
    let completed = received.recv().await.expect("completed status");
    assert_eq!(completed.params["status"], "completed");
    let running = received.recv().await.expect("running status");
    assert_eq!(running.params["status"], "running");
    assert!(!running.params["turnId"].as_str().unwrap_or_default().is_empty());
    let status = received.recv().await.expect("queue status");
    assert_eq!(status.method, protocol::method::TURN_QUEUE_STATUS);
    assert_eq!(status.params["queueDepth"], 1);
    assert_eq!(status.params["previews"], serde_json::json!(["c"]));

    publish_thread_event("session", completed_turn(), &state, &notifications, &finished)
        .await
        .expect("second terminal publish drains");
    let Some(InlineEvent::Submit(solo)) = input_receiver.recv().await else {
        panic!("plain prompt dispatches on its own turn");
    };
    assert_eq!(solo.text, "c");
    assert!(state.lock().await.queue.is_empty());

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn terminal_event_keeps_queued_prompts_during_session_transition() {
    let (state, mut input_receiver, runtime_task, exit_sender) = runtime_state("session");
    {
        let mut state = state.lock().await;
        state.active_turn_id = Some("old-turn".into());
        state.session_transitioning = true;
        state.queue = VecDeque::from([
            QueuedInput::new("first queued".into(), None),
            QueuedInput::new("second queued".into(), None),
        ]);
    }
    let (notifications, _received) = mpsc::channel(4);
    publish_thread_event("session", completed_turn(), &state, &notifications, &Notify::new())
        .await
        .expect("terminal event");
    let mut state = state.lock().await;
    assert!(state.active_turn_id.is_none());
    assert_eq!(state.queue.len(), 2);
    assert!(input_receiver.try_recv().is_err(), "the old runtime must not receive a queued prompt");
    state.session_transitioning = false;
    let drained = state.pop_next_queued_turn().expect("queue resumes after transition");
    assert_eq!(drained.batch.input.text, "first queued");
    assert_eq!(state.queue.front().unwrap().input.text, "second queued");
    drop(state);
    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn installed_runtime_resumes_queue_only_after_readiness_and_transition() {
    use crate::agent::runloop::unified::state::CtrlCState;
    let workspace = hermetic_workspace();
    let (context, _notifications) =
        test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
    {
        let mut state = context.state.lock().await;
        state.session_transitioning = true;
        state.queue = VecDeque::from([
            QueuedInput::new("first queued".into(), None),
            QueuedInput::new("second queued".into(), None),
        ]);
    }
    let (input, mut input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    let stop = stop_requests::new_stop_handle_slot();
    let (_commands, command_receiver) = mpsc::channel(1);
    let (_events, event_receiver) = protocol::live_event_channel(1);
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
    tokio::task::yield_now().await;
    assert!(input_receiver.try_recv().is_err(), "startup has not made the runtime ready");
    stop_requests::publish_stop_handles(
        &stop,
        &std::sync::Arc::new(CtrlCState::new()),
        &std::sync::Arc::new(Notify::new()),
    );
    stop_requests::mark_host_turns_ready(&stop);
    tokio::task::yield_now().await;
    assert!(input_receiver.try_recv().is_err(), "the session transition is still active");
    context.state.lock().await.session_transitioning = false;
    context.turn_finished.notify_waiters();
    let event = timeout(Duration::from_secs(1), input_receiver.recv()).await.unwrap();
    assert!(matches!(event, Some(InlineEvent::Submit(input)) if input.text == "first queued"));
    assert_eq!(context.state.lock().await.queue.front().unwrap().input.text, "second queued");
    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn take_latest_request_pops_newest_for_editing() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, mut notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let (input, _input_receiver) = mpsc::unbounded_channel();
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
    let queueable = EnqueueMode::Queueable { batchable: false };
    enqueue_turn(&context.state, &context.notifications, "session", "running".into(), Vec::new(), queueable)
        .await
        .expect("idle submit");
    for prompt in ["older", "newer"] {
        enqueue_turn(&context.state, &context.notifications, "session", prompt.into(), Vec::new(), queueable)
            .await
            .expect("queue behind the turn");
    }
    for _ in 0..3 {
        notifications.recv().await.expect("drain setup notifications");
    }

    let take_request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(1),
        method: protocol::method::TURN_QUEUE_TAKE_LATEST.into(),
        params: serde_json::json!({ "sessionId": "session" }),
    };
    for (expected_text, expected_depth) in [(Some("newer"), 1), (Some("older"), 0)] {
        let (result, stop) = dispatch::handle_request_inner(&context, &take_request)
            .await
            .expect("take-latest");
        assert!(!stop);
        let taken: protocol::TurnQueueTakeLatestResult = serde_json::from_value(result).expect("result");
        assert_eq!(taken.text.as_deref(), expected_text);
        assert_eq!(taken.queue_depth, expected_depth);
        let status = notifications.recv().await.expect("queue status on pop");
        assert_eq!(status.method, protocol::method::TURN_QUEUE_STATUS);
        assert_eq!(status.params["queueDepth"], expected_depth);
    }
    let (result, _) = dispatch::handle_request_inner(&context, &take_request)
        .await
        .expect("empty take-latest");
    let taken: protocol::TurnQueueTakeLatestResult = serde_json::from_value(result).expect("result");
    assert_eq!(taken.text, None);
    assert_eq!(taken.queue_depth, 0);
    assert!(notifications.try_recv().is_err(), "empty pop publishes no status");

    let foreign = Request {
        params: serde_json::json!({ "sessionId": "other" }),
        ..take_request
    };
    assert!(dispatch::handle_request_inner(&context, &foreign).await.is_err(), "wrong session rejected");

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn remove_request_deletes_one_queued_message_by_entry_id() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, mut notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let (input, _input_receiver) = mpsc::unbounded_channel();
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
    let queueable = EnqueueMode::Queueable { batchable: false };
    enqueue_turn(&context.state, &context.notifications, "session", "running".into(), Vec::new(), queueable)
        .await
        .expect("idle submit");
    for prompt in ["older", "middle", "newer"] {
        enqueue_turn(&context.state, &context.notifications, "session", prompt.into(), Vec::new(), queueable)
            .await
            .expect("queue behind the turn");
    }
    // Turn status from the immediate submit plus one queue status per queued entry.
    for _ in 0..4 {
        notifications.recv().await.expect("drain setup notifications");
    }
    let entry_ids: Vec<String> = {
        let state = context.state.lock().await;
        state.queue.iter().map(|entry| entry.entry_id()).collect()
    };
    assert_eq!(entry_ids.len(), 3, "every queue entry carries a service-minted id");

    // Removing the middle entry keeps FIFO order for the rest.
    let remove = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(1),
        method: protocol::method::TURN_QUEUE_REMOVE.into(),
        params: serde_json::json!({ "sessionId": "session", "entryId": entry_ids[1] }),
    };
    let (result, stop) = dispatch::handle_request_inner(&context, &remove).await.expect("remove");
    assert!(!stop);
    let removed: protocol::TurnQueueRemoveResult = serde_json::from_value(result).expect("result");
    assert!(removed.removed);
    assert_eq!(removed.queue_depth, 2);
    let status = notifications.recv().await.expect("queue status on remove");
    assert_eq!(status.method, protocol::method::TURN_QUEUE_STATUS);
    assert_eq!(status.params["queueDepth"], 2);
    let previews: Vec<String> = status.params["previews"]
        .as_array()
        .expect("previews")
        .iter()
        .map(|value| value.as_str().expect("string preview").to_string())
        .collect();
    assert_eq!(previews, vec!["older".to_string(), "newer".to_string()], "removal preserves FIFO order");
    // The remove mutates through a fresh id lookup, so the remaining ids match
    // the published snapshot one-to-one.
    let published_ids: Vec<String> = status.params["entryIds"]
        .as_array()
        .expect("entry ids")
        .iter()
        .map(|value| value.as_str().expect("string entry id").to_string())
        .collect();
    assert_eq!(published_ids, vec![entry_ids[0].clone(), entry_ids[2].clone()]);

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn remove_request_with_a_stale_id_reports_not_removed_without_an_error() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, mut notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let (input, _input_receiver) = mpsc::unbounded_channel();
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
    let queueable = EnqueueMode::Queueable { batchable: false };
    enqueue_turn(&context.state, &context.notifications, "session", "running".into(), Vec::new(), queueable)
        .await
        .expect("idle submit");
    enqueue_turn(&context.state, &context.notifications, "session", "queued".into(), Vec::new(), queueable)
        .await
        .expect("queue behind the turn");
    for _ in 0..2 {
        notifications.recv().await.expect("drain setup notifications");
    }

    let remove = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(1),
        method: protocol::method::TURN_QUEUE_REMOVE.into(),
        params: serde_json::json!({ "sessionId": "session", "entryId": "qe-gone" }),
    };
    let (result, stop) = dispatch::handle_request_inner(&context, &remove).await.expect("stale remove");
    assert!(!stop);
    let removed: protocol::TurnQueueRemoveResult = serde_json::from_value(result).expect("result");
    assert!(!removed.removed, "stale id reports a clean miss");
    assert_eq!(removed.queue_depth, 1, "the live entry stays queued");
    assert!(notifications.try_recv().is_err(), "a miss publishes no status");

    // Foreign session ids still fail closed.
    let foreign = Request {
        params: serde_json::json!({ "sessionId": "other", "entryId": "qe-gone" }),
        ..remove
    };
    assert!(dispatch::handle_request_inner(&context, &foreign).await.is_err(), "wrong session rejected");

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn soft_cap_drops_oldest_and_counts_the_gap() {
    let (state, _input_receiver, runtime_task, exit_sender) = runtime_state("session");
    let (notifications, _received) = mpsc::channel(512);
    enqueue_turn(&state, &notifications, "session", "running".into(), Vec::new(), EnqueueMode::Immediate)
        .await
        .expect("idle submit");

    let queueable = EnqueueMode::Queueable { batchable: false };
    let mut last = None;
    for index in 0..258 {
        last = Some(
            enqueue_turn(&state, &notifications, "session", format!("msg-{index}"), Vec::new(), queueable)
                .await
                .expect("queue accepts under and over the cap"),
        );
    }
    let last = last.expect("acceptance");
    assert_eq!(last.queue_depth, 256);
    assert_eq!(last.dropped, 1, "258th prompt displaces exactly one oldest message");
    let state = state.lock().await;
    assert_eq!(state.queue.len(), 256);
    assert_eq!(state.queue_dropped_total, 2);
    assert_eq!(state.queue.front().expect("oldest kept").input.text, "msg-2", "oldest is dropped first");
    assert_eq!(state.queue.back().expect("newest kept").input.text, "msg-257", "newest is always kept");

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn dead_runtime_keeps_queued_prompts_instead_of_dispatching() {
    let (state, input_receiver, runtime_task, exit_sender) = runtime_state("session");
    let (notifications, mut received) = mpsc::channel(8);
    let finished = Notify::new();
    enqueue_turn(&state, &notifications, "session", "running".into(), Vec::new(), EnqueueMode::Immediate)
        .await
        .expect("idle submit");
    enqueue_turn(
        &state,
        &notifications,
        "session",
        "waiting".into(),
        Vec::new(),
        EnqueueMode::Queueable { batchable: false },
    )
    .await
    .expect("queue behind the turn");
    drop(input_receiver);
    let _ = received.recv().await;
    let _ = received.recv().await;

    publish_thread_event("session", completed_turn(), &state, &notifications, &finished)
        .await
        .expect("terminal publish with dead runtime");
    let state = state.lock().await;
    assert!(state.active_turn_id.is_none());
    assert_eq!(state.queue.len(), 1, "undispatchable batch stays queued");
    assert_eq!(state.queue.front().expect("kept prompt").input.text, "waiting");
    assert_eq!(received.recv().await.expect("thread event").method, protocol::method::THREAD_EVENT);
    let completed = received.recv().await.expect("completed status");
    assert_eq!(completed.params["status"], "completed");
    assert!(received.try_recv().is_err(), "no phantom running turn is published");

    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn failed_drain_dispatch_restores_the_batch_at_the_front() {
    let (input, input_receiver) = mpsc::unbounded_channel::<InlineEvent>();
    drop(input_receiver);
    let (notifications, mut received) = mpsc::channel(4);
    let state = Mutex::new(ServerState {
        session_id: Some("session".into()),
        active_turn_id: Some("turn".into()),
        queue: VecDeque::from([QueuedInput::new("rest".into(), None)]),
        ..ServerState::default()
    });
    let batch = QueuedInput::new("drained".into(), None);

    dispatch_drained_turn(&state, &notifications, &Notify::new(), "session", "turn".into(), batch, input)
        .await
        .expect_err("closed runtime fails delivery");
    let state = state.lock().await;
    assert!(state.active_turn_id.is_none());
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue.front().expect("restored batch").input.text, "drained");
    assert_eq!(state.queue.back().expect("untouched rest").input.text, "rest");
    let status = received.try_recv().expect("queue status resync");
    assert_eq!(status.method, protocol::method::TURN_QUEUE_STATUS);
    assert_eq!(status.params["queueDepth"], 2);
}

#[tokio::test]
async fn failed_drain_dispatch_restores_without_touching_a_newer_turn() {
    let (input, input_receiver) = mpsc::unbounded_channel::<InlineEvent>();
    drop(input_receiver);
    let (notifications, mut received) = mpsc::channel(4);
    let state = Mutex::new(ServerState {
        session_id: Some("session".into()),
        active_turn_id: Some("newer-turn".into()),
        ..ServerState::default()
    });
    let batch = QueuedInput::new("drained".into(), None);

    dispatch_drained_turn(&state, &notifications, &Notify::new(), "session", "stale-turn".into(), batch, input)
        .await
        .expect_err("closed runtime fails delivery");
    let state = state.lock().await;
    assert_eq!(state.active_turn_id.as_deref(), Some("newer-turn"), "newer turn slot is untouched");
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.queue.front().expect("restored batch").input.text, "drained");
    let status = received.try_recv().expect("queue status resync");
    assert_eq!(status.params["queueDepth"], 1);
}

#[tokio::test]
async fn stale_open_runtime_cannot_receive_a_drained_prompt() {
    let (state, mut current_input, runtime_task, exit_sender) = runtime_state("session");
    let (old_input, mut old_receiver) = mpsc::unbounded_channel();
    let (notifications, _received) = mpsc::channel(8);
    let finished = Notify::new();
    state.lock().await.active_turn_id = Some("old-turn".into());
    dispatch_drained_turn(
        &state,
        &notifications,
        &finished,
        "session",
        "old-turn".into(),
        QueuedInput::new("preserved prompt".into(), None),
        old_input,
    )
    .await
    .expect_err("an open but replaced runtime is stale");
    assert!(old_receiver.try_recv().is_err());
    let drained = state.lock().await.pop_next_queued_turn().expect("prompt is restored");
    dispatch_drained_turn(&state, &notifications, &finished, "session", drained.turn_id, drained.batch, drained.input)
        .await
        .expect("current runtime receives the restored prompt");
    assert!(matches!(current_input.recv().await, Some(InlineEvent::Submit(input)) if input.text == "preserved prompt"));
    drop(exit_sender);
    runtime_task.abort();
}

#[tokio::test]
async fn session_teardown_clears_ephemeral_queue_state() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _notifications) = test_service_context(test_startup_context(&workspace), workspace);
    let (input, _input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    {
        let mut state = context.state.lock().await;
        state.session_id = Some("session".into());
        state.runtime = Some(RuntimeSession {
            session_id: "session".into(),
            input,
            stop: stop_requests::new_stop_handle_slot(),
            exit,
            abort: runtime_task.abort_handle(),
            command_forwarder: std::thread::spawn(|| {}),
        });
        state.queue = VecDeque::from([QueuedInput::new("stale".into(), None)]);
        state.queue_dropped_total = 7;
    }
    shutdown_current_session(&context, false).await.expect("teardown");
    let state = context.state.lock().await;
    assert!(state.queue.is_empty());
    assert_eq!(state.queue_dropped_total, 0);

    drop(exit_sender);
    runtime_task.abort();
}
