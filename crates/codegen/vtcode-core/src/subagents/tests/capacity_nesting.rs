use super::*;

#[tokio::test]
async fn spawn_rejects_fourth_active_subagent() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    {
        let mut state = controller.state.write().await;
        for idx in 0..SUBAGENT_HARD_CONCURRENCY_LIMIT {
            let id = format!("active-{idx}");
            state.children.insert(
                id.clone(),
                ChildRecord {
                    id: id.clone(),
                    session_id: format!("session-{id}"),
                    parent_thread_id: "parent-session".to_string(),
                    spec: spec.clone(),
                    display_label: subagent_display_label(&spec),
                    status: SubagentStatus::Running,
                    background: false,
                    depth: 1,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    completed_at: None,
                    summary: None,
                    error: None,
                    archive_metadata: None,
                    archive_path: None,
                    transcript_path: None,
                    effective_config: None,
                    stored_messages: Vec::new(),
                    last_prompt: Some("Inspect the codebase.".to_string()),
                    queued_prompts: VecDeque::new(),
                    max_turns: None,
                    model_override: None,
                    reasoning_override: None,
                    thread_handle: None,
                    handle: None,
                    notify: Arc::new(Notify::new()),
                    worktree_path: None,
                    child_controller: None,
                },
            );
        }
    }

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect another codepath.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("fourth active subagent should be rejected");

    assert!(err.to_string().contains(&format!(
            "Subagent concurrency limit reached (max_concurrent={})",
            controller.config.vt_cfg.subagents.max_concurrent.min(
                SUBAGENT_HARD_CONCURRENCY_LIMIT
            )
        )));
}
#[tokio::test]
async fn simultaneous_spawns_reserve_capacity_before_launch_and_unwind_failure() {
    let temp = TempDir::new().unwrap();
    let controller = Arc::new(
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .unwrap(),
    );
    let parent_id = controller.parent_session_id.write().await;
    let barrier = Arc::new(tokio::sync::Barrier::new(13));
    let mut launches = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let controller = controller.clone();
        let barrier = barrier.clone();
        launches.spawn(async move {
            barrier.wait().await;
            controller
                .spawn(SpawnAgentRequest {
                    agent_type: Some("default".into()),
                    message: Some("Inspect Cargo.toml and summarize package names.".into()),
                    ..Default::default()
                })
                .await
        });
    }
    barrier.wait().await;
    for _ in 0..9 {
        let error = tokio::time::timeout(Duration::from_secs(3), launches.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("concurrency limit"), "{error:#}");
    }
    assert_eq!(controller.admission.available_permits(), 0);
    assert!(controller.state.read().await.children.is_empty());
    launches.abort_all();
    while launches.join_next().await.is_some() {}
    assert_eq!(controller.admission.available_permits(), 3);
    drop(parent_id);
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .unwrap();
    spec.isolation = Some(IsolationMode::Worktree);
    assert!(
        controller
            .spawn_custom(
                spec,
                SpawnAgentRequest {
                    message: Some("Inspect Cargo.toml and summarize package names.".into()),
                    ..Default::default()
                }
            )
            .await
            .is_err(),
        "non-Git workspace must reject worktree creation"
    );
    assert_eq!(controller.admission.available_permits(), 3);
    assert!(controller.state.read().await.children.is_empty());
    controller.signal_shutdown().await;
}
#[tokio::test]
async fn spawn_from_child_controller_respects_depth_limit() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.subagents.max_depth = 2;

    // Child controller runs at depth 1: it may spawn a grandchild (depth 2),
    // but the grandchild itself (depth 2) may not spawn further.
    let mut child_config = test_controller_config(temp.path().to_path_buf(), vt_cfg.clone());
    child_config.depth = 1;
    let child_controller = SubagentController::new(child_config).await.expect("child controller");

    child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("grandchild spawn should be allowed at depth 1 with max_depth=2");

    // Now a controller at depth 2 must refuse another spawn.
    let mut grandchild_config = test_controller_config(temp.path().to_path_buf(), vt_cfg);
    grandchild_config.depth = 2;
    let grandchild_controller = SubagentController::new(grandchild_config).await.expect("grandchild controller");

    let err = grandchild_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect yet more.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("spawn at depth == max_depth should hit the depth limit");

    assert!(err.to_string().contains("Subagent depth limit reached (max_depth=2)"));

    // Abort queued child tasks so they cannot run provider work after the
    // temp workspace is removed.
    child_controller.signal_shutdown().await;
    grandchild_controller.signal_shutdown().await;
}
#[tokio::test]
async fn nested_spawn_rejects_worktree_isolation() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.subagents.max_depth = 2;
    let mut child_config = test_controller_config(temp.path().to_path_buf(), vt_cfg);
    child_config.depth = 1;
    let child_controller = SubagentController::new(child_config).await.expect("child controller");
    child_controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    // Force the discovered "worker" spec to request worktree isolation so the
    // nested-spawn guard can be exercised.
    {
        let mut state = child_controller.state.write().await;
        if let Some(spec) = state.discovered.effective.iter_mut().find(|spec| spec.name == "worker") {
            spec.isolation = Some(IsolationMode::Worktree);
        }
    }

    let err = child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("nested worktree isolation should be rejected");

    assert!(err.to_string().contains("nested worktree isolation is not supported"));
}
#[tokio::test]
async fn close_cascades_to_child_scoped_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    // Child-scoped controller that already has a grandchild tracked under the
    // child's session id.
    let child_controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("child controller");
    let child_session_id = "child-session".to_string();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    {
        let mut state = child_controller.state.write().await;
        state.children.insert(
            "grandchild".to_string(),
            test_child_record(
                "grandchild",
                "session-grandchild",
                &child_session_id,
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let child_controller = Arc::new(child_controller);

    // Register the child on the parent controller with the child-scoped
    // controller attached, then close it and assert the grandchild is closed.
    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "child".to_string(),
            test_child_record(
                "child",
                &child_session_id,
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(child_controller.clone()),
            ),
        );
    }

    let closed = parent.close("child").await.expect("close child");
    assert!(closed.status.is_terminal());

    let grandchild_status = child_controller.status_for("grandchild").await.expect("grandchild status");
    assert_eq!(
        grandchild_status.status,
        SubagentStatus::Closed,
        "closing the child must cascade to its grandchildren"
    );
}
#[tokio::test]
async fn close_does_not_affect_sibling_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    // Two siblings each with their own child-scoped controller and a grandchild.
    let controller_a =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller a");
    {
        let mut state = controller_a.state.write().await;
        state.children.insert(
            "a-grandchild".to_string(),
            test_child_record(
                "a-grandchild",
                "session-a-grandchild",
                "session-a",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_b =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller b");
    {
        let mut state = controller_b.state.write().await;
        state.children.insert(
            "b-grandchild".to_string(),
            test_child_record(
                "b-grandchild",
                "session-b-grandchild",
                "session-b",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_a = Arc::new(controller_a);
    let controller_b = Arc::new(controller_b);

    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "a".to_string(),
            test_child_record(
                "a",
                "session-a",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_a.clone()),
            ),
        );
        state.children.insert(
            "b".to_string(),
            test_child_record(
                "b",
                "session-b",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_b.clone()),
            ),
        );
    }

    parent.close("a").await.expect("close a");

    let b_grandchild = controller_b.status_for("b-grandchild").await.expect("b-grandchild status");
    assert_eq!(
        b_grandchild.status,
        SubagentStatus::Running,
        "closing sibling 'a' must not affect sibling 'b''s grandchildren"
    );
    let a_grandchild = controller_a.status_for("a-grandchild").await.expect("a-grandchild status");
    assert_eq!(a_grandchild.status, SubagentStatus::Closed, "closing 'a' must cascade to its own grandchildren");
}
