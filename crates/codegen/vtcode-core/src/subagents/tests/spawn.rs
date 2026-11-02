use super::*;

#[tokio::test]
async fn controller_exposes_builtin_specs() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let specs = controller.effective_specs().await;
    assert!(specs.iter().any(|spec| spec.name == "explorer"));
    assert!(specs.iter().any(|spec| spec.name == "worker"));
}
#[tokio::test]
async fn spawn_defaults_to_single_explicit_mention() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "explorer");
    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_defaults_to_single_natural_language_selection() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let mentions = controller
        .set_turn_delegation_hints_from_input("use explorer agent to inspect the codebase")
        .await;
    assert_eq!(mentions, vec!["explorer".to_string()]);

    let spawned = controller
        .spawn(SpawnAgentRequest {
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "explorer");
    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_rejects_mismatched_explicit_mention() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
        .await;

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("mismatched mention should fail");

    assert!(err.to_string().contains("user explicitly selected 'explorer'"));
}
#[tokio::test]
async fn spawn_rejects_write_capable_agent_without_explicit_request_or_agent_type() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let err = controller
        .spawn(SpawnAgentRequest {
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("write-capable agent should require explicit request or agent_type");

    assert!(err.to_string().contains("cannot launch write-capable agent"));
}
#[tokio::test]
async fn spawn_allows_write_capable_agent_with_explicit_agent_type() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("explicit agent_type should allow write-capable agent");

    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_rejects_primary_only_agent_as_child() {
    let temp = TempDir::new().expect("tempdir");
    write_test_primary_agent(temp.path());
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let mentions = controller
        .set_turn_delegation_hints_from_input("@agent-duck discuss the task")
        .await;
    assert!(mentions.is_empty());

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("duck".to_string()),
            message: Some("Discuss the task.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("primary-only agent should not spawn as child");

    assert!(err.to_string().contains("Unknown subagent type duck"));
}
#[tokio::test]
async fn spawn_accepts_background_flag_outside_managed_background_runtime() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            background: true,
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("background child spawn should succeed");

    assert!(spawned.background);
    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_allows_background_capable_spec_as_foreground_child() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("run background-demo subagent and demo")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("background-demo".to_string()),
            message: Some("Run the demo.".to_string()),
            background: false,
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("foreground background-capable spawn should succeed");

    assert_eq!(spawned.agent_name, "background-demo");
    assert!(!spawned.background);
    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_rejects_vague_task_even_with_explicit_request() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("run worker subagent and report")
        .await;

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("report".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("vague task should require clarification");

    assert!(err.to_string().contains("too vague ('report')"));
}
#[tokio::test]
async fn spawn_defaults_to_write_capable_run_subagent_selection() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let mentions = controller
        .set_turn_delegation_hints_from_input("run worker subagent and implement the change")
        .await;
    assert_eq!(mentions, vec!["worker".to_string()]);

    let spawned = controller
        .spawn(SpawnAgentRequest {
            message: Some("Implement the change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "worker");
    controller.close(&spawned.id).await.expect("close");
}
#[tokio::test]
async fn spawn_rejects_read_only_agent_when_auto_delegate_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    write_test_read_only_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.auto_delegate_read_only = false;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("readonly-demo".to_string()),
            message: Some("Inspect the repository.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("read-only agent should require explicit delegation");

    assert!(
        err.to_string()
            .contains("cannot proactively launch read-only agent 'readonly-demo'")
    );
}
#[test]
fn load_memory_appendix_renders_compact_summary() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");
    std::fs::create_dir_all(&memory_dir).expect("memory dir");
    std::fs::write(
            memory_dir.join("MEMORY.md"),
            "# Reviewer Memory\n\n## Preferences\n- Keep diffs surgical.\n- Run focused tests before broad checks.\n- Prefer repo docs for orientation.\n- Ask only when a decision is materially blocked.\n- Additional long-form notes that should stay out of the prompt body.\n",
        )
        .expect("write memory");

    let appendix = load_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
        .expect("appendix")
        .expect("memory appendix");

    assert!(appendix.contains("Persistent memory file:"));
    assert!(appendix.contains("Key points:"));
    assert!(appendix.contains("Keep diffs surgical."));
    assert!(appendix.contains("Open `MEMORY.md` when exact wording or more detail matters."));
    assert!(!appendix.contains("Current MEMORY.md excerpt"));
    assert!(!appendix.contains("## Preferences"));
}
#[tokio::test]
async fn async_load_memory_appendix_matches_sync_output() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");
    std::fs::create_dir_all(&memory_dir).expect("memory dir");
    std::fs::write(
        memory_dir.join("MEMORY.md"),
        "# Reviewer Memory\n\n- Keep the patch focused.\n- Run nextest before the workspace gate.\n",
    )
    .expect("write memory");

    let sync =
        load_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project)).expect("sync appendix");
    let asynchronous = load_memory_appendix_async(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
        .await
        .expect("async appendix");

    assert_eq!(asynchronous, sync);
}
#[test]
fn load_primary_memory_appendix_reads_existing_memory_without_write_guidance() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");
    std::fs::create_dir_all(&memory_dir).expect("memory dir");
    std::fs::write(
        memory_dir.join("MEMORY.md"),
        "# Reviewer Memory\n\n## Preferences\n- Keep diffs surgical.\n- Run focused tests before broad checks.\n",
    )
    .expect("write memory");

    let appendix = load_primary_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
        .expect("appendix")
        .expect("memory appendix");

    assert!(appendix.contains("Primary-agent memory file:"));
    assert!(appendix.contains("Loaded read-only for this request."));
    assert!(appendix.contains("Key points:"));
    assert!(appendix.contains("Keep diffs surgical."));
    assert!(!appendix.contains("Read and maintain `MEMORY.md`"));
    assert!(!appendix.contains("Create or update `MEMORY.md`"));
    assert!(!appendix.contains("Open `MEMORY.md` when exact wording or more detail matters."));
}
#[test]
fn load_primary_memory_appendix_missing_memory_is_noop_without_directory_creation() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");

    let appendix =
        load_primary_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project)).expect("appendix");

    assert!(appendix.is_none());
    assert!(!memory_dir.exists());
}
