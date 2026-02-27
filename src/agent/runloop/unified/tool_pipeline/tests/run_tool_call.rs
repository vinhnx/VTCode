use std::sync::Arc;

use serde_json::json;
use tokio::sync::Notify;
use vtcode_core::acp::PermissionGrant;
use vtcode_core::acp::permission_cache::ToolPermissionCache;
use vtcode_core::config::constants::tools;
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
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
    session_stats.switch_to_planner();

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
async fn test_run_tool_call_prevalidated_blocks_task_tracker_in_plan_mode() {
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
    session_stats.switch_to_planner();

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
        ToolExecutionStatus::Failure { error } => {
            let error_text = error.to_string();
            assert!(
                error_text.contains("task_tracker")
                    || error_text.contains("tool denied by plan mode"),
                "unexpected error text: {error_text}"
            );
            assert!(
                error_text.contains("not allowed in Plan mode")
                    || error_text.contains("tool denied by plan mode"),
                "unexpected error text: {error_text}"
            );
        }
        other => panic!("Expected plan mode failure, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_run_tool_call_non_prevalidated_blocks_task_tracker_in_plan_mode_without_budget_use() {
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
    session_stats.switch_to_planner();

    let mut harness_state = build_harness_state_with(2);
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
        ToolExecutionStatus::Failure { error } => {
            let error_text = error.to_string();
            assert!(
                error_text.contains("task_tracker")
                    || error_text.contains("tool denied by plan mode"),
                "unexpected error text: {error_text}"
            );
            assert!(
                error_text.contains("not allowed in Plan mode")
                    || error_text.contains("tool denied by plan mode"),
                "unexpected error text: {error_text}"
            );
        }
        other => panic!("Expected plan mode failure, got: {:?}", other),
    }

    assert_eq!(ctx.harness_state.tool_calls, 0);
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
    session_stats.switch_to_planner();

    let plans_dir = test_ctx.workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_file = plans_dir.join("tracker-test.md");
    std::fs::write(&plan_file, "# Tracker Test\n").expect("write plan file");
    registry
        .plan_mode_state()
        .set_plan_file(Some(plan_file))
        .await;

    let mut harness_state = build_harness_state();
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
            assert!(error.to_string().contains("only available"));
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
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
    let mut ctx = crate::agent::runloop::unified::run_loop_context::RunLoopContext {
        renderer: &mut test_ctx.renderer,
        handle: &test_ctx.handle,
        tool_registry: &mut registry,
        tools: &tools,
        tool_result_cache: &result_cache,
        tool_permission_cache: &permission_cache_arc,
        decision_ledger: &decision_ledger,
        session_stats: &mut session_stats,
        mcp_panel_state: &mut mcp_panel,
        approval_recorder: &approval_recorder,
        session: &mut test_ctx.session,
        safety_validator: None,
        traj: &traj,
        harness_state: &mut harness_state,
        harness_emitter: None,
    };

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
    assert_eq!(first_output, second_output);
}
