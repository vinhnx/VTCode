use super::*;

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
