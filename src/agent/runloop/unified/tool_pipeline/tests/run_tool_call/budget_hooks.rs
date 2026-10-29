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
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
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

    let call =
        vtcode_core::llm::provider::ToolCall::function("call_1".to_string(), "test_tool".to_string(), "{}".to_string());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
        .await
        .expect("run_tool_call must run");

    assert!(matches!(outcome.status, ToolExecutionStatus::Failure { .. }));
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
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
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
        "call_budget".to_string(),
        "read_file".to_string(),
        "{}".to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, false, None, 0, false)
        .await
        .expect("run_tool_call must run");

    println!("Outcome status: {:?}", outcome.status);

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("Policy violation"));
            assert!(error.to_string().contains("exceeded max tool calls per turn"));
        }
        other => panic!("Expected permission denial, got: {other:?}"),
    }
}
#[tokio::test]
async fn test_run_tool_call_hook_deny_blocks_before_safety() {
    use vtcode_core::config::{HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig};
    use vtcode_core::hooks::LifecycleHookEngine;

    let mut test_ctx = TestContext::new().await;
    let hook_marker = test_ctx.workspace.join("deny-hook-ran");
    let hooks_config = HooksConfig {
        lifecycle: LifecycleHooksConfig {
            pre_tool_use: vec![HookGroupConfig {
                matcher: Some("read_file".into()),
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    // stderr text turns exit 2 into an explicit deny decision.
                    command: format!("touch \"{}\"; echo denied >&2; exit 2", hook_marker.display()),
                    timeout_seconds: None,
                }],
            }],
            ..Default::default()
        },
    };
    let hooks = LifecycleHookEngine::new_with_session_gated(
        test_ctx.workspace.clone(),
        &hooks_config,
        vtcode_core::hooks::SessionStartTrigger::Startup,
        "test-session",
        false,
    )
    .expect("hook engine")
    .unwrap();
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

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_deny".to_string(),
        tools::READ_FILE.to_string(),
        json!({"path": "README.md"}).to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome =
        run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, Some(&hooks), true, None, 0, false)
            .await
            .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("Tool permission denied"), "got: {error}");
        }
        other => panic!("Expected hook deny failure, got: {other:?}"),
    }
    assert!(hook_marker.exists(), "hook must have run exactly once (deny path)");
}
#[tokio::test]
async fn test_run_tool_call_hook_rewrite_runs_hooks_exactly_once() {
    use vtcode_core::config::{HookCommandConfig, HookGroupConfig, HooksConfig, LifecycleHooksConfig};
    use vtcode_core::hooks::LifecycleHookEngine;

    let mut test_ctx = TestContext::new().await;
    // The hook rewrites the path argument and fails closed (exit 2) if it is
    // invoked a second time for the same call. A regression that re-runs the
    // hook phase after forwarding would therefore deny the call instead of
    // executing the rewritten arguments.
    let hook_invocations = test_ctx.workspace.join("rewrite-hook-invocations");
    let hook_script = format!(
        "f=\"{}\"; [ -f \"$f\" ] && exit 2; touch \"$f\"; printf '%s' '{{\"hookSpecificOutput\":{{\"hookEventName\":\"PreToolUse\",\"updatedInput\":{{\"path\":\"HOOK_REWROTE.md\"}}}}}}'",
        hook_invocations.display()
    );
    let hooks_config = HooksConfig {
        lifecycle: LifecycleHooksConfig {
            pre_tool_use: vec![HookGroupConfig {
                matcher: Some("read_file".into()),
                hooks: vec![HookCommandConfig {
                    kind: Default::default(),
                    command: hook_script,
                    timeout_seconds: None,
                }],
            }],
            ..Default::default()
        },
    };
    let hooks = LifecycleHookEngine::new_with_session_gated(
        test_ctx.workspace.clone(),
        &hooks_config,
        vtcode_core::hooks::SessionStartTrigger::Startup,
        "test-session",
        false,
    )
    .expect("hook engine")
    .unwrap();
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

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_rewrite".to_string(),
        tools::READ_FILE.to_string(),
        json!({"path": "README.md"}).to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    // The rewritten path must exist so execution confirms the rewrite reached
    // the tool.
    std::fs::write(test_ctx.workspace.join("HOOK_REWROTE.md"), "rewritten").expect("write rewritten file");

    let outcome =
        run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, Some(&hooks), true, None, 0, false)
            .await
            .expect("run_tool_call must run");

    assert!(hook_invocations.exists(), "hook must have run at least once");
    match outcome.status {
        ToolExecutionStatus::Success { output, .. } => {
            let output_text = output.to_string();
            assert!(output_text.contains("rewritten"), "expected rewritten file content, got: {output_text}");
        }
        other => panic!("Expected rewritten call to execute, got: {other:?}"),
    }
}
