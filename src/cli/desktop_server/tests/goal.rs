use super::*;
use std::sync::Arc;

/// A selected desktop session ("session") with a published goal tracker.
struct GoalHarness {
    context: ServiceContext,
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
        Self {
            context,
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
