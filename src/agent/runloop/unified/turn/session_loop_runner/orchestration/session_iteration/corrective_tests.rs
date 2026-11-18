use super::*;
use vtcode_core::core::agent::session::AgentSessionState;

#[test]
fn planning_budget_stop_includes_runtime_block_reason() {
    assert!(planning_budget_exhausted(false, Some("Tool loop budget exhausted (60/60).")));
    assert!(planning_budget_exhausted(false, Some("Tool wall-clock budget exhausted for this turn (600s).")));
    assert!(planning_budget_exhausted(true, None));
    assert!(!planning_budget_exhausted(false, Some("Turn ended with a recovery fallback")));
    assert!(!planning_budget_exhausted(false, None));
}

#[tokio::test]
async fn budget_stop_does_not_queue_a_corrective_follow_up() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;
    for (reason, should_queue) in [
        (
            "Tool loop budget exhausted (60/60). Changes remain unverified; no additional verifier was scheduled.",
            false,
        ),
        ("Tool wall-clock budget exhausted for this turn (600s).", false),
        ("Tool loop budget exhausted before a final response.", false),
        (
            "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained.",
            true,
        ),
    ] {
        let mut stats = SessionStats::default();
        let mut runtime = AgentRuntime::new(AgentSessionState::new("test".into(), 4, 16_000), None, None);
        let mut renderer = AnsiRenderer::stdout();
        let plan = PlanningWorkflowSessionState::default();
        let outcome = RunLoopTurnLoopResult::Blocked { reason: Some(reason.into()) };
        let ctrl_c = Arc::new(CtrlCState::new());
        let goal_tracker = noop_goal_tracker();
        let flow = run_corrective_gate(CorrectiveGate {
            session_stats: &mut stats,
            runtime: &mut runtime,
            renderer: &mut renderer,
            tool_registry: &registry,
            plan_session: &plan,
            goal_tracker: &goal_tracker,
            harness_emitter: None,
            vt_cfg: &None,
            outcome_result: &outcome,
            ctrl_c_state: &ctrl_c,
            next_turn_input: "original request",
            turn_refused: false,
            refused_turn_rolled_back: false,
            session_end_reason: SessionEndReason::Completed,
        })
        .await
        .unwrap();
        assert_eq!(runtime.has_pending_follow_up_inputs(), should_queue, "{reason}");
        assert_eq!(matches!(flow, CorrectiveFlow::ContinueLoop), should_queue, "{reason}");
        if !should_queue {
            runtime.try_queue_follow_up_input("continue".into()).unwrap();
            assert_eq!(runtime.run_until_idle().as_deref(), Some("continue"));
        }
    }
}

fn noop_goal_tracker() -> SharedGoalTracker {
    Arc::new(tokio::sync::Mutex::new(vtcode_memory::GoalTracker::new(
        std::env::temp_dir().join("vtcode-goal-test-noop"),
    )))
}

async fn active_goal_tracker(objective: &str) -> (tempfile::TempDir, SharedGoalTracker) {
    let dir = tempfile::tempdir().unwrap();
    let tracker = vtcode_memory::GoalTracker::new(dir.path().join("session-1"));
    let handle = Arc::new(tokio::sync::Mutex::new(tracker));
    handle
        .lock()
        .await
        .create_goal("goal-1".into(), objective.into(), None, 0, "2026-01-01T00:00:00Z".into(), None);
    (dir, handle)
}

#[tokio::test]
async fn active_goal_queues_continue_after_completed_turn() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;
    let mut stats = SessionStats::default();
    let mut runtime = AgentRuntime::new(AgentSessionState::new("test".into(), 4, 16_000), None, None);
    let mut renderer = AnsiRenderer::stdout();
    let plan = PlanningWorkflowSessionState::default();
    let outcome = RunLoopTurnLoopResult::Completed { plan_approved_execution_pending: false };
    let ctrl_c = Arc::new(CtrlCState::new());
    let (goal_dir, goal_tracker) = active_goal_tracker("Ship the release").await;

    let events_path = goal_dir.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(events_path.clone()).unwrap();
    let flow = run_corrective_gate(CorrectiveGate {
        session_stats: &mut stats,
        runtime: &mut runtime,
        renderer: &mut renderer,
        tool_registry: &registry,
        plan_session: &plan,
        goal_tracker: &goal_tracker,
        harness_emitter: Some(&emitter),
        vt_cfg: &None,
        outcome_result: &outcome,
        ctrl_c_state: &ctrl_c,
        next_turn_input: "",
        turn_refused: false,
        refused_turn_rolled_back: false,
        session_end_reason: SessionEndReason::Completed,
    })
    .await
    .unwrap();

    assert!(matches!(flow, CorrectiveFlow::ContinueLoop));
    assert_eq!(goal_tracker.lock().await.auto_continue_turns(), 1);
    let queued = runtime.run_until_idle().expect("goal follow-up must be queued");
    assert!(queued.contains("Ship the release"), "follow-up must carry the objective: {queued}");
    assert!(
        goal_dir.path().join("session-1").join("goal").join("goal.json").exists(),
        "turn boundary must persist the snapshot"
    );
    let log = std::fs::read_to_string(&events_path).unwrap();
    assert!(log.contains("\"goal.updated\""), "active goal must emit per turn: {log}");
    assert!(log.contains("Ship the release"), "emitted snapshot must carry the objective");
}

async fn goal_gate_queues(
    registry: &ToolRegistry,
    goal_tracker: &SharedGoalTracker,
    outcome: RunLoopTurnLoopResult,
    next_input: &str,
    refused: bool,
    seed_pending: bool,
) -> (bool, bool) {
    let mut stats = SessionStats::default();
    let mut runtime = AgentRuntime::new(AgentSessionState::new("test".into(), 4, 16_000), None, None);
    if seed_pending {
        runtime
            .try_queue_synthetic_follow_up_input("already queued".to_string())
            .unwrap();
    }
    let mut renderer = AnsiRenderer::stdout();
    let plan = PlanningWorkflowSessionState::default();
    let ctrl_c = Arc::new(CtrlCState::new());
    let flow = run_corrective_gate(CorrectiveGate {
        session_stats: &mut stats,
        runtime: &mut runtime,
        renderer: &mut renderer,
        tool_registry: registry,
        plan_session: &plan,
        goal_tracker,
        harness_emitter: None,
        vt_cfg: &None,
        outcome_result: &outcome,
        ctrl_c_state: &ctrl_c,
        next_turn_input: next_input,
        turn_refused: refused,
        refused_turn_rolled_back: false,
        session_end_reason: SessionEndReason::Completed,
    })
    .await
    .unwrap();
    (matches!(flow, CorrectiveFlow::ContinueLoop), runtime.has_pending_follow_up_inputs())
}

fn completed_turn() -> RunLoopTurnLoopResult {
    RunLoopTurnLoopResult::Completed { plan_approved_execution_pending: false }
}

#[tokio::test]
async fn inactive_goal_or_busy_thread_never_auto_continues() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;

    let noop = noop_goal_tracker();
    assert_eq!(
        goal_gate_queues(&registry, &noop, completed_turn(), "", false, false).await,
        (false, false),
        "no goal must idle"
    );

    let (_dir, paused) = active_goal_tracker("Ship it").await;
    assert!(paused.lock().await.pause(vtcode_memory::GoalPauseReason::User));
    assert_eq!(
        goal_gate_queues(&registry, &paused, completed_turn(), "", false, false).await,
        (false, false),
        "paused goal must idle"
    );

    let (_dir, active) = active_goal_tracker("Ship it").await;
    let blocked = RunLoopTurnLoopResult::Blocked {
        reason: Some("Tool loop budget exhausted (60/60).".into()),
    };
    assert_eq!(
        goal_gate_queues(&registry, &active, blocked, "", false, false).await,
        (false, false),
        "budget stop must idle, never auto-continue"
    );
    assert_eq!(
        goal_gate_queues(&registry, &active, completed_turn(), "", true, false).await,
        (false, false),
        "refused turn must idle"
    );
}

#[tokio::test]
async fn active_goal_queues_despite_turn_input_but_idles_on_pending_queue() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;
    let (_dir, active) = active_goal_tracker("Ship it").await;

    // `next_turn_input` is the just-finished turn's own prompt, which is
    // non-empty for every user turn: it must never block auto-continue.
    assert_eq!(
        goal_gate_queues(&registry, &active, completed_turn(), "user typed this", false, false).await,
        (true, true),
        "non-empty turn input must still queue"
    );
    // A queued follow-up already waiting does block: the gate re-runs
    // after that turn, so idling here loses nothing.
    assert_eq!(
        goal_gate_queues(&registry, &active, completed_turn(), "", false, true).await,
        (false, true),
        "pending follow-up must idle with the queue intact"
    );
    assert_eq!(active.lock().await.auto_continue_turns(), 1, "only the queued attempt may consume budget");
}

#[tokio::test]
async fn goal_token_budget_limits_and_idles_without_queueing() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;
    let mut stats = SessionStats::default();
    let mut runtime = AgentRuntime::new(AgentSessionState::new("test".into(), 4, 16_000), None, None);
    let mut renderer = AnsiRenderer::stdout();
    let plan = PlanningWorkflowSessionState::default();
    let outcome = completed_turn();
    let ctrl_c = Arc::new(CtrlCState::new());
    let (goal_dir, goal_tracker) = active_goal_tracker("Ship it").await;
    goal_tracker.lock().await.set_token_budget(Some(100));
    goal_tracker.lock().await.record_token_observation(500);
    let events_path = goal_dir.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(events_path.clone()).unwrap();

    let flow = run_corrective_gate(CorrectiveGate {
        session_stats: &mut stats,
        runtime: &mut runtime,
        renderer: &mut renderer,
        tool_registry: &registry,
        plan_session: &plan,
        goal_tracker: &goal_tracker,
        harness_emitter: Some(&emitter),
        vt_cfg: &None,
        outcome_result: &outcome,
        ctrl_c_state: &ctrl_c,
        next_turn_input: "",
        turn_refused: false,
        refused_turn_rolled_back: false,
        session_end_reason: SessionEndReason::Completed,
    })
    .await
    .unwrap();

    assert!(matches!(flow, CorrectiveFlow::Proceed { .. }), "limited goal must idle");
    assert!(!runtime.has_pending_follow_up_inputs(), "limited goal must not queue");
    assert_eq!(goal_tracker.lock().await.status(), Some(vtcode_memory::GoalStatus::BudgetLimited));
    let log = std::fs::read_to_string(&events_path).unwrap();
    assert!(log.contains("budget_limited"), "limit must emit: {log}");
}

#[tokio::test]
async fn goal_budget_exhaustion_idles_without_queueing() {
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers as tracker_continue;

    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;
    let max_turns = tracker_continue::tracker_cross_turn_turns(None);
    let (_dir, goal_tracker) = active_goal_tracker("Ship it").await;
    for _ in 0..max_turns {
        assert!(goal_tracker.lock().await.claim_auto_continue_turn(max_turns).is_some());
    }
    assert_eq!(goal_tracker.lock().await.claim_auto_continue_turn(max_turns), None, "counter must cap at max");

    let mut stats = SessionStats::default();
    let mut runtime = AgentRuntime::new(AgentSessionState::new("test".into(), 4, 16_000), None, None);
    let mut renderer = AnsiRenderer::stdout();
    let plan = PlanningWorkflowSessionState::default();
    let outcome = completed_turn();
    let ctrl_c = Arc::new(CtrlCState::new());

    let flow = run_corrective_gate(CorrectiveGate {
        session_stats: &mut stats,
        runtime: &mut runtime,
        renderer: &mut renderer,
        tool_registry: &registry,
        plan_session: &plan,
        goal_tracker: &goal_tracker,
        harness_emitter: None,
        vt_cfg: &None,
        outcome_result: &outcome,
        ctrl_c_state: &ctrl_c,
        next_turn_input: "",
        turn_refused: false,
        refused_turn_rolled_back: false,
        session_end_reason: SessionEndReason::Completed,
    })
    .await
    .unwrap();

    assert!(matches!(flow, CorrectiveFlow::Proceed { .. }), "exhausted budget must idle");
    assert!(!runtime.has_pending_follow_up_inputs(), "exhausted budget must not queue");
    assert_eq!(goal_tracker.lock().await.auto_continue_turns(), max_turns);
}

#[test]
fn goal_claims_completion_matches_exact_first_line_only() {
    assert!(goal_claims_completion(Some("GOAL ACHIEVED: all widgets shipped")));
    assert!(goal_claims_completion(Some("\n\n  GOAL ACHIEVED: done\nmore detail")));
    assert!(!goal_claims_completion(None));
    assert!(!goal_claims_completion(Some("")));
    assert!(!goal_claims_completion(Some("goal achieved: done")), "case-sensitive");
    assert!(!goal_claims_completion(Some("Done.\nGOAL ACHIEVED: x")), "first line only");
    assert!(!goal_claims_completion(Some("GOAL ACHIEVED")), "colon required");
    assert!(!goal_claims_completion(Some("I will write GOAL ACHIEVED: later")), "prefix, not contains");
}

#[tokio::test]
async fn goal_completion_claim_completes_and_idles_without_queueing() {
    let workspace = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new_for_schema(workspace.path().to_path_buf()).await;
    let mut stats = SessionStats::default();
    let mut runtime = AgentRuntime::new(AgentSessionState::new("test".into(), 4, 16_000), None, None);
    Arc::make_mut(&mut runtime.state.messages)
        .push(vtcode_core::llm::provider::Message::assistant("GOAL ACHIEVED: all widgets shipped".into()));
    let mut renderer = AnsiRenderer::stdout();
    let plan = PlanningWorkflowSessionState::default();
    let outcome = completed_turn();
    let ctrl_c = Arc::new(CtrlCState::new());
    let (goal_dir, goal_tracker) = active_goal_tracker("Ship it").await;
    let events_path = goal_dir.path().join("events.jsonl");
    let emitter = HarnessEventEmitter::new(events_path.clone()).unwrap();

    let flow = run_corrective_gate(CorrectiveGate {
        session_stats: &mut stats,
        runtime: &mut runtime,
        renderer: &mut renderer,
        tool_registry: &registry,
        plan_session: &plan,
        goal_tracker: &goal_tracker,
        harness_emitter: Some(&emitter),
        vt_cfg: &None,
        outcome_result: &outcome,
        ctrl_c_state: &ctrl_c,
        next_turn_input: "",
        turn_refused: false,
        refused_turn_rolled_back: false,
        session_end_reason: SessionEndReason::Completed,
    })
    .await
    .unwrap();

    assert!(matches!(flow, CorrectiveFlow::Proceed { .. }), "completion must idle, not continue");
    assert!(!runtime.has_pending_follow_up_inputs(), "completion must not queue");
    assert_eq!(goal_tracker.lock().await.status(), Some(vtcode_memory::GoalStatus::Complete));
    let log = std::fs::read_to_string(&events_path).unwrap();
    assert!(log.contains("\"goal.updated\""), "completion must emit: {log}");
    assert!(log.contains("\"complete\""), "emitted state must be complete: {log}");
}
