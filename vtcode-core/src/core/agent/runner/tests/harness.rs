#![allow(missing_docs)]

use super::*;

#[tokio::test]
async fn exec_full_auto_continues_until_tracker_is_completed() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Finish tracker step"])).await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    vt_cfg.automation.full_auto.max_turns = 3;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-continuation-success")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
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

    let tracker =
        fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("- [x] Finish tracker step"));
}

#[tokio::test]
async fn runner_keeps_openai_requests_stateless_and_reuses_session_cache_key() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Cache-aware tracker step"])).await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-cache-lineage")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
    assert_eq!(
        requests[0].prompt_cache_key.as_deref(),
        Some("vtcode:openai:thread-cache-lineage")
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
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-verification-success")).await;
    runner.enable_full_auto(&[]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

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
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    vt_cfg.automation.full_auto.max_turns = 2;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-verification-failure")).await;
    runner.enable_full_auto(&[]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        text_response("The task is complete."),
        text_response("Task is now complete."),
    ]));

    let result = Box::pin(runner.execute_task(&task("Verification failure", "exec-task"), &[]))
        .await
        .expect("task result");

    assert!(matches!(
        result.outcome,
        TaskOutcome::TurnLimitReached { .. }
    ));
    assert!(result.turns_executed > 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::VerificationStarted));
    assert!(events.contains(&HarnessEventKind::VerificationFailed));
    assert!(events.contains(&HarnessEventKind::ContinuationStarted));
}

#[tokio::test]
async fn review_runs_skip_continuation_and_finish_single_pass() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(make_runner(
        &temp,
        VTCodeConfig::default(),
        "thread-review-skip",
    ))
    .await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

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
    let request = record_review_request(
        &temp,
        ModelId::default(),
        "queued-test-provider",
        "thread-review-non-openai-tools",
    )
    .await;
    assert_review_request_exposes_only_code_search(&request);
    assert!(request.tool_choice.is_none());
}

#[tokio::test]
async fn review_openai_compatible_request_filters_inactive_tools() {
    let temp = TempDir::new().expect("tempdir");
    let request = record_review_request(
        &temp,
        ModelId::GPT53Codex,
        "openai",
        "thread-review-openai-compatible",
    )
    .await;
    assert_review_request_exposes_only_code_search(&request);
}

#[tokio::test]
async fn review_openai_non_responses_request_filters_inactive_tools() {
    let temp = TempDir::new().expect("tempdir");
    let model = ModelId::OpenAIGptOss20b;
    let expected_model = model.as_str().into_owned();
    let request =
        record_review_request(&temp, model, "openai", "thread-review-openai-non-responses").await;
    assert_eq!(request.model, expected_model);
    assert_review_request_exposes_only_code_search(&request);
}

fn assert_review_request_exposes_only_code_search(request: &LLMRequest) {
    let tool_names = request
        .tools
        .as_deref()
        .map(|definitions| {
            definitions
                .iter()
                .map(|tool| tool.function_name())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(tool_names.contains(&tools::CODE_SEARCH));
    for mutating_tool in [tools::EXEC_COMMAND, tools::APPLY_PATCH] {
        assert!(
            !tool_names.contains(&mutating_tool),
            "review request must hide {mutating_tool}; got {tool_names:?}"
        );
    }
}

async fn record_review_request(
    temp: &TempDir,
    model: ModelId,
    provider_name: &'static str,
    session_id: &str,
) -> LLMRequest {
    let mut runner = Box::pin(make_runner_for_model(
        temp,
        VTCodeConfig::default(),
        session_id,
        model,
    ))
    .await;
    let allowlist = runner
        .review_tool_allowlist(&[tools::WILDCARD_ALL.to_string()])
        .await;
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
    let mut runner = Box::pin(make_runner(
        &temp,
        VTCodeConfig::default(),
        "thread-planning-workflow-skip",
    ))
    .await;
    runner.enable_full_auto(&[]).await;
    runner.enable_planning();
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

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
    vt_cfg.agent.harness.continuation_policy =
        vtcode_config::core::agent::ContinuationPolicy::ExecOnly;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-exec-only-skip")).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

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
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    vt_cfg.automation.full_auto.max_turns = 1;
    vt_cfg.tools.max_tool_loops = 1;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-tool-loop-blocked")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![tool_call_response(
        tools::TASK_TRACKER,
        json!({
            "action": "list"
        }),
    )]));

    let result = Box::pin(runner.execute_task(&task("Loop blocked", "exec-task"), &[]))
        .await
        .expect("task result");

    assert!(matches!(
        result.outcome,
        TaskOutcome::ToolLoopLimitReached { .. }
    ));
    let paths = harness_paths(&result, HarnessEventKind::BlockedHandoffWritten);
    assert_eq!(paths.len(), 2);
    for path in paths {
        let content = fs::read_to_string(&path).expect("blocked handoff file");
        assert!(content.contains("tool_loop_limit_reached"));
        assert!(content.contains("Stopped after reaching tool loop limit"));
    }
}

#[tokio::test]
async fn plan_build_evaluate_exec_creates_spec_and_evaluation_artifacts() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-plan-build-evaluate")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        json_response(evaluator_response_json(
            "pass",
            "Evaluator accepted the implementation.",
            0,
        )),
    ]));

    let result = Box::pin(runner.execute_task(&task("Planner + evaluator", "exec-task"), &[]))
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(
        workspace.join(".vtcode/tasks/current_spec.md").exists(),
        "planner should write current_spec.md"
    );
    assert!(
        workspace.join(".vtcode/tasks/current_contract.md").exists(),
        "planner should write current_contract.md"
    );
    assert!(
        workspace
            .join(".vtcode/tasks/current_evaluation.md")
            .exists(),
        "evaluator should write current_evaluation.md"
    );
    let tracker =
        fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("outcome: The requested change is implemented and tracked."));
    assert!(tracker.contains("verify: pwd"));

    let contract = fs::read_to_string(workspace.join(".vtcode/tasks/current_contract.md"))
        .expect("contract file");
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
    let mut runner = Box::pin(make_runner(
        &temp,
        vt_cfg,
        "thread-default-plan-build-evaluate",
    ))
    .await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        json_response(evaluator_response_json(
            "pass",
            "Evaluator accepted the implementation.",
            0,
        )),
    ]));

    let result =
        Box::pin(runner.execute_task(&task("Default planner + evaluator", "exec-task"), &[]))
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
        workspace
            .join(".vtcode/tasks/current_evaluation.md")
            .exists(),
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
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-revision")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        .evaluator(json_response(evaluator_response_json(
            "fail",
            "A high-severity issue remains.",
            1,
        )))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json(
            "pass",
            "All issues have been addressed.",
            0,
        )));
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
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-verification")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        .evaluator(json_response(evaluator_response_json(
            "pass",
            "Verification evidence looks good.",
            0,
        )));
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);

    let result =
        Box::pin(runner.execute_task(&task("Evaluator verification evidence", "exec-task"), &[]))
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
}

#[tokio::test]
async fn evaluator_scorecard_below_threshold_forces_revision() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-scorecard")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        .evaluator(json_response(evaluator_response_json_with_scorecard(
            "pass",
            "Looks mostly good.",
            0,
            (5, 3, 5, 5),
        )))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json(
            "pass",
            "All issues have been addressed.",
            0,
        )));
    runner.provider_client = Box::new(provider);

    let result =
        Box::pin(runner.execute_task(&task("Evaluator scorecard revision", "exec-task"), &[]))
            .await
            .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));

    let evaluation = fs::read_to_string(temp.path().join(".vtcode/tasks/current_evaluation.md"))
        .expect("evaluation file");
    assert!(evaluation.contains("## Scorecard"));
    assert!(evaluation.contains("Functionality: 5/5"));
}

#[tokio::test]
async fn evaluator_missing_scorecard_forces_revision() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(
        &temp,
        vt_cfg,
        "thread-evaluator-missing-scorecard",
    ))
    .await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        .evaluator(json_response(evaluator_response_json(
            "pass",
            "All issues have been addressed.",
            0,
        )));
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(
        &task("Evaluator missing scorecard revision", "exec-task"),
        &[],
    ))
    .await
    .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));

    let evaluation = fs::read_to_string(temp.path().join(".vtcode/tasks/current_evaluation.md"))
        .expect("evaluation file");
    assert!(evaluation.contains("All issues have been addressed."));
}

#[tokio::test]
async fn evaluator_out_of_range_scorecard_forces_revision() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(
        &temp,
        vt_cfg,
        "thread-evaluator-invalid-scorecard",
    ))
    .await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        .evaluator(json_response(evaluator_response_json_with_scorecard(
            "pass",
            "Looks mostly good.",
            0,
            (5, 9, 5, 5),
        )))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json(
            "pass",
            "All issues have been addressed.",
            0,
        )));
    runner.provider_client = Box::new(provider);

    let result = Box::pin(runner.execute_task(
        &task("Evaluator invalid scorecard revision", "exec-task"),
        &[],
    ))
    .await
    .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));

    let evaluation = fs::read_to_string(temp.path().join(".vtcode/tasks/current_evaluation.md"))
        .expect("evaluation file");
    assert!(evaluation.contains("All issues have been addressed."));
}

#[tokio::test]
async fn evaluator_exhaustion_writes_blocked_handoff_with_artifact_paths() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.agent.harness.max_revision_rounds = 1;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = Box::pin(make_runner(&temp, vt_cfg, "thread-evaluator-exhaustion")).await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
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
        .evaluator(json_response(evaluator_response_json(
            "fail",
            "First evaluator rejection.",
            1,
        )))
        .replanner(text_response("Revision 1: task is complete."))
        .evaluator(json_response(evaluator_response_json(
            "fail",
            "Second evaluator rejection.",
            1,
        )));
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
    assert!(
        workspace
            .join(".vtcode/tasks/current_evaluation.md")
            .exists()
    );
}
