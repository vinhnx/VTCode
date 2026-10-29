use super::*;

#[tokio::test]
async fn test_run_tool_call_invalid_preflight_does_not_consume_budget() {
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

    let mut harness_state = build_harness_state_with(1);
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
        "call_invalid_preflight".to_string(),
        tools::READ_FILE.to_string(),
        r#"{"path":"/var/db/shadow"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let first_outcome =
        run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, false, None, 0, false)
            .await
            .expect("first run_tool_call must run");

    assert!(matches!(first_outcome.status, ToolExecutionStatus::Failure { .. }));
    assert_eq!(ctx.harness_state.tool_calls, 0);

    let second_outcome =
        run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, false, None, 0, false)
            .await
            .expect("second run_tool_call must run");

    assert!(matches!(second_outcome.status, ToolExecutionStatus::Failure { .. }));
    assert_eq!(ctx.harness_state.tool_calls, 0);
}
#[tokio::test]
async fn test_run_tool_call_command_session_git_diff_uses_cache_on_repeat() {
    let mut test_ctx = TestContext::new().await;
    std::fs::create_dir_all(&test_ctx.workspace).expect("create workspace directory");
    std::fs::write(test_ctx.workspace.join("a.txt"), "same-content\n").expect("write a.txt");
    std::fs::write(test_ctx.workspace.join("b.txt"), "same-content\n").expect("write b.txt");

    let mut registry = test_ctx.registry;
    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    {
        let mut cache = permission_cache_arc.write().await;
        cache.cache_grant(tools::EXEC_COMMAND.to_string(), PermissionGrant::Permanent);
    }

    let result_cache = Arc::new(tokio::sync::RwLock::new(ToolResultCache::new(32)));
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

    let args = serde_json::to_string(&json!({
        "action": "run",
        "command": "git diff --no-index ./a.txt ./b.txt"
    }))
    .expect("serialize command_session args");

    let first_call = vtcode_core::llm::provider::ToolCall::function(
        "call_command_session_1".to_string(),
        tools::EXEC_COMMAND.to_string(),
        args.clone(),
    );
    let second_call = vtcode_core::llm::provider::ToolCall::function(
        "call_command_session_2".to_string(),
        tools::EXEC_COMMAND.to_string(),
        args,
    );

    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let first_outcome =
        run_tool_call(&mut ctx, &first_call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
            .await
            .expect("first command_session call must run");

    let second_outcome =
        run_tool_call(&mut ctx, &second_call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
            .await
            .expect("second command_session call must run");

    let extract_session_id = |status: &ToolExecutionStatus| -> String {
        match status {
            ToolExecutionStatus::Success { output, command_success, .. } => {
                assert!(*command_success);
                output
                    .get("session_id")
                    .or_else(|| output.get("id"))
                    .and_then(|value| value.as_str())
                    .expect("command output should include session id")
                    .to_string()
            }
            other => panic!("Expected success status, got: {other:?}"),
        }
    };

    let first_session_id = extract_session_id(&first_outcome.status);
    let second_session_id = extract_session_id(&second_outcome.status);
    assert_eq!(first_session_id, second_session_id);

    let first_output = match &first_outcome.status {
        ToolExecutionStatus::Success { output, .. } => output,
        other => panic!("expected Success, got: {other:?}"),
    };
    let second_output = match &second_outcome.status {
        ToolExecutionStatus::Success { output, .. } => output,
        other => panic!("expected Success, got: {other:?}"),
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
    first_stable.as_object_mut().map(|object| object.remove("wall_time"));
    second_stable.as_object_mut().map(|object| object.remove("wall_time"));
    assert_eq!(first_stable, second_stable);
}
#[tokio::test]
async fn successful_apply_patch_invalidates_cached_code_search() {
    let mut test_ctx = TestContext::new().await;
    let source_dir = test_ctx.workspace.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::write(source_dir.join("widget.rs"), "pub struct Widget;\n").expect("write search fixture");

    let mut registry = test_ctx.registry;
    registry.allow_all_tools().await.expect("allow test tools");
    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    {
        let mut cache = permission_cache_arc.write().await;
        cache.cache_grant(tools::CODE_SEARCH.to_string(), PermissionGrant::Permanent);
        cache.cache_grant(tools::APPLY_PATCH.to_string(), PermissionGrant::Permanent);
        cache.cache_grant(tools::EXEC_COMMAND.to_string(), PermissionGrant::Permanent);
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
    let mut harness_state = build_harness_state_with(8);
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
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let search_args = json!({
        "query": "Widget",
        "path": "src",
        "file_types": ["rust"],
        "result_types": ["text"]
    });
    let search_call = |id: &str| {
        vtcode_core::llm::provider::ToolCall::function(
            id.to_string(),
            tools::CODE_SEARCH.to_string(),
            search_args.to_string(),
        )
    };

    let first = run_tool_call(
        &mut ctx,
        &search_call("search_before_patch"),
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
    .expect("initial code_search should run");
    match first.status {
        ToolExecutionStatus::Success { output, .. } => {
            assert_eq!(output["returned"], json!(1));
        }
        other => panic!("expected initial search success, got: {other:?}"),
    }
    assert_eq!(result_cache.read().await.stats().current_size, 1);

    let failed_patch = vtcode_core::llm::provider::ToolCall::function(
        "failed_patch".to_string(),
        tools::APPLY_PATCH.to_string(),
        json!({"input": "*** Begin Patch\n*** Not An Operation\n*** End Patch\n"}).to_string(),
    );
    let failed =
        run_tool_call(&mut ctx, &failed_patch, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
            .await
            .expect("failed apply_patch should produce a pipeline outcome");
    assert!(matches!(failed.status, ToolExecutionStatus::Failure { .. }));
    assert_eq!(result_cache.read().await.stats().current_size, 1, "a failed patch must preserve cached reads");

    let cached = run_tool_call(
        &mut ctx,
        &search_call("search_after_failed_patch"),
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
    .expect("search after failed patch should run");
    assert!(matches!(
        cached.status,
        ToolExecutionStatus::Success { ref output, .. } if output["returned"] == json!(1)
    ));
    assert_eq!(result_cache.read().await.stats().hits, 1);

    let patch = "*** Begin Patch\n*** Update File: src/widget.rs\n@@\n-pub struct Widget;\n+pub struct Gadget;\n*** End Patch\n";
    let successful_patch = vtcode_core::llm::provider::ToolCall::function(
        "successful_patch".to_string(),
        tools::APPLY_PATCH.to_string(),
        json!({"input": patch}).to_string(),
    );
    let patched =
        run_tool_call(&mut ctx, &successful_patch, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
            .await
            .expect("successful apply_patch should run");
    match patched.status {
        ToolExecutionStatus::Success { modified_files, .. } => {
            assert_eq!(
                modified_files,
                vec![
                    canonicalize(source_dir.join("widget.rs"))
                        .expect("canonical widget.rs")
                        .to_string_lossy()
                ]
            );
        }
        other => panic!("expected patch success, got: {other:?}"),
    }
    assert_eq!(
        result_cache.read().await.stats().current_size,
        0,
        "successful patch metadata must invalidate the scoped search cache"
    );

    let fresh = run_tool_call(
        &mut ctx,
        &search_call("search_after_successful_patch"),
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
    .expect("search after successful patch should run");
    match fresh.status {
        ToolExecutionStatus::Success { output, .. } => {
            assert_eq!(output["returned"], json!(0));
        }
        other => panic!("expected fresh search success, got: {other:?}"),
    }
    assert_eq!(
        result_cache.read().await.stats().hits,
        1,
        "the post-success search must execute freshly instead of hitting stale cache"
    );

    let shell_call = vtcode_core::llm::provider::ToolCall::function(
        "successful_shell_mutation".to_string(),
        tools::EXEC_COMMAND.to_string(),
        json!({
            "action": "run",
            "command": "printf marker > shell-marker.txt"
        })
        .to_string(),
    );
    let shell = run_tool_call(&mut ctx, &shell_call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
        .await
        .expect("shell mutation should run");
    assert!(matches!(shell.status, ToolExecutionStatus::Success { .. }));
    assert_eq!(
        result_cache.read().await.stats().current_size,
        0,
        "a shell command must invalidate cached filesystem results"
    );
}
