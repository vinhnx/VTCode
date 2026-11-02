use super::*;

#[tokio::test]
async fn evaluator_length_recovery_retries_only_the_tool_free_response_once() {
    for truncate_retry in [false, true] {
        let temp = TempDir::new().unwrap();
        let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "evaluator-length")).await;
        runner.enable_full_auto(&[tools::TASK_TRACKER.into()]).await;
        let mut truncated = text_response("{\"verdict\":\"pass\",\"summary\":\"unfinished");
        truncated.finish_reason = FinishReason::Length;
        let mut provider = RoleQueuedProvider::new();
        provider
            .planner(json_response(planner_response_json("pwd")))
            .build(tool_call_response(tools::TASK_TRACKER, json!({"action":"update","index":1,"status":"completed"})))
            .build(text_response("The task is complete."))
            .evaluator(truncated.clone())
            .evaluator(if truncate_retry {
                truncated
            } else {
                json_response(evaluator_response_json("pass", "Checked", 0))
            });
        let observed = provider.clone();
        runner.provider_client = Box::new(provider);
        let result = Box::pin(runner.execute_task(&task("Evaluator length recovery", "exec-task"), &[])).await;
        if truncate_retry {
            assert!(format!("{:#}", result.unwrap_err()).contains("truncated after one retry"));
        } else {
            assert_eq!(result.unwrap().outcome, TaskOutcome::Success);
        }
        let requests = observed.recorded_requests();
        let evaluator_requests: Vec<_> = requests
            .iter()
            .filter(|r| harness_role_of(r) == HarnessRole::Evaluator)
            .collect();
        assert_eq!(evaluator_requests.len(), 2);
        assert_eq!(requests.iter().filter(|r| harness_role_of(r) == HarnessRole::Build).count(), 2);
        for request in &evaluator_requests {
            assert_eq!(request.max_tokens, Some(4096));
            assert!(request.tools.as_ref().is_some_and(|tools| tools.is_empty()));
            assert_eq!(request.output_format, Some(json!({"type":"json_object"})));
        }
        assert!(
            evaluator_requests[1]
                .messages
                .last()
                .unwrap()
                .content
                .as_text()
                .contains("concise, complete JSON")
        );
    }
}
#[tokio::test]
async fn evaluator_notes_remain_evidence_and_replan_adds_only_executable_task_checks() {
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
            "new_tracker_items": [{
                "description": "Verify the requested bounded search result",
                "outcome": "The task-specific check passes",
                "verify": ["pwd"],
            }],
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
        prompt.contains("smallest claim the evidence supports")
            && prompt.contains("Preserve pre-existing workspace changes")
            && prompt.contains("Generalization notes are observations, not new acceptance criteria")
            && !prompt.contains("Every supplied falsifier must become a verification step")
            && !prompt.contains("arXiv")
    }));
    let tracker = fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("Verify the requested bounded search result"));
    assert!(!tracker.contains("Falsify task-scoped claim"));
    assert!(!tracker.contains("A regression test returns a result outside the requested scope"));

    let contract = fs::read_to_string(workspace.join(".vtcode/tasks/current_contract.md")).expect("contract file");
    assert!(contract.contains("Only code-search changes in this task"));
    assert!(contract.contains("Evidence: The focused code-search regression tests pass"));
    assert!(contract.contains("Falsifier: A regression test returns a result outside the requested scope"));
    assert!(contract.contains("these notes do not add acceptance criteria or executable tracker checks"));
}
