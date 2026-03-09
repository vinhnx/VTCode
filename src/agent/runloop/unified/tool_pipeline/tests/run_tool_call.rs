use std::sync::Arc;

use serde_json::json;
use tokio::sync::Notify;
use vtcode_core::acp::PermissionGrant;
use vtcode_core::acp::permission_cache::ToolPermissionCache;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::tools::result_cache::ToolResultCache;

use super::*;

#[tokio::test]
async fn test_run_tool_call_unknown_tool_failure() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    {
        let mut cache = permission_cache_arc.write().await;
        cache.cache_grant("test_tool".to_string(), PermissionGrant::Permanent);
    }

    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_1".to_string(),
        "test_tool".to_string(),
        "{}".to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        false,
    )
    .await
    .expect("run_tool_call must run");

    assert!(matches!(
        outcome.status,
        ToolExecutionStatus::Failure { .. }
    ));
}

#[tokio::test]
async fn test_run_tool_call_respects_max_tool_calls_budget() {
    let mut test_ctx = TestContext::new().await;
    test_ctx.session.set_skip_confirmations(false);
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state_with(1);
    harness_state.record_tool_call(); // Exhaust the budget (1/1)
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_budget".to_string(),
        "read_file".to_string(),
        "{}".to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        false,
        None,
        0,
        false,
    )
    .await
    .expect("run_tool_call must run");

    println!("Outcome status: {:?}", outcome.status);

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("Policy violation"));
            assert!(
                error
                    .to_string()
                    .contains("exceeded max tool calls per turn")
            );
        }
        other => panic!("Expected permission denial, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_run_tool_call_allows_unlimited_budget_when_disabled() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state_with(0);
    for _ in 0..4 {
        harness_state.record_tool_call();
    }
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
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

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        false,
        None,
        0,
        false,
    )
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
async fn test_run_tool_call_prevalidated_blocks_mutation_in_plan_mode() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    {
        let mut cache = permission_cache_arc.write().await;
        cache.cache_grant(tools::WRITE_FILE.to_string(), PermissionGrant::Permanent);
    }

    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    registry.enable_plan_mode();
    registry.plan_mode_state().enable();
    session_stats.set_plan_mode(true);

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let payload = serde_json::to_string(&json!({
        "path": "notes.txt",
        "content": "hello plan mode"
    }))
    .expect("serialize tool args");
    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_plan_write".to_string(),
        tools::WRITE_FILE.to_string(),
        payload,
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        true,
    )
    .await
    .expect("run_tool_call must run");

    println!("Plan-mode guard test outcome status: {:?}", outcome.status);

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("plan mode"));
        }
        other => panic!("Expected plan mode failure, got: {:?}", other),
    }
    assert!(session_stats.is_plan_mode());
    assert!(registry.is_plan_mode());
    assert!(registry.plan_mode_state().is_active());
}

#[tokio::test]
async fn test_run_tool_call_prevalidated_allows_task_tracker_in_plan_mode() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    registry.enable_plan_mode();
    registry.plan_mode_state().enable();
    session_stats.set_plan_mode(true);

    let plans_dir = test_ctx.workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_file = plans_dir.join("tracker-test-task-tracker.md");
    std::fs::write(&plan_file, "# Tracker Test\n").expect("write plan file");
    registry
        .plan_mode_state()
        .set_plan_file(Some(plan_file))
        .await;

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_plan_task_tracker".to_string(),
        tools::TASK_TRACKER.to_string(),
        r#"{"action":"list"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        true,
    )
    .await
    .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            assert!(
                output["status"] == "ok" || output["status"] == "empty",
                "unexpected status: {}",
                output["status"]
            );
        }
        other => panic!(
            "Expected task_tracker success in plan mode, got: {:?}",
            other
        ),
    }
}

#[tokio::test]
async fn test_run_tool_call_non_prevalidated_allows_task_tracker_in_plan_mode_and_tracks_budget() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    registry.enable_plan_mode();
    registry.plan_mode_state().enable();
    session_stats.set_plan_mode(true);

    let plans_dir = test_ctx.workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_file = plans_dir.join("tracker-test-task-tracker-non-prevalidated.md");
    std::fs::write(&plan_file, "# Tracker Test\n").expect("write plan file");
    registry
        .plan_mode_state()
        .set_plan_file(Some(plan_file))
        .await;

    let mut harness_state = build_harness_state_with(2);
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_plan_task_tracker_non_prevalidated".to_string(),
        tools::TASK_TRACKER.to_string(),
        r#"{"action":"list"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        false,
    )
    .await
    .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            assert!(
                output["status"] == "ok" || output["status"] == "empty",
                "unexpected status: {}",
                output["status"]
            );
        }
        other => panic!(
            "Expected task_tracker success in plan mode, got: {:?}",
            other
        ),
    }

    assert_eq!(ctx.harness_state.tool_calls, 1);
}

#[tokio::test]
async fn test_run_tool_call_prevalidated_allows_plan_task_tracker_in_plan_mode() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    registry.enable_plan_mode();
    registry.plan_mode_state().enable();
    session_stats.set_plan_mode(true);

    let plans_dir = test_ctx.workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_file = plans_dir.join("tracker-test.md");
    std::fs::write(&plan_file, "# Tracker Test\n").expect("write plan file");
    registry
        .plan_mode_state()
        .set_plan_file(Some(plan_file))
        .await;

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_plan_task_tracker_allowed".to_string(),
        tools::PLAN_TASK_TRACKER.to_string(),
        r#"{"action":"create","items":["Define guard","  Verify guard"]}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        true,
    )
    .await
    .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            assert_eq!(output["status"], "created");
            assert_eq!(output["checklist"]["total"], 2);
        }
        other => panic!("Expected success, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_run_tool_call_non_prevalidated_blocks_plan_task_tracker_outside_plan_mode_without_budget_use()
 {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state_with(2);
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_plan_task_tracker_blocked".to_string(),
        tools::PLAN_TASK_TRACKER.to_string(),
        r#"{"action":"list"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        false,
    )
    .await
    .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("plan_task_tracker"));
            assert!(error.to_string().contains("compatibility alias"));
        }
        other => panic!("Expected plan mode failure, got: {:?}", other),
    }

    assert_eq!(ctx.harness_state.tool_calls, 0);
}

#[tokio::test]
async fn test_run_tool_call_invalid_preflight_does_not_consume_budget() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state_with(1);
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_invalid_preflight".to_string(),
        tools::READ_FILE.to_string(),
        r#"{"path":"/var/db/shadow"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let first_outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        false,
        None,
        0,
        false,
    )
    .await
    .expect("first run_tool_call must run");

    assert!(matches!(
        first_outcome.status,
        ToolExecutionStatus::Failure { .. }
    ));
    assert_eq!(ctx.harness_state.tool_calls, 0);

    let second_outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        false,
        None,
        0,
        false,
    )
    .await
    .expect("second run_tool_call must run");

    assert!(matches!(
        second_outcome.status,
        ToolExecutionStatus::Failure { .. }
    ));
    assert_eq!(ctx.harness_state.tool_calls, 0);
}

#[tokio::test]
async fn test_run_tool_call_unified_exec_git_diff_uses_cache_on_repeat() {
    let mut test_ctx = TestContext::new().await;
    std::fs::create_dir_all(&test_ctx.workspace).expect("create workspace directory");
    std::fs::write(test_ctx.workspace.join("a.txt"), "same-content\n").expect("write a.txt");
    std::fs::write(test_ctx.workspace.join("b.txt"), "same-content\n").expect("write b.txt");

    let mut registry = test_ctx.registry;
    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    {
        let mut cache = permission_cache_arc.write().await;
        cache.cache_grant(tools::UNIFIED_EXEC.to_string(), PermissionGrant::Permanent);
    }

    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(32)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let args = serde_json::to_string(&json!({
        "action": "run",
        "command": "git diff --no-index ./a.txt ./b.txt"
    }))
    .expect("serialize unified_exec args");

    let first_call = vtcode_core::llm::provider::ToolCall::function(
        "call_unified_exec_1".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        args.clone(),
    );
    let second_call = vtcode_core::llm::provider::ToolCall::function(
        "call_unified_exec_2".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        args,
    );

    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let first_outcome = run_tool_call(
        &mut ctx,
        &first_call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        false,
    )
    .await
    .expect("first unified_exec call must run");

    let second_outcome = run_tool_call(
        &mut ctx,
        &second_call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        true,
        None,
        0,
        false,
    )
    .await
    .expect("second unified_exec call must run");

    let extract_session_id = |status: &ToolExecutionStatus| -> String {
        match status {
            ToolExecutionStatus::Success {
                output,
                command_success,
                ..
            } => {
                assert!(*command_success);
                output
                    .get("session_id")
                    .or_else(|| output.get("id"))
                    .and_then(|value| value.as_str())
                    .expect("command output should include session id")
                    .to_string()
            }
            other => panic!("Expected success status, got: {:?}", other),
        }
    };

    let first_session_id = extract_session_id(&first_outcome.status);
    let second_session_id = extract_session_id(&second_outcome.status);
    assert_eq!(first_session_id, second_session_id);

    let first_output = match &first_outcome.status {
        ToolExecutionStatus::Success { output, .. } => output,
        _ => unreachable!(),
    };
    let second_output = match &second_outcome.status {
        ToolExecutionStatus::Success { output, .. } => output,
        _ => unreachable!(),
    };

    let mut first_stable = first_output.clone();
    let mut second_stable = second_output.clone();
    let first_wall_time = first_stable
        .get("wall_time")
        .and_then(|value| value.as_f64())
        .expect("first output should include wall_time");
    let second_wall_time = second_stable
        .get("wall_time")
        .and_then(|value| value.as_f64())
        .expect("second output should include wall_time");
    assert!(first_wall_time >= 0.0);
    assert!(second_wall_time >= 0.0);
    first_stable
        .as_object_mut()
        .map(|object| object.remove("wall_time"));
    second_stable
        .as_object_mut()
        .map(|object| object.remove("wall_time"));
    assert_eq!(first_stable, second_stable);
}

#[tokio::test]
async fn test_run_tool_call_rejects_escalated_shell_when_hitl_disabled() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.security.human_in_the_loop = false;

    let args = serde_json::to_string(&json!({
        "action": "run",
        "command": "echo hi",
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to run this command without sandbox restrictions?"
    }))
    .expect("serialize unified_exec args");

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_unified_exec_escalated".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        args,
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        false,
        Some(&vt_cfg),
        0,
        false,
    )
    .await
    .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("Tool permission denied"));
        }
        other => panic!("Expected permission denial, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_run_tool_call_allows_escalated_shell_with_saved_prefix_rule() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(10)));
    let decision_ledger = Arc::new(tokio::sync::RwLock::new(DecisionTracker::new()));
    let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
    let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
    let approval_recorder = test_ctx.approval_recorder;
    let traj = TrajectoryLogger::new(&test_ctx.workspace);
    let tools = Arc::new(tokio::sync::RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
        &mut test_ctx.renderer,
        &test_ctx.handle,
        &mut registry,
        &tools,
        &result_cache,
        &permission_cache_arc,
        &decision_ledger,
        &mut session_stats,
        &mut mcp_panel,
        &approval_recorder,
        &mut test_ctx.session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.security.human_in_the_loop = false;
    vt_cfg.commands.approval_prefixes.push(
        "echo hi|sandbox_permissions=\"require_escalated\"|additional_permissions=null".to_string(),
    );
    ctx.tool_registry.apply_commands_config(&vt_cfg.commands);

    let args = serde_json::to_string(&json!({
        "action": "run",
        "command": "echo hi",
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to run this command without sandbox restrictions?"
    }))
    .expect("serialize unified_exec args");

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_unified_exec_escalated_saved_prefix".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        args,
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(
        &mut ctx,
        &call,
        &ctrl_c_state,
        &ctrl_c_notify,
        None,
        None,
        false,
        Some(&vt_cfg),
        0,
        false,
    )
    .await
    .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Success { .. } => {}
        other => panic!(
            "Expected saved prefix approval to allow execution, got: {:?}",
            other
        ),
    }
}
