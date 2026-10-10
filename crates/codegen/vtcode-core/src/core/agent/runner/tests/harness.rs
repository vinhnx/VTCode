#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

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

    assert!(matches!(result.outcome, TaskOutcome::TurnLimitReached { .. }));
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

fn assert_review_request_exposes_only_code_search(request: &LLMRequest) {
    let tool_names = request
        .tools
        .as_deref()
        .map(|definitions| definitions.iter().map(|tool| tool.function_name()).collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(tool_names.contains(&tools::CODE_SEARCH));
    for mutating_tool in [tools::EXEC_COMMAND, tools::APPLY_PATCH] {
        assert!(!tool_names.contains(&mutating_tool), "review request must hide {mutating_tool}; got {tool_names:?}");
    }
}

async fn record_review_request(
    temp: &TempDir,
    model: ModelId,
    provider_name: &'static str,
    session_id: &str,
) -> LLMRequest {
    let mut runner = Box::pin(make_runner_for_model(temp, VTCodeConfig::default(), session_id, model)).await;
    let allowlist = runner.review_tool_allowlist(&[tools::WILDCARD_ALL.to_string()]).await;
    runner.enable_full_auto(&allowlist).await;

    let provider = RecordingQueuedProvider::with_name(
        provider_name,
        vec![
            text_response("The review is complete."),
            text_response("The review is complete."),
        ],
    );
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);
    let _result = Box::pin(runner.execute_task(&task("Review task", "review-task"), &[]))
        .await
        .expect("task result");

    recorded
        .recorded_requests()
        .into_iter()
        .next()
        .expect("review provider request")
}

#[tokio::test]
async fn planning_workflow_runs_skip_continuation_and_finish_single_pass() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "thread-planning-workflow-skip")).await;
    runner.enable_full_auto(&[]).await;
    runner.enable_planning();
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response("The task is complete.")]));

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

#[tokio::test]
async fn tool_loop_limit_writes_blocked_handoff_artifacts() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Investigate loop"])).await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    vt_cfg.automation.full_auto.max_turns = 1;
    vt_cfg.tools.max_tool_loops = 1;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-tool-loop-blocked")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![tool_call_response(
        tools::TASK_TRACKER,
        json!({
            "action": "list"
        }),
    )]));

    let result = Box::pin(runner.execute_task(&task("Loop blocked", "exec-task"), &[]))
        .await
        .expect("task result");

    assert!(matches!(result.outcome, TaskOutcome::ToolLoopLimitReached { .. }));
    let paths = harness_paths(&result, HarnessEventKind::BlockedHandoffWritten);
    assert_eq!(paths.len(), 2);
    for path in paths {
        let content = fs::read_to_string(&path).expect("blocked handoff file");
        assert!(content.contains("tool_loop_limit_reached"));
        assert!(content.contains("Stopped after reaching tool loop limit"));
        assert!(!content.contains("resume_command:"));
        assert!(
            content.contains("Resume unavailable because the legacy agent runner does not create a session archive.")
        );
    }
}

#[tokio::test]
async fn plan_build_evaluate_exec_creates_spec_and_evaluation_artifacts() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-plan-build-evaluate")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        json_response(planner_response_json("pwd")),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("The task is complete."),
        json_response(evaluator_response_json("pass", "Evaluator accepted the implementation.", 0)),
    ]));

    let result = Box::pin(runner.execute_task(&task("Planner + evaluator", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(workspace.join(".vtcode/tasks/current_spec.md").exists(), "planner should write current_spec.md");
    assert!(
        workspace.join(".vtcode/tasks/current_contract.md").exists(),
        "planner should write current_contract.md"
    );
    assert!(
        workspace.join(".vtcode/tasks/current_evaluation.md").exists(),
        "evaluator should write current_evaluation.md"
    );
    let tracker = fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("outcome: The requested change is implemented and tracked."));
    assert!(tracker.contains("verify: pwd"));

    let contract = fs::read_to_string(workspace.join(".vtcode/tasks/current_contract.md")).expect("contract file");
    assert!(contract.contains("Execution Contract"));
    assert!(contract.contains("Verify with `pwd`"));

    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::PlanningStarted));
    assert!(events.contains(&HarnessEventKind::PlanningCompleted));
    assert!(events.contains(&HarnessEventKind::EvaluationStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));
}

#[tokio::test]
async fn default_full_auto_exec_uses_plan_build_evaluate_harness() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let vt_cfg = VTCodeConfig::default();
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-default-plan-build-evaluate")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        json_response(planner_response_json("pwd")),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("The task is complete."),
        json_response(evaluator_response_json("pass", "Evaluator accepted the implementation.", 0)),
    ]));

    let result = Box::pin(runner.execute_task(&task("Default planner + evaluator", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(
        workspace.join(".vtcode/tasks/current_spec.md").exists(),
        "default full-auto should write current_spec.md"
    );
    assert!(
        workspace.join(".vtcode/tasks/current_contract.md").exists(),
        "default full-auto should write current_contract.md"
    );
    assert!(
        workspace.join(".vtcode/tasks/current_evaluation.md").exists(),
        "default full-auto should write current_evaluation.md"
    );

    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::PlanningStarted));
    assert!(events.contains(&HarnessEventKind::PlanningCompleted));
    assert!(events.contains(&HarnessEventKind::EvaluationStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));
}

#[tokio::test]
async fn evaluator_failure_forces_revision_before_success() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-revision")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json("pwd")))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ))
        .build(text_response("The task is complete."))
        .evaluator(json_response(evaluator_response_json("fail", "A high-severity issue remains.", 1)))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json("pass", "All issues have been addressed.", 0)));
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Evaluator revision", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));
}

#[tokio::test]
async fn evaluator_request_includes_verification_results() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-verification")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json("pwd")))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ))
        .build(text_response("The task is complete."))
        .evaluator(json_response(evaluator_response_json("pass", "Verification evidence looks good.", 0)));
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Evaluator verification evidence", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);

    let requests = recorded.recorded_requests();
    let evaluator_request = requests.last().expect("evaluator request");
    let evaluator_prompt = evaluator_request
        .messages
        .first()
        .map(|message| message.content.as_text().into_owned())
        .expect("evaluator prompt");
    assert!(evaluator_prompt.contains("Current contract:"));
    assert!(evaluator_prompt.contains("Verification results:"));
    assert!(evaluator_prompt.contains("[PASS] pwd (exit 0)"));
    assert!(evaluator_prompt.contains("contract_fidelity"));
    assert!(evaluator_request.system_prompt.as_deref().is_some_and(|prompt| {
        prompt.contains("smallest claim the evidence supports")
            && prompt.contains("falsifier")
            && prompt.contains("generalization_notes")
            && !prompt.contains("arXiv")
    }));

    let planner_request = requests
        .iter()
        .find(|request| harness_role_of(request) == HarnessRole::Planner)
        .expect("planner request");
    let planner_system = planner_request.system_prompt.as_deref().expect("planner system prompt");
    assert_eq!(planner_system.matches("JSON only").count(), 1);
    let planner_prompt = planner_request
        .messages
        .first()
        .map(|message| message.content.as_text().into_owned())
        .expect("planner prompt");
    assert!(!planner_prompt.contains("JSON only"));
}

#[tokio::test]
async fn evaluator_notes_render_and_replan_adds_falsifier_tracker_steps() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 8;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evidence-bounded-replan")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json("pwd")))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ))
        .build(text_response("The task is complete."))
        .evaluator(json_response(evaluator_response_json_with_notes(
            "fail",
            "The implementation needs one bounded follow-up.",
            1,
            json!([{
                "claim": "The changed code-search scope remains bounded",
                "scope": "Only code-search changes in this task",
                "evidence": "The focused code-search regression tests pass",
                "falsifier": "A regression test returns a result outside the requested scope",
            }]),
        )))
        .replanner(json_response(json!({
            "revised_feature_list": "# Features\n\n- [x] Preserve bounded code-search scope",
            "contract_addendum": "- Preserve the supplied scope.",
            "new_tracker_items": [],
            "preserved_scopes": ["Only code-search changes in this task"],
            "rationale": "Keep the observation task-scoped.",
        })))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 2,
                "status": "completed",
            }),
        ))
        .build(text_response("The falsifier is verified."))
        .build(text_response("The evidence-bounded follow-up is complete."))
        .build(text_response("All task-scoped verification is complete."))
        .evaluator(json_response(evaluator_response_json("pass", "Follow-up verified.", 0)));
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Evidence-bounded replan", "exec-task"), &[]))
        .await
        .expect("task result");

    assert!(
        matches!(result.outcome, TaskOutcome::Success | TaskOutcome::StoppedNoAction),
        "unexpected replan outcome: {:?}",
        result.outcome
    );
    let replan_request = recorded
        .recorded_requests()
        .into_iter()
        .find(|request| harness_role_of(request) == HarnessRole::Replanner)
        .expect("replanner request");
    let replan_prompt = replan_request
        .messages
        .first()
        .map(|message| message.content.as_text().into_owned())
        .expect("replanner prompt");
    assert!(replan_prompt.contains("Only code-search changes in this task"));
    assert!(replan_prompt.contains("A regression test returns a result outside the requested scope"));
    assert!(replan_request.system_prompt.as_deref().is_some_and(|prompt| {
        prompt.contains("smallest claim the evidence supports") && !prompt.contains("arXiv")
    }));
    let tracker = fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("Falsify task-scoped claim: The changed code-search scope remains bounded"));
    assert!(tracker.contains("A regression test returns a result outside the requested scope"));

    let contract = fs::read_to_string(workspace.join(".vtcode/tasks/current_contract.md")).expect("contract file");
    assert!(contract.contains("Only code-search changes in this task"));
    assert!(contract.contains("Evidence: The focused code-search regression tests pass"));
}

#[tokio::test]
async fn evaluator_scorecard_below_threshold_forces_revision() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-scorecard")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json("pwd")))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ))
        .build(text_response("The task is complete."))
        .evaluator(json_response(evaluator_response_json_with_scorecard("pass", "Looks mostly good.", 0, (5, 3, 5, 5))))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json("pass", "All issues have been addressed.", 0)));
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Evaluator scorecard revision", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));

    let evaluation =
        fs::read_to_string(temp.path().join(".vtcode/tasks/current_evaluation.md")).expect("evaluation file");
    assert!(evaluation.contains("## Scorecard"));
    assert!(evaluation.contains("Functionality: 5/5"));
}

#[tokio::test]
async fn evaluator_missing_scorecard_forces_revision() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-missing-scorecard")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json("pwd")))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ))
        .build(text_response("The task is complete."))
        .evaluator(json_response(json!({
            "verdict": "pass",
            "summary": "Looks mostly good.",
            "high_severity_findings": 0,
            "findings": [],
            "unmet_contract_items": [],
            "residual_risks": [],
            "required_tracker_updates": [],
        })))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json("pass", "All issues have been addressed.", 0)));
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Evaluator missing scorecard revision", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));

    let evaluation =
        fs::read_to_string(temp.path().join(".vtcode/tasks/current_evaluation.md")).expect("evaluation file");
    assert!(evaluation.contains("All issues have been addressed."));
}

#[tokio::test]
async fn evaluator_out_of_range_scorecard_forces_revision() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-invalid-scorecard")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json("pwd")))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ))
        .build(text_response("The task is complete."))
        .evaluator(json_response(evaluator_response_json_with_scorecard("pass", "Looks mostly good.", 0, (5, 9, 5, 5))))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json("pass", "All issues have been addressed.", 0)));
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Evaluator invalid scorecard revision", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));

    let evaluation =
        fs::read_to_string(temp.path().join(".vtcode/tasks/current_evaluation.md")).expect("evaluation file");
    assert!(evaluation.contains("All issues have been addressed."));
}

#[tokio::test]
async fn evaluator_exhaustion_writes_blocked_handoff_with_artifact_paths() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.agent.harness.max_revision_rounds = 1;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-exhaustion")).await;
    runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json("pwd")))
        .build(tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ))
        .build(text_response("The task is complete."))
        .evaluator(json_response(evaluator_response_json("fail", "First evaluator rejection.", 1)))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json("fail", "Second evaluator rejection.", 1)));
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(&task("Evaluator exhaustion", "exec-task"), &[]))
        .await
        .expect("task result");

    assert!(matches!(result.outcome, TaskOutcome::Failed { .. }));
    let paths = harness_paths(&result, HarnessEventKind::BlockedHandoffWritten);
    assert_eq!(paths.len(), 2);
    for path in paths {
        let content = fs::read_to_string(&path).expect("blocked handoff file");
        assert!(content.contains("current_spec.md"));
        assert!(content.contains("current_contract.md"));
        assert!(content.contains("current_evaluation.md"));
    }
    assert!(workspace.join(".vtcode/tasks/current_spec.md").exists());
    assert!(workspace.join(".vtcode/tasks/current_contract.md").exists());
    assert!(workspace.join(".vtcode/tasks/current_evaluation.md").exists());
}

fn refusal_response(content: Option<&str>, category: &str) -> LLMResponse {
    LLMResponse {
        content: content.map(str::to_string),
        finish_reason: FinishReason::Refusal,
        reasoning_details: Some(vec![json!({"type": "stop_details", "category": category}).to_string()]),
        ..LLMResponse::default()
    }
}

#[tokio::test]
async fn refusal_stops_runner_immediately_with_reason_instead_of_idle_retries() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-refusal-stop")).await;
    let provider = RecordingQueuedProvider::with_name(
        "queued-test-provider",
        vec![
            refusal_response(None, "cyber"),
            text_response("should never be requested"),
            text_response("should never be requested"),
            text_response("should never be requested"),
        ],
    );
    runner.provider_client = Box::new(provider.clone());

    let result = Box::pin(runner.execute_task(&task("Refused task", "refused-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(provider.recorded_requests().len(), 1, "a refusal must not be resent");
    let TaskOutcome::Refused { reason } = &result.outcome else {
        panic!("expected refused outcome, got {:?}", result.outcome);
    };
    assert!(reason.starts_with("The model declined this request (category: cyber)."));
    assert_eq!(result.outcome.code(), "refused");
    assert!(result.summary.contains("Outcome Code: refused"));
    assert!(result.summary.contains(reason.as_str()));
    assert!(result.thread_events.iter().any(|event| matches!(
        event,
        ThreadEvent::TurnFailed(failed) if failed.message == *reason
    )));
    assert!(
        !runner
            .thread_handle
            .messages()
            .iter()
            .any(|message| message.role == crate::llm::provider::MessageRole::Assistant),
        "refused output must not stay in the kept history"
    );
}

#[tokio::test]
async fn refusal_explanation_falls_back_to_trimmed_content() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-refusal-content")).await;
    let mut response = refusal_response(Some("  I can't help with that request.  "), "");
    response.reasoning_details = None;
    runner.provider_client = Box::new(QueuedProvider::new(vec![response]));

    let result = Box::pin(runner.execute_task(&task("Refused task", "refused-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(
        result.outcome,
        TaskOutcome::refused(
            "The model declined this request: I can't help with that request. \
             The request was not retried; rephrase it or switch models."
                .to_string()
        )
    );
}
