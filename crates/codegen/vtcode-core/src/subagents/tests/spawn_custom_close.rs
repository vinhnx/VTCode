use super::*;

#[tokio::test]
async fn resume_preserves_captured_runtime_overrides() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            model: Some(models::openai::GPT_5_6_LUNA.to_string()),
            max_turns: Some(2),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let initial_model = wait_for_effective_model(&controller, &spawned.id)
        .await
        .expect("initial effective model");
    assert_eq!(initial_model, models::openai::GPT_5_6_LUNA);

    let closed = controller.close(&spawned.id).await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);

    controller.resume(&spawned.id).await.expect("resume");

    for _ in 0..100 {
        let status = controller.status_for(&spawned.id).await.expect("status");
        if status.updated_at > closed.updated_at && status.status != SubagentStatus::Closed {
            let snapshot = controller.snapshot_for_thread(&spawned.id).await.expect("snapshot");
            assert_eq!(snapshot.effective_config.agent.default_model, models::openai::GPT_5_6_LUNA);
            controller.close(&spawned.id).await.expect("final close");
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("resumed subagent did not capture runtime config in time");
}
#[tokio::test]
async fn spawn_captures_runtime_config_before_first_child_turn() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let snapshot = controller.snapshot_for_thread(&spawned.id).await.expect("snapshot");

    assert_eq!(snapshot.id, spawned.id);
    assert!(!snapshot.effective_config.agent.default_model.trim().is_empty());

    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_custom_uses_explicit_spec_without_delegation_hints() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    spec.name = "init-grounding-explorer".to_string();
    spec.description = "VT Code /init grounding explorer.".to_string();
    spec.source = SubagentSource::ProjectVtcode;

    let spawned = controller
        .spawn_custom(
            spec,
            SpawnAgentRequest {
                message: Some("Inspect the repository and report agent-facing findings.".to_string()),
                max_turns: Some(2),
                ..SpawnAgentRequest::default()
            },
        )
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "init-grounding-explorer");
    assert_eq!(spawned.source, SubagentSource::ProjectVtcode.label());
    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_custom_rejects_write_capable_spec() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let err = controller
        .spawn_custom(
            spec,
            SpawnAgentRequest {
                message: Some("Implement a change.".to_string()),
                ..SpawnAgentRequest::default()
            },
        )
        .await
        .expect_err("write-capable custom spec should be rejected");

    assert!(err.to_string().contains("custom subagent spawn only supports read-only specs"));
}
#[tokio::test]
async fn spawn_custom_rejects_primary_only_spec() {
    let temp = TempDir::new().expect("tempdir");
    write_test_primary_agent(temp.path());
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let spec = controller
        .effective_specs()
        .await
        .into_iter()
        .find(|spec| spec.name == "duck")
        .expect("duck primary agent");

    let err = controller
        .spawn_custom(
            spec,
            SpawnAgentRequest {
                message: Some("Discuss the task.".to_string()),
                ..SpawnAgentRequest::default()
            },
        )
        .await
        .expect_err("primary-only custom spec should be rejected");

    assert!(
        err.to_string()
            .contains("custom subagent spawn only supports subagent-capable specs")
    );
}
#[tokio::test]
async fn close_marks_child_closed() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    controller.set_turn_delegation_hints_from_input("delegate this task").await;
    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("default".to_string()),
            message: Some("Summarize the repository.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");
    let closed = controller.close(&spawned.id).await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);
}
#[tokio::test]
async fn close_is_idempotent_for_closed_agents() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    controller.set_turn_delegation_hints_from_input("delegate this task").await;
    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("default".to_string()),
            message: Some("Summarize the repository.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let closed = controller.close(&spawned.id).await.expect("first close");
    let closed_again = controller.close(&spawned.id).await.expect("second close");

    assert_eq!(closed_again.status, SubagentStatus::Closed);
    assert_eq!(closed_again.updated_at, closed.updated_at);
    assert_eq!(closed_again.completed_at, closed.completed_at);
}
#[tokio::test]
async fn close_and_resume_cascade_through_spawn_tree() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    {
        let mut state = controller.state.write().await;
        state.children.insert(
            "parent".to_string(),
            test_child_record("parent", "session-parent", "session-root", &spec, SubagentStatus::Running, 1, None),
        );
        state.children.insert(
            "child".to_string(),
            test_child_record("child", "session-child", "parent", &spec, SubagentStatus::Running, 2, None),
        );
        state.children.insert(
            "grandchild".to_string(),
            test_child_record("grandchild", "session-grandchild", "child", &spec, SubagentStatus::Running, 3, None),
        );
    }

    let closed = controller.close("parent").await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);
    assert_eq!(controller.status_for("child").await.expect("child").status, SubagentStatus::Closed);
    assert_eq!(controller.status_for("grandchild").await.expect("grandchild").status, SubagentStatus::Closed);

    let subtree_ids = controller.collect_spawn_subtree_ids("parent").await.expect("collect subtree");
    assert_eq!(subtree_ids, vec!["parent".to_string(), "child".to_string(), "grandchild".to_string()]);

    let mut restart_ids = Vec::new();
    for node_id in subtree_ids {
        if controller.reopen_single(node_id.as_str()).await.expect("reopen subtree node") {
            restart_ids.push(node_id);
        }
    }

    assert_eq!(restart_ids, vec!["parent".to_string(), "child".to_string(), "grandchild".to_string()]);
    assert_eq!(controller.status_for("parent").await.expect("parent").status, SubagentStatus::Queued);
    assert_eq!(controller.status_for("child").await.expect("child").status, SubagentStatus::Queued);
    assert_eq!(controller.status_for("grandchild").await.expect("grandchild").status, SubagentStatus::Queued);
}
