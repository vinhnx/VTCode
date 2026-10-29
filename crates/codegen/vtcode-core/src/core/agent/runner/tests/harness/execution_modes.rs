use super::*;

#[tokio::test]
async fn headless_loop_stop_preserves_failure_and_workspace_context() {
    let temp = TempDir::new().unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    cfg.automation.full_auto.max_turns = 8;
    let mut runner = Box::pin(make_runner(&temp, cfg, "loop-stop")).await;
    assert!(
        runner
            .system_prompt
            .contains(&format!("Working directory: {}", workspace_root(&temp).display()))
    );
    runner.enable_full_auto(&[tools::TASK_TRACKER.into()]).await;
    runner.loop_detector.lock().set_tool_limit(tools::TASK_TRACKER, 1);
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        tool_call_response(tools::TASK_TRACKER, json!({"action":"list"})),
        tool_call_response(tools::TASK_TRACKER, json!({"action":"list"})),
        tool_call_response(tools::TASK_TRACKER, json!({"action":"list"})),
        text_response("The task is complete."),
    ]));
    let result = Box::pin(runner.execute_task(&task("Inspect tracker", "loop-stop"), &[]))
        .await
        .unwrap();
    assert_eq!(result.outcome, TaskOutcome::LoopDetected);
}
#[tokio::test]
async fn coordinator_headless_completion_waits_for_owned_verification_and_rejects_failure() {
    use crate::core::agent::events::SessionStoreSink;
    use crate::exec::events::matrix::*;
    use vtcode_memory::matrix::MatrixState;
    for (check, succeeds) in [("sleep 0.05; printf checked > proof.txt", true), ("false", false)] {
        let temp = TempDir::new().unwrap();
        let workspace = workspace_root(&temp);
        assert!(
            std::process::Command::new("git")
                .arg("init")
                .current_dir(&workspace)
                .output()
                .unwrap()
                .status
                .success()
        );
        fs::write(workspace.join("source.rs"), "source").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["add", "source.rs"])
                .current_dir(&workspace)
                .status()
                .unwrap()
                .success()
        );
        let mut cfg = VTCodeConfig {
            default_primary_agent: "coordinator".into(),
            ..Default::default()
        };
        cfg.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Off;
        cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
        cfg.automation.full_auto.enabled = true;
        cfg.automation.full_auto.max_turns = 4;
        cfg.commands.allow_list.push("false".into());
        let session_id = "matrix-headless";
        let mut runner = Box::pin(make_runner(&temp, cfg, session_id)).await;
        runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(
            &vtcode_config::builtin_primary_coordinator_agent(),
        ));
        runner.enable_full_auto(&["matrix".into()]).await;
        let mut state = MatrixState::create(
            MatrixSpec {
                id: "check".into(),
                resources: Default::default(),
                tasks: vec![MatrixTaskSpec {
                    id: "one".into(),
                    instructions: "Inspect source".into(),
                    dependencies: vec![],
                    workspace: ".".into(),
                    access: WorkspaceAccess::Read,
                    checks: vec![check.into()],
                    resources: Default::default(),
                    timeout_secs: 5,
                    replay_safe: false,
                    inputs: vec![],
                }],
            },
            &workspace,
        )
        .unwrap();
        state.start().unwrap();
        let execution = state.reserve_ready(3).unwrap().remove(0);
        state
            .report(&execution.attempt_id, &execution.worker_id, MatrixOutcome::Success, vec![], true)
            .unwrap();
        let sink = SessionStoreSink::open(&workspace, session_id).await.unwrap();
        (sink.matrix_persistence().persist)(state.snapshot().clone()).await.unwrap();
        sink.close().await.unwrap();
        runner.provider_client = Box::new(QueuedProvider::new(vec![
            tool_call_response("matrix", json!({"action":"resume","matrix_id":"check"})),
            text_response("The task is complete."),
            text_response("The matrix results are ready."),
        ]));
        let result = Box::pin(runner.execute_task(&task("Matrix checks", "matrix-task"), &[]))
            .await
            .unwrap();
        assert_eq!(result.outcome.is_success(), succeeds, "{:?}", result.outcome);
        assert_eq!(result.turns_executed, 2);
        let sink = SessionStoreSink::open(&workspace, session_id).await.unwrap();
        let snapshots = (sink.matrix_persistence().load)().await.unwrap();
        assert_eq!(
            snapshots[0].lifecycle,
            if succeeds {
                MatrixLifecycle::Succeeded
            } else {
                MatrixLifecycle::Blocked
            }
        );
        if succeeds {
            assert_eq!(fs::read_to_string(workspace.join("proof.txt")).unwrap(), "checked");
        }
        sink.close().await.unwrap();
    }
}
#[tokio::test]
async fn exec_full_auto_continues_until_tracker_is_completed() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Finish tracker step"])).await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-continuation-success")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        tool_call_response(
            tools::TASK_TRACKER,
            json!({"action":"create", "title":"Current task", "items":["Finish tracker step"]}),
        ),
        text_response("The task is complete."),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("I have finished all the work."),
    ]));

    let result = Box::pin(runner.execute_task(&task("Harness continuation", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed > 1);
    assert!(harness_events(&result).contains(&HarnessEventKind::ContinuationStarted));

    let tracker = fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("- [x] Finish tracker step"));
}
#[tokio::test]
async fn informational_exec_does_not_continue_or_edit_an_unadopted_workspace_tracker() {
    for full_auto in [false, true] {
        for answer in [
            "VT Code is a coding assistant. The task is complete.",
            "The task is complete.",
        ] {
            let temp = TempDir::new().expect("tempdir");
            let workspace = workspace_root(&temp);
            seed_tracker(&workspace, json!(["Unrelated README edits"])).await;
            let tracker_path = workspace.join(".vtcode/tasks/current_task.md");
            let before = fs::read_to_string(&tracker_path).expect("old tracker");
            let mut cfg = VTCodeConfig::default();
            cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
            let mut runner = Box::pin(make_runner(&temp, cfg, "thread-informational-tracker")).await;
            if full_auto {
                runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
            }
            runner.provider_client = Box::new(QueuedProvider::new(vec![
                text_response(answer),
                tool_call_response(tools::TASK_TRACKER, json!({"action":"update", "index":1,"status":"completed"})),
                text_response("All work is complete."),
            ]));
            let result = Box::pin(runner.execute_task(&task("what is vtcode", "informational-task"), &[]))
                .await
                .expect("task result");
            assert_eq!(result.turns_executed, 1, "full_auto={full_auto}, answer={answer}");
            assert!(!harness_events(&result).contains(&HarnessEventKind::ContinuationStarted));
            assert_eq!(fs::read_to_string(&tracker_path).expect("preserved tracker"), before);
        }
    }
}
#[tokio::test]
async fn runner_keeps_openai_requests_stateless_and_reuses_session_cache_key() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Cache-aware tracker step"])).await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-cache-lineage")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .build(tool_call_response_with_request_id(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
            "resp_first_turn",
        ))
        .build(text_response("All work is complete."))
        .build(text_response("All work is complete."))
        .build(text_response("All work is complete."));
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Cache-aware continuation", "exec-task"), &[]))
        .await
        .expect("task result");

    assert!(result.turns_executed >= 2);

    let requests = recorded.recorded_requests();
    assert!(requests.len() >= 2);
    assert_eq!(requests[0].previous_response_id, None);
    // The key is stable per session with no per-turn suffix: OpenAI routes
    // by (prefix hash + key), so the key must stay consistent across requests
    // sharing a prefix. Prefix identity is tracked separately via
    // `tool_catalog_hash` / `system_prompt_prefix_hash`; both turns must
    // share one stable key.
    assert_eq!(
        requests[0].prompt_cache_key.as_deref(),
        Some("vtcode:openai:thread-cache-lineage"),
        "unexpected cache key: {:?}",
        requests[0].prompt_cache_key
    );
    assert_eq!(requests[1].previous_response_id, None);
    assert!(requests[1].messages.starts_with(&requests[0].messages));
    assert_eq!(requests[1].prompt_cache_key, requests[0].prompt_cache_key);
}
#[tokio::test]
async fn exec_full_auto_runs_verification_before_accepting_completion() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(
        &workspace,
        json!([{
            "description": "Verify harness",
            "status": "completed",
            "verify": "pwd",
        }]),
    )
    .await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-verification-success")).await;
    runner.enable_full_auto(&[]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action":"create", "title":"Current verification", "items":[{
                    "description":"Verify harness", "status":"completed", "verify":"pwd"
                }]
            }),
        ),
        text_response("The task is complete."),
    ]));

    let result = Box::pin(runner.execute_task(&task("Verification success", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::VerificationStarted));
    assert!(events.contains(&HarnessEventKind::VerificationPassed));
}
#[tokio::test]
async fn exec_full_auto_retries_after_verification_failure() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(
        &workspace,
        json!([{
            "description": "Verify harness",
            "status": "completed",
            "verify": "cat missing-verification-target",
        }]),
    )
    .await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    vt_cfg.automation.full_auto.max_turns = 3;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-verification-failure")).await;
    runner.enable_full_auto(&[]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action":"create", "title":"Current verification", "items":[{
                    "description":"Verify harness", "status":"completed", "verify":"cat missing-verification-target"
                }]
            }),
        ),
        text_response("The task is complete."),
        text_response("Task is now complete."),
    ]));

    let result = Box::pin(runner.execute_task(&task("Verification failure", "exec-task"), &[]))
        .await
        .expect("task result");

    assert!(!result.outcome.is_success());
    assert!(result.outcome.description().contains("Unverified"));
    assert!(result.outcome.description().contains("missing-verification-target"));
    assert!(result.turns_executed > 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::VerificationStarted));
    assert!(events.contains(&HarnessEventKind::VerificationFailed));
    assert!(events.contains(&HarnessEventKind::ContinuationStarted));
}
#[tokio::test]
async fn review_runs_skip_continuation_and_finish_single_pass() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-review-skip")).await;
    runner.enable_full_auto(&[tools::UNIFIED_FILE.to_string()]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response("The task is complete.")]));

    let result = Box::pin(runner.execute_task(&task("Review task", "review-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::ContinuationSkipped));
    assert!(!events.contains(&HarnessEventKind::ContinuationStarted));
}
#[tokio::test]
async fn review_non_openai_request_exposes_only_read_only_inspection_tools() {
    let temp = TempDir::new().expect("tempdir");
    let request =
        record_review_request(&temp, ModelId::default(), "queued-test-provider", "thread-review-non-openai-tools")
            .await;
    assert_review_request_exposes_only_code_search(&request);
    assert!(request.tool_choice.is_none());
}
#[tokio::test]
async fn review_openai_compatible_request_filters_inactive_tools() {
    let temp = TempDir::new().expect("tempdir");
    let request = record_review_request(&temp, ModelId::GPT56Sol, "openai", "thread-review-openai-compatible").await;
    assert_review_request_exposes_only_code_search(&request);
}
#[tokio::test]
async fn review_openai_non_responses_request_filters_inactive_tools() {
    let temp = TempDir::new().expect("tempdir");
    let model = ModelId::OpenAIGptOss20b;
    let expected_model = model.as_str().into_owned();
    let request = record_review_request(&temp, model, "openai", "thread-review-openai-non-responses").await;
    assert_eq!(request.model, expected_model);
    assert_review_request_exposes_only_code_search(&request);
}
#[tokio::test]
async fn planning_workflow_runs_skip_continuation_and_finish_single_pass() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-planning-workflow-skip")).await;
    runner.enable_full_auto(&[]).await;
    runner.enable_planning();
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(COMPLETE_HEADLESS_PLAN)]));

    let result = Box::pin(runner.execute_task(&task("Planning workflow task", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::ContinuationSkipped));
    assert!(!events.contains(&HarnessEventKind::ContinuationStarted));
}
#[tokio::test]
async fn exec_only_policy_skips_when_full_auto_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.continuation_policy = vtcode_config::core::agent::ContinuationPolicy::ExecOnly;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-exec-only-skip")).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response("The task is complete.")]));

    let result = Box::pin(runner.execute_task(&task("Exec task", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::ContinuationSkipped));
    assert!(!events.contains(&HarnessEventKind::ContinuationStarted));
}
