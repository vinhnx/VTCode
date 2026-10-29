use super::*;

#[tokio::test]
async fn test_run_tool_call_allows_unlimited_budget_when_disabled() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tool_defs = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state_with(0);
    for _ in 0..4 {
        harness_state.record_tool_call();
    }
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tool_defs,
        &result_cache,
        &permission_cache_arc,
        &test_ctx.permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_unlimited".to_string(),
        "read_file".to_string(),
        "{}".to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, false, None, 0, false)
        .await
        .expect("run_tool_call must run");

    assert!(!matches!(
        outcome.status,
        ToolExecutionStatus::Failure { ref error }
            if error
                .to_string()
                .contains("exceeded max tool calls per turn")
    ));
}
#[tokio::test]
async fn test_run_tool_call_forwards_runtime_agent_permissions_to_routing() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &test_ctx.permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );
    let vt_cfg = VTCodeConfig {
        runtime_agent_permissions: Some(agent_permissions),
        ..Default::default()
    };

    let args = serde_json::to_string(&json!({
        "action": "write",
        "path": "notes.md",
        "content": "hello"
    }))
    .expect("serialize file_operation args");
    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_active_agent_deny".to_string(),
        tools::WRITE_FILE.to_string(),
        args,
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome =
        run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, true, Some(&vt_cfg), 0, false)
            .await
            .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("Tool permission denied"));
        }
        other => panic!("Expected permission denial, got: {other:?}"),
    }
    assert!(!test_ctx.workspace.join("notes.md").exists());
}
#[tokio::test]
async fn test_run_tool_call_prevalidated_blocks_mutation_in_planning_workflow() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    {
        let mut cache = permission_cache_arc.write().await;
        cache.cache_grant(tools::APPLY_PATCH.to_string(), PermissionGrant::Permanent);
    }

    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tool_defs = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    registry.enable_planning();
    registry.planning_workflow_state().enable();
    plan_session.enter(vtcode_core::core::interfaces::session::PlanningEntrySource::UserRequest);

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tool_defs,
        &result_cache,
        &permission_cache_arc,
        &test_ctx.permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let payload = serde_json::to_string(&json!({
        "input": "*** Begin Patch\n*** Add File: notes.txt\n+hello planning workflow\n*** End Patch\n"
    }))
    .expect("serialize tool args");
    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_plan_patch".to_string(),
        tools::APPLY_PATCH.to_string(),
        payload,
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, true)
        .await
        .expect("run_tool_call must run");

    println!("Planning workflow guard test outcome status: {:?}", outcome.status);

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("planning workflow"));
        }
        other => panic!("Expected planning workflow failure, got: {other:?}"),
    }
    assert!(!test_ctx.workspace.join("notes.txt").exists());
    assert!(registry.is_planning_active());
    assert!(registry.planning_workflow_state().is_active());
}
#[tokio::test]
async fn test_run_tool_call_prevalidated_allows_task_tracker_in_planning_workflow() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tool_defs = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    registry.enable_planning();
    registry.planning_workflow_state().enable();
    plan_session.enter(vtcode_core::core::interfaces::session::PlanningEntrySource::UserRequest);

    let plans_dir = test_ctx.workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_file = plans_dir.join("tracker-test-task-tracker.md");
    std::fs::write(&plan_file, "# Tracker Test\n").expect("write plan file");
    registry.planning_workflow_state().set_plan_file(Some(plan_file)).await;

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tool_defs,
        &result_cache,
        &permission_cache_arc,
        &test_ctx.permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_task_tracker".to_string(),
        tools::TASK_TRACKER.to_string(),
        r#"{"action":"list"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, true)
        .await
        .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            assert!(output["status"] == "ok" || output["status"] == "empty", "unexpected status: {}", output["status"]);
        }
        other => panic!("Expected task_tracker success in planning workflow, got: {other:?}"),
    }
}
#[tokio::test]
async fn test_run_tool_call_non_prevalidated_allows_task_tracker_in_planning_workflow_and_tracks_budget() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tool_defs = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    registry.enable_planning();
    registry.planning_workflow_state().enable();
    plan_session.enter(vtcode_core::core::interfaces::session::PlanningEntrySource::UserRequest);

    let plans_dir = test_ctx.workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_file = plans_dir.join("tracker-test-task-tracker-non-prevalidated.md");
    std::fs::write(&plan_file, "# Tracker Test\n").expect("write plan file");
    registry.planning_workflow_state().set_plan_file(Some(plan_file)).await;

    let mut harness_state = build_harness_state_with(2);
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tool_defs,
        &result_cache,
        &permission_cache_arc,
        &test_ctx.permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_task_tracker_non_prevalidated".to_string(),
        tools::TASK_TRACKER.to_string(),
        r#"{"action":"list"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
        .await
        .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            assert!(output["status"] == "ok" || output["status"] == "empty", "unexpected status: {}", output["status"]);
        }
        other => panic!("Expected task_tracker success in planning workflow, got: {other:?}"),
    }

    assert_eq!(ctx.harness_state.tool_calls, 1);
}
