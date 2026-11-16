use super::*;
use std::sync::Arc;

/// A selected desktop session ("session") with a published goal tracker.
struct GoalHarness {
    context: ServiceContext,
    events_path: PathBuf,
    _runtime_task: tokio::task::JoinHandle<()>,
    _workspace_guard: tempfile::TempDir,
}

impl GoalHarness {
    async fn new() -> Self {
        let workspace_guard = hermetic_workspace();
        let workspace = workspace_guard.path().to_path_buf();
        let (context, _notifications) = test_service_context(test_startup_context(&workspace), workspace);
        let (input_sender, _input) = mpsc::unbounded_channel();
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
                stop: stop.clone(),
                exit,
                abort: runtime_task.abort_handle(),
                command_forwarder: std::thread::spawn(|| {}),
            });
        }
        let tracker = Arc::new(Mutex::new(vtcode_memory::GoalTracker::new(workspace_guard.path().join("session-1"))));
        stop_requests::publish_goal_tracker(&stop, &tracker);
        let events_path = workspace_guard.path().join("events.jsonl");
        let emitter = stop_requests::HarnessEventEmitter::new(events_path.clone()).unwrap();
        stop_requests::publish_goal_emitter(&stop, &emitter);
        Self {
            context,
            events_path,
            _runtime_task: runtime_task,
            _workspace_guard: workspace_guard,
        }
    }

    fn request(&self, session_id: &str, args: &str) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: RequestId::Number(1),
            method: protocol::method::GOAL_EXECUTE.into(),
            params: serde_json::json!({ "sessionId": session_id, "args": args }),
        }
    }

    async fn execute(&self, session_id: &str, args: &str) -> protocol::GoalExecuteResult {
        let request = self.request(session_id, args);
        let (value, _) = handle_request(&self.context, &request).await.expect("goal execute");
        serde_json::from_value(value).expect("goal result")
    }
}

#[tokio::test]
async fn goal_execute_set_starts_work_and_returns_snapshot() {
    let harness = GoalHarness::new().await;
    let result = harness.execute("session", "Ship the release").await;

    assert!(result.mutated);
    assert_eq!(result.submit.as_deref(), Some("Ship the release"));
    assert!(result.lines.iter().any(|l| l.contains("Goal set: Ship the release")), "{:?}", result.lines);
    let snapshot = result.goal.expect("set must return a snapshot");
    assert_eq!(snapshot.objective, "Ship the release");
    assert_eq!(snapshot.state, vtcode_core::exec::events::goal::GoalState::Active);
}

#[tokio::test]
async fn goal_execute_pause_and_resume_round_trip() {
    let harness = GoalHarness::new().await;
    harness.execute("session", "Ship it").await;

    let paused = harness.execute("session", "pause").await;
    assert!(paused.mutated);
    assert!(paused.submit.is_none());
    assert_eq!(paused.goal.map(|g| g.state), Some(vtcode_core::exec::events::goal::GoalState::Paused));

    let repeat = harness.execute("session", "pause").await;
    assert!(!repeat.mutated);
    assert!(repeat.lines.iter().any(|l| l.contains("already")), "{:?}", repeat.lines);

    let resumed = harness.execute("session", "resume").await;
    assert!(resumed.mutated);
    assert!(resumed.submit.as_deref().is_some_and(|p| p.contains("Ship it")));
}

#[tokio::test]
async fn goal_execute_emits_canonical_event_on_mutation() {
    let harness = GoalHarness::new().await;
    harness.execute("session", "Ship it").await;
    harness.execute("session", "pause").await;

    let log = std::fs::read_to_string(&harness.events_path).unwrap();
    assert!(log.contains("\"goal.updated\""), "mutations must emit: {log}");
    assert!(log.contains("Ship it"), "emitted snapshot must carry the objective");
}

#[tokio::test]
async fn goal_execute_budget_sets_shows_and_rejects_garbage() {
    let harness = GoalHarness::new().await;
    harness.execute("session", "Ship it").await;

    let set = harness.execute("session", "budget 5000").await;
    assert!(set.mutated);
    assert!(set.lines.iter().any(|l| l.contains("Budget set: 5000 tokens")), "{:?}", set.lines);

    let shown = harness.execute("session", "budget").await;
    assert!(!shown.mutated);
    assert!(shown.lines.iter().any(|l| l.contains("Budget:")), "{:?}", shown.lines);

    let request = harness.request("session", "budget lots");
    let err = handle_request(&harness.context, &request)
        .await
        .expect_err("garbage budget must fail");
    assert!(err.to_string().contains("invalid budget"), "{err:#}");
}

#[tokio::test]
async fn goal_execute_steers_live_turn_instead_of_queueing() {
    let harness = GoalHarness::new().await;
    let (steer_tx, mut steer_rx) = mpsc::unbounded_channel();
    {
        let mut state = harness.context.state.lock().await;
        state.active_turn_id = Some("turn-1".into());
        let runtime = state.runtime.as_ref().expect("runtime");
        stop_requests::publish_steering_sender(&runtime.stop, &steer_tx);
    }

    let result = harness.execute("session", "Ship it").await;
    assert!(result.mutated);
    assert!(result.submit.is_none(), "steered submit must not queue a turn");
    let steered = steer_rx.try_recv().expect("steering message");
    assert!(
        matches!(steered, vtcode_core::core::agent::steering::SteeringMessage::FollowUpInput(text) if text == "Ship it"),
        "objective must steer the live turn"
    );
}

#[tokio::test]
async fn goal_execute_falls_back_to_submit_without_steering_channel() {
    let harness = GoalHarness::new().await;
    {
        let mut state = harness.context.state.lock().await;
        state.active_turn_id = Some("turn-1".into());
    }

    let result = harness.execute("session", "Ship it").await;
    assert_eq!(result.submit.as_deref(), Some("Ship it"));
}

#[tokio::test]
async fn goal_execute_rejects_unknown_session_and_unready_runtime() {
    let harness = GoalHarness::new().await;
    let request = harness.request("other-session", "pause");
    let err = handle_request(&harness.context, &request)
        .await
        .expect_err("wrong session must fail");
    assert!(err.to_string().contains("selected session"), "{err:#}");

    let workspace = hermetic_workspace();
    let (context, _) = test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
    {
        let mut state = context.state.lock().await;
        state.initialized = true;
        state.session_id = Some("session".into());
    }
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(2),
        method: protocol::method::GOAL_EXECUTE.into(),
        params: serde_json::json!({ "sessionId": "session", "args": "pause" }),
    };
    let err = handle_request(&context, &request).await.expect_err("missing runtime must fail");
    assert!(err.to_string().contains("not running"), "{err:#}");
}
