use super::*;

#[tokio::test]
async fn test_handle_pipeline_output_collects_modified_files_and_records_stats() {
    if !stdin().is_terminal() {
        eprintln!("Skipping TUI-dependent test in non-interactive environment");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path().to_path_buf();

    let mut registry = ToolRegistry::new(workspace.clone()).await;
    let permission_cache_arc = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));

    let mut session = spawn_session_with_options(
        inline_theme_from_core_styles(&theme::active_styles()),
        SessionOptions {
            inline_rows: 10,
            workspace_root: Some(workspace.clone()),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let key = ToolCacheKey::new("read_file", "{}", "/tmp/foo.txt");
    {
        let mut c = cache.write().await;
        c.insert_arc(key.clone(), Arc::new("{}".to_string()));
        assert!(c.get(&key).is_some());
    }

    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = McpPanelState::new(10, true);
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let traj = TrajectoryLogger::new(&workspace);
    let tools = Arc::new(RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = RunLoopContext::new(
        &mut renderer,
        &handle,
        &mut registry,
        &tools,
        &cache,
        &permission_cache_arc,
        &permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"ok": true}),
        stdout: None,
        modified_files: vec!["/tmp/foo.txt".to_string()],
        command_success: true,
    });

    let (mod_files, _last_stdout) =
        handle_pipeline_output(&mut ctx, "read_file", &serde_json::json!({}), &outcome, None::<&VTCodeConfig>)
            .await
            .expect("handle should succeed");

    assert_eq!(mod_files, vec![PathBuf::from("/tmp/foo.txt")]);

    // Cache invalidation is handled in execution side-effects, not output rendering.
    {
        let c = cache.write().await;
        assert!(c.get(&key).is_some());
    }

    // Ensure session stats were updated
    let rec = session_stats.sorted_tools();
    assert!(rec.contains(&"read_file".to_string()));
}
#[tokio::test]
async fn task_tracker_updates_replace_previous_inline_block() {
    transcript::clear();

    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::default();

    let first = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "status": "updated",
            "view": {
                "title": "Respond to user greeting and assess next steps",
                "lines": [
                    {"display": "├ ✔ Greet user and summarize current workspace state"},
                    {"display": "├ > Ask what task they'd like to tackle"},
                    {"display": "└ • Offer to provide workspace tour if needed"}
                ]
            },
            "checklist": {
                "title": "Respond to user greeting and assess next steps",
                "total": 3,
                "completed": 1,
                "in_progress": 1,
                "pending": 1,
                "blocked": 0,
                "progress_percent": 33,
                "items": [
                    {"index": 1, "description": "Greet user and summarize current workspace state", "status": "completed"},
                    {"index": 2, "description": "Ask what task they'd like to tackle", "status": "in_progress"},
                    {"index": 3, "description": "Offer to provide workspace tour if needed", "status": "pending"}
                ]
            },
            "message": "Item 2 status changed: pending → in_progress"
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let second = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "status": "updated",
            "view": {
                "title": "Respond to user greeting and assess next steps",
                "lines": [
                    {"display": "├ ✔ Greet user and summarize current workspace state"},
                    {"display": "├ ✔ Ask what task they'd like to tackle"},
                    {"display": "└ • Offer to provide workspace tour if needed"}
                ]
            },
            "checklist": {
                "title": "Respond to user greeting and assess next steps",
                "total": 3,
                "completed": 2,
                "in_progress": 0,
                "pending": 1,
                "blocked": 0,
                "progress_percent": 67,
                "items": [
                    {"index": 1, "description": "Greet user and summarize current workspace state", "status": "completed"},
                    {"index": 2, "description": "Ask what task they'd like to tackle", "status": "completed"},
                    {"index": 3, "description": "Offer to provide workspace tour if needed", "status": "pending"}
                ]
            },
            "message": "Item 2 status changed: in_progress → completed"
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let args = serde_json::json!({"action": "update", "index": 2, "status": "in_progress"});
    let mut output_ctx = OutcomeContext {
        workspace_root: None,
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };

    process_outcome_common(&mut output_ctx, tools::TASK_TRACKER, &args, &first)
        .await
        .expect("first tracker render should succeed");

    let args = serde_json::json!({"action": "update", "index": 2, "status": "completed"});
    process_outcome_common(&mut output_ctx, tools::TASK_TRACKER, &args, &second)
        .await
        .expect("second tracker render should succeed");

    let mut saw_task_panel_update = false;
    while let Ok(command) = receiver.try_recv() {
        if matches!(command, InlineCommand::ShowTransient { .. }) {
            saw_task_panel_update = true;
        }
    }

    assert!(saw_task_panel_update, "expected tracker updates to refresh the dedicated task panel");
}
#[tokio::test]
async fn test_handle_pipeline_output_mcp_events() {
    if !stdin().is_terminal() {
        eprintln!("Skipping TUI-dependent test in non-interactive environment");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path().to_path_buf();

    let mut registry = ToolRegistry::new(workspace.clone()).await;
    let permission_cache_arc = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));

    let mut session = spawn_session_with_options(
        inline_theme_from_core_styles(&theme::active_styles()),
        SessionOptions {
            inline_rows: 10,
            workspace_root: Some(workspace.clone()),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = McpPanelState::new(10, true);
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let traj = TrajectoryLogger::new(&workspace);
    let tools = Arc::new(RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = RunLoopContext::new(
        &mut renderer,
        &handle,
        &mut registry,
        &tools,
        &cache,
        &permission_cache_arc,
        &permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: Some("ok".to_string()),
        modified_files: vec![],
        command_success: true,
    });

    let (_mod_files, _last_stdout) =
        handle_pipeline_output(&mut ctx, "mcp_example", &serde_json::json!({}), &outcome, None::<&VTCodeConfig>)
            .await
            .expect("handle should succeed");

    assert!(ctx.mcp_panel_state.event_count() > 0);
}
