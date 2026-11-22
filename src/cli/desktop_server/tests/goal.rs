use super::*;
use std::sync::Arc;

/// A selected desktop session ("session") with a published goal tracker.
struct GoalHarness {
    context: ServiceContext,
    events_path: PathBuf,
    stats: crate::agent::runloop::unified::state::SessionStats,
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
        let mut stats = crate::agent::runloop::unified::state::SessionStats::default();
        let token_total = stats.share_goal_token_total();
        let events_path = workspace_guard.path().join("events.jsonl");
        let emitter = stop_requests::HarnessEventEmitter::new(events_path.clone()).unwrap();
        stop_requests::publish_goal_handles(
            &stop,
            stop_requests::HostGoalHandles { tracker, token_total, emitter: Some(emitter) },
        );
        Self {
            context,
            events_path,
            stats,
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
async fn goal_execute_only_charges_active_intervals_and_counts_cached_input_once() {
    let mut harness = GoalHarness::new().await;
    let sample = |prompt_tokens, completion_tokens, cached_prompt_tokens| vtcode_core::llm::provider::Usage {
        prompt_tokens,
        completion_tokens,
        cached_prompt_tokens: Some(cached_prompt_tokens),
        ..Default::default()
    };
    harness.stats.record_usage("openai", Some(&sample(1000, 19, 700)));
    let set = harness.execute("session", "Ship it").await;
    assert_eq!(set.goal.unwrap().tokens_used, 0);
    harness.stats.record_usage("openai", Some(&sample(160, 11, 130)));
    let paused = harness.execute("session", "pause").await;
    assert_eq!(paused.goal.unwrap().tokens_used, 171);
    harness.stats.record_usage("openai", Some(&sample(500, 29, 300)));
    let resumed = harness.execute("session", "resume").await;
    assert_eq!(resumed.goal.unwrap().tokens_used, 171);
    harness.stats.record_usage("openai", Some(&sample(90, 7, 35)));
    let status = harness.execute("session", "").await;
    assert_eq!(status.goal.unwrap().tokens_used, 268);
    harness.stats.record_usage(
        "anthropic",
        Some(&vtcode_core::llm::provider::Usage {
            prompt_tokens: 60,
            completion_tokens: 9,
            cache_read_tokens: Some(21),
            cache_creation_tokens: Some(17),
            ..Default::default()
        }),
    );
    let status = harness.execute("session", "").await;
    assert_eq!(status.goal.unwrap().tokens_used, 375);
}

#[tokio::test]
async fn failed_goal_persist_keeps_the_current_goal_and_allows_retry() {
    let harness = GoalHarness::new().await;
    let original = harness.execute("session", "Ship it").await.goal.unwrap();
    let path = harness._workspace_guard.path().join("session-1/goal/goal.json");
    let backup = path.with_extension("backup");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();

    let request = harness.request("session", "Write release notes");
    let error = handle_request(&harness.context, &request).await.unwrap_err();
    assert!(error.to_string().contains("goal snapshot persist failed"));
    let current = harness.execute("session", "").await.goal.unwrap();
    assert_eq!(current.goal_id, original.goal_id);
    assert_eq!(current.objective, "Ship it");

    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(backup, &path).unwrap();
    let updated = harness.execute("session", "Write release notes").await.goal.unwrap();
    assert_ne!(updated.goal_id, original.goal_id);
    let restored = vtcode_memory::GoalTracker::restore(harness._workspace_guard.path().join("session-1")).unwrap();
    assert_eq!(restored.objective(), Some("Write release notes"));
}

#[tokio::test]
async fn goal_execute_set_starts_work_and_returns_snapshot() {
    let harness = GoalHarness::new().await;
    let result = harness.execute("session", "Ship the release").await;

    assert!(result.mutated);
    let submit = result.submit.as_deref().expect("set must submit a turn");
    assert!(submit.starts_with("Ship the release"), "{submit}");
    assert!(submit.contains("GOAL ACHIEVED:"), "submit must teach the completion contract: {submit}");
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
    let submit = resumed.submit.as_deref().expect("resume must submit a turn");
    assert!(submit.contains("Ship it"), "{submit}");
    assert!(submit.contains("GOAL ACHIEVED:"), "resume must re-teach the completion contract: {submit}");
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
async fn goal_execute_returns_submit_while_a_turn_is_active() {
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
    let submit = result.submit.as_deref().expect("the client can queue the new goal input");
    assert!(submit.starts_with("Ship it"));
    assert!(submit.contains("Goal activation ID"));
    assert!(steer_rx.try_recv().is_err());
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
