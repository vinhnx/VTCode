use super::*;

#[tokio::test]
async fn test_run_tool_call_rejects_escalated_shell_when_hitl_disabled() {
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

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.security.human_in_the_loop = false;

    let args = serde_json::to_string(&json!({
        "action": "run",
        "command": "echo hi",
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to run this command without sandbox restrictions?"
    }))
    .expect("serialize command_session args");

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_command_session_escalated".to_string(),
        tools::EXEC_COMMAND.to_string(),
        args,
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome =
        run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, false, Some(&vt_cfg), 0, false)
            .await
            .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("Tool permission denied"));
        }
        other => panic!("Expected permission denial, got: {other:?}"),
    }
    assert_eq!(ctx.harness_state.tool_calls, 0);
}
#[tokio::test]
async fn test_run_tool_call_requires_operator_preapproval_for_escalated_shell() {
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

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.security.human_in_the_loop = false;
    vt_cfg.runtime_agent_permissions = Some(AgentPermissionsConfig::new(PermissionDefault::Allow));
    vt_cfg
        .commands
        .approval_prefixes
        .push("echo hi|sandbox_permissions=\"require_escalated\"|additional_permissions=null".to_string());
    ctx.tool_registry.apply_commands_config(&vt_cfg.commands);

    let args = serde_json::to_string(&json!({
        "action": "run",
        "command": "echo hi",
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to run this command without sandbox restrictions?"
    }))
    .expect("serialize command_session args");

    let call = vtcode_core::llm::provider::ToolCall::function(
        "call_command_session_escalated_saved_prefix".to_string(),
        tools::EXEC_COMMAND.to_string(),
        args,
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome =
        run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, false, Some(&vt_cfg), 0, false)
            .await
            .expect("run_tool_call must run");

    match outcome.status {
        ToolExecutionStatus::Failure { error } => {
            assert!(error.to_string().contains("requires an enforced operator approval decision"));
        }
        other => panic!("Expected operator preapproval denial, got: {other:?}"),
    }
}
#[tokio::test]
async fn test_run_tool_call_reuses_streamed_invocation_and_emits_one_terminal_observation() {
    let mut test_ctx = TestContext::new().await;
    std::fs::create_dir_all(&test_ctx.workspace).expect("create workspace");
    std::fs::write(test_ctx.workspace.join("note.txt"), "hello\n").expect("write note.txt");
    let mut registry = test_ctx.registry;

    let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));
    {
        let mut cache = permission_cache_arc.write().await;
        cache.cache_grant(tools::READ_FILE.to_string(), PermissionGrant::Permanent);
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

    let log_dir = tempfile::TempDir::new().expect("log dir");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(
        log_dir.path().join("harness.jsonl"),
    )
    .expect("harness emitter");

    let mut harness_state = build_harness_state();
    let tool_call_id = "call_streamed".to_string();
    let streamed_item_id = "streamed-tool-item".to_string();
    harness_state.remember_streamed_tool_call_items([(
        tool_call_id.clone(),
        crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem {
            item_id: streamed_item_id.clone(),
            tool_name: tools::READ_FILE.to_string(),
        },
    )]);
    emitter
        .emit(crate::agent::runloop::unified::inline_events::harness::tool_started_event(
            streamed_item_id.clone(),
            tools::READ_FILE,
            Some(&json!({"path":"note.txt"})),
            Some(tool_call_id.as_str()),
        ))
        .expect("emit tool started");

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
        Some(&emitter),
    );

    let call = vtcode_core::llm::provider::ToolCall::function(
        tool_call_id.clone(),
        tools::READ_FILE.to_string(),
        r#"{"path":"note.txt"}"#.to_string(),
    );
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, false)
        .await
        .expect("run_tool_call must run");

    assert!(matches!(outcome.status, ToolExecutionStatus::Success { .. }));
    assert!(ctx.harness_state.take_streamed_tool_call_item_id(&tool_call_id).is_none());

    let payload = std::fs::read_to_string(log_dir.path().join("harness.jsonl")).expect("read harness log");
    let mut started_count = 0usize;
    let mut completed_count = 0usize;
    let mut terminal_observation_count = 0usize;

    for line in payload.lines() {
        let value: Value = serde_json::from_str(line).expect("json line");
        let event = value.get("event").expect("event");
        let event_type = event.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();
        let item = event.get("item").expect("item");
        let item_id = item.get("id").and_then(|id| id.as_str()).unwrap_or_default();
        let item_type = item.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();

        if item_id == streamed_item_id && item_type == "tool_invocation" {
            if event_type == "item.started" {
                started_count += 1;
            }
            if event_type == "item.completed" {
                completed_count += 1;
            }
        }
        if event_type == "item.completed"
            && item_type == "harness"
            && item.get("event").and_then(Value::as_str) == Some("tool_latency_recorded")
        {
            terminal_observation_count += 1;
            assert_eq!(item.get("attempt").and_then(Value::as_u64), Some(1));
            assert!(item.get("duration_ms").and_then(Value::as_u64).is_some());
        }
    }

    assert_eq!(started_count, 1);
    assert_eq!(completed_count, 1);
    assert_eq!(terminal_observation_count, 1);
}
/// Regression: the interactive runloop validates (safety-admits) each tool
/// call through the shared SafetyGateway BEFORE invoking the pipeline with
/// prevalidated = true. The pipeline must not let the registry re-admit the
/// same call on that shared gateway (the old double admission halved the
/// effective per-turn budget at checkpoint turn_942/943). Pinned via the
/// short-window rate limiter: two calls admit twice, so a limit of 3
/// succeeds, while a double-admitting regression would record 4 and deny.
#[tokio::test]
async fn test_prevalidated_runloop_execution_admits_safety_once() {
    let mut test_ctx = TestContext::new().await;
    let mut registry = test_ctx.registry;
    // TestContext drops its TempDir (the workspace directory is deleted), so
    // re-create it before writing the fixture.
    std::fs::create_dir_all(&test_ctx.workspace).expect("recreate workspace");
    let read_path = test_ctx.workspace.join("budget.txt");
    std::fs::write(&read_path, "hello").expect("write fixture");

    let gateway = registry.safety_gateway();
    gateway.set_rate_limits(3, Some(3));
    gateway.start_turn();

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

    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    for index in 0..2 {
        let args = json!({ "path": read_path });

        // The runloop handlers safety-admit each call BEFORE the pipeline.
        let admission = gateway
            .check_and_record_with_id(
                &vtcode_core::tools::SafetyContext::new("runloop-safety-validator"),
                tools::READ_FILE,
                &args,
                Some(vtcode_core::tools::ToolInvocationId::new()),
            )
            .await;
        assert!(
            matches!(admission.decision, vtcode_core::tools::SafetyDecision::Allow),
            "admission {index} should be allowed: {:?}",
            admission.decision
        );

        let call = vtcode_core::llm::provider::ToolCall::function(
            format!("call_budget_{index}"),
            tools::READ_FILE.to_string(),
            serde_json::to_string(&args).expect("serialize args"),
        );
        let outcome = run_tool_call(&mut ctx, &call, &ctrl_c_state, &ctrl_c_notify, None, None, true, None, 0, true)
            .await
            .expect("run_tool_call must run");
        assert!(
            matches!(outcome.status, ToolExecutionStatus::Success { .. }),
            "prevalidated call {index} must succeed: {:?}",
            outcome.status
        );
    }

    // Each call recorded exactly once (the runloop admission). A third
    // admission still fits the window, but a fourth — which a
    // double-admitting regression would already have recorded — is denied.
    let args = json!({ "path": read_path });
    let third = gateway
        .check_and_record_with_id(
            &vtcode_core::tools::SafetyContext::new("runloop-safety-validator"),
            tools::READ_FILE,
            &args,
            Some(vtcode_core::tools::ToolInvocationId::new()),
        )
        .await;
    assert!(third.decision.is_allowed());
    let fourth = gateway
        .check_and_record_with_id(
            &vtcode_core::tools::SafetyContext::new("runloop-safety-validator"),
            tools::READ_FILE,
            &args,
            Some(vtcode_core::tools::ToolInvocationId::new()),
        )
        .await;
    assert!(fourth.decision.is_denied());
}
