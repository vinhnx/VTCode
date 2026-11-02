use super::*;

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
async fn evaluator_errors_finalize_the_canonical_session_and_restore_runner_state() {
    for malformed_response in [true, false] {
        let temp = TempDir::new().unwrap();
        let session_id = "thread-evaluator-error";
        let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), session_id)).await;
        runner.enable_full_auto(&[tools::TASK_TRACKER.to_string()]).await;
        let (steering_sender, steering_receiver) = tokio::sync::mpsc::unbounded_channel();
        *runner.steering_receiver.lock() = Some(steering_receiver);
        let observed_events = Arc::new(Mutex::new(Vec::new()));
        let observer = Arc::clone(&observed_events);
        runner.set_event_handler(move |event| observer.lock().push(event.clone()));
        let mut responses = vec![
            json_response(planner_response_json("pwd")),
            tool_call_response(tools::TASK_TRACKER, json!({"action":"update", "index":1, "status":"completed"})),
            text_response("The task is complete."),
        ];
        if malformed_response {
            responses.push(text_response("This is not an evaluator JSON response."));
        }
        runner.provider_client = Box::new(QueuedProvider::new(responses));

        let error = Box::pin(runner.execute_task(&task("Evaluator error", "exec-task"), &[]))
            .await
            .unwrap_err();
        let expected = if malformed_response {
            "parse evaluator response"
        } else {
            "evaluator request failed"
        };
        assert!(format!("{error:#}").contains(expected), "{error:#}");
        assert!(runner.steering_receiver.lock().is_some());
        assert!(!steering_sender.is_closed());
        assert!(runner.tool_registry.harness_context_snapshot().task_id.is_none());
        let events = observed_events.lock();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
                .count(),
            1
        );
        let completions: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ThreadEvent::ThreadCompleted(event) => Some(event),
                _ => None,
            })
            .collect();
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].subtype, vtcode_exec_events::ThreadCompletionSubtype::ErrorDuringExecution);
        assert!(completions[0].stop_reason.as_deref().unwrap().contains(expected));
        drop(events);
        let session = workspace_root(&temp).join(".vtcode/sessions").join(session_id);
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(session.join("manifest.json")).unwrap()).unwrap();
        assert_ne!(manifest["status"], "active");
        assert_eq!(manifest["in_turn"], false);
        let persisted = fs::read_to_string(session.join("events.jsonl")).unwrap();
        let events: Vec<serde_json::Value> =
            persisted.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(events.iter().filter(|entry| entry["event"]["type"] == "turn.failed").count(), 1);
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry["event"]["type"] == "thread.completed")
                .count(),
            1
        );
    }
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
    assert_eq!(
        evaluator_request.output_format,
        Some(json!({ "type": "json_object" })),
        "evaluator requests must enable provider JSON mode"
    );
    let evaluator_prompt = evaluator_request
        .messages
        .first()
        .map(|message| message.content.as_text().into_owned())
        .expect("evaluator prompt");
    assert!(evaluator_prompt.contains("Current contract:"));
    assert!(evaluator_prompt.contains("Verification results:"));
    assert!(evaluator_prompt.contains("[PASS] pwd (exit 0)"));
    assert!(evaluator_prompt.contains("contract_fidelity"));
    assert!(evaluator_prompt.contains("Execution evidence (recent bounded tool records)"));
    assert!(evaluator_prompt.contains("Tool call: task_tracker"));
    assert!(!evaluator_prompt.contains("new acceptance criteria"));
    assert!(evaluator_request.system_prompt.as_deref().is_some_and(|prompt| {
        prompt.contains("smallest claim the evidence supports")
            && prompt.contains("falsifier")
            && prompt.contains("generalization_notes")
            && prompt
                .contains("high_severity_findings must be a non-negative integer count (0 when none), never an array")
            && prompt.contains("integer values from 1 through 5")
            && !prompt.contains("arXiv")
    }));
    let evaluator_system = evaluator_request.system_prompt.as_deref().expect("evaluator system prompt");
    assert!(evaluator_system.contains("only for unmet requirements already established by the user's request"));
    assert!(evaluator_system.contains("Identify the requirement and its missing observable check"));
    assert!(evaluator_system.contains("Optional improvements and out-of-scope observations belong in residual_risks"));
    assert!(evaluator_system.contains("must not independently cause failure"));
    assert!(!evaluator_system.contains("discover new acceptance criteria"));

    let planner_request = requests
        .iter()
        .find(|request| harness_role_of(request) == HarnessRole::Planner)
        .expect("planner request");
    let planner_system = planner_request.system_prompt.as_deref().expect("planner system prompt");
    assert_eq!(planner_system.matches("JSON only").count(), 1);
    assert!(
        planner_system.contains("All mandatory checks must be satisfiable together after implementation and cleanup")
    );
    let planner_prompt = planner_request
        .messages
        .first()
        .map(|message| message.content.as_text().into_owned())
        .expect("planner prompt");
    assert!(!planner_prompt.contains("JSON only"));

    for request in [planner_request, evaluator_request] {
        let system = request.system_prompt.as_deref().unwrap();
        assert!(system.contains("Preserve pre-existing workspace changes"));
        assert!(system.contains("do not require an empty git status unless the user explicitly requests a clean tree"));
        assert!(system.contains("A dirty status alone neither proves a regression nor proves preservation"));
        assert!(system.contains("Configured checks can produce build artifacts"));
        assert!(system.contains("report the conflict before implementation"));
        assert!(system.contains("Generalization notes are observations, not new acceptance criteria"));
    }
}
#[cfg(unix)]
#[tokio::test]
async fn smoke_lifecycle_preserves_dirty_user_file_and_verifies_after_cleanup() {
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
    fs::write(workspace.join("user-notes.txt"), "original\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "user-notes.txt"])
            .current_dir(&workspace)
            .output()
            .unwrap()
            .status
            .success()
    );
    let user_edit = "pre-existing user edit, preserve exactly\n";
    fs::write(workspace.join("user-notes.txt"), user_edit).unwrap();
    fs::write(workspace.join("expected.txt"), b"smoke literal\n").unwrap();
    fs::create_dir(workspace.join("target")).unwrap();
    let final_check = "test ! -e target/smoke.txt && test ! -L target/smoke.txt";
    let mut config = VTCodeConfig::default();
    config.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    config.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Off;
    // This fixture explicitly authorizes its cleanup command; production
    // command-policy defaults remain unchanged.
    config.commands.deny_glob.retain(|pattern| pattern != "rm *");
    config.commands.deny_list.retain(|command| command != "rm");
    config.commands.allow_glob.push("rm target/smoke.txt".into());
    config.commands.allow_list.extend(["cmp".into(), "test".into()]);
    config.automation.full_auto.max_turns = 12;
    let mut runner = Box::pin(make_runner(&temp, config, "dirty-smoke-lifecycle")).await;
    runner
        .enable_full_auto(&[tools::EXEC_COMMAND.into(), tools::TASK_TRACKER.into()])
        .await;
    let mut provider = RoleQueuedProvider::new();
    provider
        .planner(json_response(planner_response_json(final_check)))
        .build(tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"git status --porcelain"})))
        .build(tool_call_response(
            tools::EXEC_COMMAND,
            json!({"cmd":"printf 'smoke literal\\n' > target/smoke.txt"}),
        ))
        .build(tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"cmp expected.txt target/smoke.txt"})))
        .build(tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"rm target/smoke.txt"})))
        .build(tool_call_response(tools::TASK_TRACKER, json!({"action":"update","index":1,"status":"completed"})))
        .build(text_response("The task is complete."))
        .evaluator(json_response(evaluator_response_json("pass", "Final absence verified; user edit preserved.", 0)));
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);
    let result = Box::pin(runner.execute_task(
        &task("Create, compare, and remove smoke file; preserve existing user edits", "dirty-smoke"),
        &[],
    ))
    .await
    .unwrap();
    assert_eq!(result.outcome, TaskOutcome::Success, "warnings: {:?}", result.warnings);
    assert!(!runner.tool_registry.completion_verification_pending());
    assert_eq!(fs::read_to_string(workspace.join("user-notes.txt")).unwrap(), user_edit);
    assert_eq!(fs::read(workspace.join("expected.txt")).unwrap(), b"smoke literal\n");
    assert!(fs::symlink_metadata(workspace.join("target/smoke.txt")).is_err());
    let command_exits: Vec<_> = result
        .thread_events
        .iter()
        .filter_map(|event| {
            if let ThreadEvent::ItemCompleted(event) = event
                && let ThreadItemDetails::ToolOutput(output) = &event.item.details
            {
                output.exit_code
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        command_exits,
        vec![0, 0, 0, 0],
        "status, creation, exact comparison, and removal must actually pass"
    );
    let requests = recorded.recorded_requests();
    let evaluator = requests
        .iter()
        .find(|request| harness_role_of(request) == HarnessRole::Evaluator)
        .unwrap();
    let prompt = evaluator.messages[0].content.as_text();
    assert!(prompt.contains("user-notes.txt"));
    assert!(prompt.contains(&format!("[PASS] {final_check} (exit 0)")));
    assert!(
        evaluator
            .system_prompt
            .as_deref()
            .unwrap()
            .contains("comparing against the pre-task baseline")
    );
}
