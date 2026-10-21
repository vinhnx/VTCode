#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

use super::*;

const COMPLETE_HEADLESS_PLAN: &str = "<proposed_plan>\n## Summary\nUpdate the fixture greeting.\n\n## Implementation Steps\n1. Update greeting -> files: [fixture.txt] -> verify: [rg -n 'greeting' fixture.txt]\n\n## Validation\nThe greeting appears once.\n\n## Assumptions\nKeep unrelated contents unchanged.\n</proposed_plan>";

#[tokio::test]
async fn headless_planning_mixed_failures_exhaust_allowance_or_turns_without_replay() {
    use crate::core::agent::events::SessionStoreSink;
    use crate::exec::events::{
        PlanDeltaEvent,
        planning::{PlanningOutcome, PlanningRecoverySnapshot},
    };
    for max_turns in [2, 5] {
        let temp = TempDir::new().unwrap();
        let session = "planning-mixed";
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.max_turns = max_turns;
        let mut runner = Box::pin(make_runner(&temp, cfg, session)).await;
        runner.enable_planning();
        let store = SessionStoreSink::open(&workspace_root(&temp), session).await.unwrap();
        store
            .persist_checkpoint(&ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
                thread_id: session.into(),
                turn_id: "turn-0".into(),
                item_id: "planning-recovery".into(),
                delta: String::new(),
                recovery: Some(Box::new(PlanningRecoverySnapshot {
                    request_id: Some("task-1".into()),
                    research_closed: true,
                    outcome: PlanningOutcome::Synthesizing,
                    ..Default::default()
                })),
            })))
            .await
            .unwrap();
        store.close().await.unwrap();
        let provider = RecordingQueuedProvider::with_name(
            "queued-test-provider",
            vec![
                text_response(""),
                text_response("Please approve this incomplete draft."),
                tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"touch must-not-execute"})),
                text_response(COMPLETE_HEADLESS_PLAN),
            ],
        );
        let observer = provider.clone();
        runner.provider_client = Box::new(provider);
        let input = task("Plan a greeting", "task-1");
        let result = Box::pin(runner.execute_task(&input, &[])).await.unwrap();
        assert!(!result.outcome.is_success());
        let expected = max_turns.min(3);
        assert_eq!(observer.recorded_requests().len(), expected);
        assert!(
            observer
                .recorded_requests()
                .iter()
                .all(|request| request.tools.is_none() && matches!(request.tool_choice, Some(ToolChoice::None)))
        );
        let store = SessionStoreSink::open(&workspace_root(&temp), session).await.unwrap();
        let saved = store.planning_recovery().await.unwrap().unwrap();
        assert_eq!(usize::from(saved.requests_used), expected);
        assert_eq!(saved.outcome, PlanningOutcome::AwaitingInput);
        store.close().await.unwrap();
        let resumed = Box::pin(runner.execute_task(&input, &[])).await.unwrap();
        assert!(!resumed.outcome.is_success());
        assert_eq!(observer.recorded_requests().len(), expected);
        assert!(!workspace_root(&temp).join("must-not-execute").exists());
    }
}

#[tokio::test]
async fn headless_planning_corrects_each_response_shape_with_research_closed() {
    let mut reasoning = text_response("");
    reasoning.reasoning = Some("private reasoning".into());
    let mut truncated = text_response(COMPLETE_HEADLESS_PLAN);
    truncated.finish_reason = FinishReason::Length;
    for (case, failure) in [
        text_response(""),
        text_response("Approve this incomplete draft."),
        reasoning,
        truncated,
        tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"touch must-not-execute"})),
        text_response("```tool:command_session\n{\"command\":\"touch must-not-execute\"}\n```"),
    ]
    .into_iter()
    .enumerate()
    {
        use crate::core::agent::events::SessionStoreSink;
        use crate::exec::events::{
            PlanDeltaEvent,
            planning::{PlanningOutcome, PlanningRecoverySnapshot},
        };
        let temp = TempDir::new().unwrap();
        let session = "planning-saved-context";
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.max_turns = 4;
        let mut runner = Box::pin(make_runner(&temp, cfg, session)).await;
        runner.enable_planning();
        let store = SessionStoreSink::open(&workspace_root(&temp), session).await.unwrap();
        let snapshot = PlanningRecoverySnapshot {
            request_id: Some("task-1".into()),
            outcome: PlanningOutcome::Repairing,
            research_closed: true,
            requests_used: 1,
            latest_request: "Plan a greeting".into(),
            evidence: (0..8)
                .map(|index| format!("Saved source evidence: unique-greeting-42-{index}"))
                .collect(),
            best_draft: Some(COMPLETE_HEADLESS_PLAN.into()),
            best_draft_defects: Some(0),
            validation_report: Some("Preserve the greeting check and emit the complete draft.".into()),
            ..Default::default()
        };
        store
            .persist_checkpoint(&ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
                thread_id: session.into(),
                turn_id: "turn-1".into(),
                item_id: "planning-recovery".into(),
                delta: String::new(),
                recovery: Some(Box::new(snapshot)),
            })))
            .await
            .unwrap();
        store.close().await.unwrap();
        let provider = RecordingQueuedProvider::with_name(
            "queued-test-provider",
            vec![failure, text_response(COMPLETE_HEADLESS_PLAN)],
        );
        let observer = provider.clone();
        runner.provider_client = Box::new(provider);
        let result = Box::pin(runner.execute_task(&task("Plan a greeting", "task-1"), &[]))
            .await
            .unwrap();
        assert!(result.outcome.is_success(), "case {case}: {:?}, {}", result.outcome, result.summary);
        let requests = observer.recorded_requests();
        assert_eq!(requests.len(), 2, "resume uses only the remaining correction allowance");
        assert!(
            requests
                .iter()
                .all(|request| request.tools.is_none() && matches!(request.tool_choice, Some(ToolChoice::None)))
        );
        assert!(matches!(requests[0].tool_choice, Some(ToolChoice::None)));
        assert!(requests[0].tools.is_none());
        let context = requests[0]
            .messages
            .iter()
            .map(|m| m.content.as_text().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(context.contains("unique-greeting-42"), "saved evidence must reach the provider");
        assert!(context.contains("Update the fixture greeting"), "saved draft must reach the provider");
        assert!(context.contains("Preserve the greeting check"));
        let store = SessionStoreSink::open(&workspace_root(&temp), session).await.unwrap();
        let saved = store.planning_recovery().await.unwrap().unwrap();
        assert_eq!(saved.requests_used, 3);
        assert_eq!(saved.outcome, PlanningOutcome::AwaitingApproval);
        assert_eq!(saved.evidence.len(), 8, "approval must preserve bounded saved evidence");
        assert!(saved.evidence.iter().all(|item| item.len() <= 2048));
        assert!(saved.evidence.iter().map(String::len).sum::<usize>() <= 12 * 1024);
        store.close().await.unwrap();
        assert!(!workspace_root(&temp).join("must-not-execute").exists());
    }
}

#[tokio::test]
async fn headless_planning_scopes_saved_drafts_and_input_handoffs_to_the_task() {
    for (ready, next_id) in [(false, "task-1"), (true, "task-1"), (false, "task-2"), (true, "task-2")] {
        let temp = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.max_turns = 1;
        let mut runner = Box::pin(make_runner(&temp, cfg, "planning-new-task")).await;
        runner.enable_planning();
        let next_plan = COMPLETE_HEADLESS_PLAN.replace("greeting", "farewell");
        let provider = RecordingQueuedProvider::with_name(
            "queued-test-provider",
            vec![
                text_response(if ready { COMPLETE_HEADLESS_PLAN } else { "" }),
                text_response(&next_plan),
            ],
        );
        let observer = provider.clone();
        runner.provider_client = Box::new(provider);
        let first = Box::pin(runner.execute_task(&task("Plan a greeting", "task-1"), &[]))
            .await
            .unwrap();
        assert_eq!(first.outcome.is_success(), ready);
        let replay = Box::pin(runner.execute_task(&task("Plan a greeting", "task-1"), &[]))
            .await
            .unwrap();
        assert_eq!(replay.outcome.is_success(), ready);
        assert_eq!(observer.recorded_requests().len(), 1, "same task resume reuses its persisted draft");
        let next = Box::pin(runner.execute_task(&task("Plan a farewell", next_id), &[]))
            .await
            .unwrap();
        assert!(next.outcome.is_success());
        assert_eq!(observer.recorded_requests().len(), 2, "a new task requires its own plan");
        assert!(next.summary.contains("farewell"));
        let store = crate::core::agent::events::SessionStoreSink::open(&workspace_root(&temp), "planning-new-task")
            .await
            .unwrap();
        let saved = store.planning_recovery().await.unwrap().unwrap();
        assert_eq!(
            saved.cycle,
            u32::from(next_id == "task-1"),
            "only explicit same-task input renews the correction cycle"
        );
        assert!(saved.research_closed);
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn headless_planning_returns_the_actual_plan_after_distinct_shell_inspections() {
    let temp = TempDir::new().unwrap();
    let original = (1..=18).map(|n| format!("line {n}\n")).collect::<String>();
    fs::write(workspace_root(&temp).join("fixture.txt"), &original).unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.automation.full_auto.max_turns = 24;
    cfg.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Off;
    let mut runner = Box::pin(make_runner(&temp, cfg, "planning-distinct-reads")).await;
    runner.enable_full_auto(&[tools::EXEC_COMMAND.into()]).await;
    runner.enable_planning();
    let plan = COMPLETE_HEADLESS_PLAN;
    let mut responses: Vec<_> = (1..=18)
        .map(|n| tool_call_response(tools::EXEC_COMMAND, json!({"cmd":format!("sed -n '{n}p' fixture.txt")})))
        .collect();
    responses.push(text_response(plan));
    let provider = QueuedProvider::new(responses);
    let remaining = provider.responses.clone();
    runner.provider_client = Box::new(provider);
    let result =
        Box::pin(runner.execute_task(&task("Inspect fixture and return a plan", "planning-distinct-reads"), &[]))
            .await
            .unwrap();
    assert_eq!(result.outcome, TaskOutcome::Success);
    assert_eq!(result.turns_executed, 19);
    assert!(remaining.lock().is_empty());
    assert!(result.modified_files.is_empty());
    assert_eq!(fs::read_to_string(workspace_root(&temp).join("fixture.txt")).unwrap(), original);
    let terminal = result
        .thread_events
        .iter()
        .find_map(|event| {
            if let ThreadEvent::ThreadCompleted(event) = event {
                Some(event)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(terminal.result.as_deref(), Some(plan));
    let last_message = result.thread_events.iter().rev().find_map(|event| {
        if let ThreadEvent::ItemCompleted(event) = event
            && let ThreadItemDetails::AgentMessage(message) = &event.item.details
        {
            Some(message.text.as_str())
        } else {
            None
        }
    });
    assert_eq!(last_message, Some(plan));
}

#[tokio::test]
async fn headless_planning_reserves_the_last_turn_for_synthesis_and_rejects_more_tools() {
    for (case, (final_response, succeeds)) in [
        (text_response(COMPLETE_HEADLESS_PLAN), true),
        (text_response(" \n\t "), false),
        (tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"cat missing-native"})), false),
        (text_response("```command_session\n{\"cmd\":\"cat missing-text\"}\n```"), false),
    ]
    .into_iter()
    .enumerate()
    {
        let temp = TempDir::new().unwrap();
        fs::write(workspace_root(&temp).join("fixture.txt"), "original\n").unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.max_turns = 2;
        cfg.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Off;
        let mut runner = Box::pin(make_runner(&temp, cfg, "planning-budget")).await;
        runner.enable_full_auto(&[tools::EXEC_COMMAND.into()]).await;
        runner.enable_planning();
        let provider = RecordingQueuedProvider::with_name(
            "queued-test-provider",
            vec![
                tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"cat fixture.txt"})),
                final_response,
            ],
        );
        let observer = provider.clone();
        runner.provider_client = Box::new(provider);
        let result = Box::pin(runner.execute_task(&task("Return a plan", "planning-budget"), &[]))
            .await
            .unwrap();
        let requests = observer.recorded_requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].tools.as_ref().is_some_and(|tools| !tools.is_empty()));
        assert!(requests[1].tools.as_ref().is_none_or(|tools| tools.is_empty()));
        assert!(matches!(requests[1].tool_choice, Some(ToolChoice::None)));
        assert!(
            requests[1]
                .messages
                .last()
                .unwrap()
                .content
                .as_text()
                .contains("tool-free planning synthesis")
        );
        assert_eq!(result.turns_executed, 2);
        assert_eq!(result.outcome == TaskOutcome::Success, succeeds, "case {case}: {:?}", result.outcome);
        if !succeeds {
            assert_eq!(result.outcome.code(), "turn_limit_reached");
        }
        let dispatched = result.thread_events.iter().filter(|event| matches!(
            event,
            ThreadEvent::ItemCompleted(event)
                if matches!(&event.item.details, ThreadItemDetails::ToolInvocation(item) if item.tool_name == tools::EXEC_COMMAND)
        )).count();
        assert_eq!(dispatched, 1, "synthesis cannot execute additional tools");
        assert_eq!(fs::read_to_string(workspace_root(&temp).join("fixture.txt")).unwrap(), "original\n");
    }
}

#[tokio::test]
async fn headless_planning_rewrites_truncated_plans_within_shared_allowance_without_tools() {
    for truncate_rewrite in [false, true] {
        let temp = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.max_turns = 4;
        let mut runner = Box::pin(make_runner(&temp, cfg, "planning-length")).await;
        runner.enable_full_auto(&[tools::EXEC_COMMAND.into()]).await;
        runner.enable_planning();
        let mut truncated = text_response("Task complete. Partial plan: inspect the source, then");
        truncated.finish_reason = FinishReason::Length;
        let plan = COMPLETE_HEADLESS_PLAN;
        let final_response = if truncate_rewrite {
            truncated.clone()
        } else {
            text_response(plan)
        };
        let provider = RecordingQueuedProvider::with_name(
            "queued-test-provider",
            if truncate_rewrite {
                vec![
                    truncated,
                    final_response.clone(),
                    final_response.clone(),
                    final_response,
                ]
            } else {
                vec![truncated, final_response]
            },
        );
        let observer = provider.clone();
        runner.provider_client = Box::new(provider);
        let result = Box::pin(runner.execute_task(&task("Return a plan", "planning-length"), &[]))
            .await
            .unwrap();
        let requests = observer.recorded_requests();
        assert_eq!(requests.len(), if truncate_rewrite { 4 } else { 2 });
        assert_eq!(requests[0].max_tokens, Some(4096), "planning needs the existing synthesis output allowance");
        assert!(requests[0].tools.as_ref().is_some_and(|tools| !tools.is_empty()));
        assert!(requests[1].tools.as_ref().is_none_or(|tools| tools.is_empty()));
        assert_eq!(requests[1].max_tokens, requests[0].max_tokens);
        assert!(
            requests[1]
                .messages
                .last()
                .unwrap()
                .content
                .as_text()
                .contains("shorten detail")
        );
        assert_eq!(result.outcome == TaskOutcome::Success, !truncate_rewrite);
        assert_eq!(result.turns_executed, if truncate_rewrite { 4 } else { 2 });
        let terminal = result
            .thread_events
            .iter()
            .find_map(|event| {
                if let ThreadEvent::ThreadCompleted(event) = event {
                    Some(event)
                } else {
                    None
                }
            })
            .unwrap();
        if truncate_rewrite {
            assert!(terminal.result.is_none());
            assert!(result.summary.contains("output token limit"));
        } else {
            assert_eq!(terminal.result.as_deref(), Some(plan));
        }
    }
}

#[tokio::test]
async fn headless_planning_exhaustion_emits_runtime_input_actions_and_replays_without_requests() {
    use crate::core::agent::events::SessionStoreSink;
    use crate::exec::events::planning::{PlanningOutcome, PlanningResponseClass};
    for (case, (final_response, expected_class)) in [
        (text_response(""), PlanningResponseClass::EmptyContent),
        (
            {
                let mut response = text_response("");
                response.reasoning = Some("private reasoning must never be retained".into());
                response
            },
            PlanningResponseClass::ReasoningOnly,
        ),
        (text_response("The plan is ready. Approve it to continue."), PlanningResponseClass::MalformedPlan),
        (
            tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"cat must-not-execute"})),
            PlanningResponseClass::NativeToolCalls,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let temp = TempDir::new().unwrap();
        let workspace = workspace_root(&temp);
        fs::write(workspace.join("fixture.txt"), "public evidence\n").unwrap();
        let session = format!("planning-input-{case}");
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.max_turns = 2;
        cfg.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Off;
        let mut runner = Box::pin(make_runner(&temp, cfg, &session)).await;
        runner.enable_full_auto(&[tools::EXEC_COMMAND.into()]).await;
        runner.enable_planning();
        let provider = RecordingQueuedProvider::with_name(
            "queued-test-provider",
            vec![
                tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"cat fixture.txt"})),
                final_response,
            ],
        );
        let observer = provider.clone();
        runner.provider_client = Box::new(provider);
        let result = Box::pin(runner.execute_task(&task("Return a complete plan", &session), &[]))
            .await
            .unwrap();
        assert!(!result.outcome.is_success());
        assert_eq!(observer.recorded_requests().len(), 2);
        let diagnostics: Vec<_> = result
            .thread_events
            .iter()
            .filter_map(|event| {
                let ThreadEvent::ItemUpdated(event) = event else {
                    return None;
                };
                let ThreadItemDetails::Reasoning(item) = &event.item.details else {
                    return None;
                };
                item.diagnostic
                    .as_deref()
                    .filter(|diagnostic| diagnostic.code == "planning.awaiting_input")
            })
            .collect();
        assert_eq!(diagnostics.len(), 1, "one incident per exhausted planning cycle");
        assert_eq!(
            diagnostics[0]
                .actions
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            vec!["retry", "clarify", "model"]
        );
        assert!(result.summary.contains("Details:"));
        assert!(result.summary.contains("Retry from retained evidence"));
        assert!(!result.summary.contains("private reasoning must never be retained"));
        let store = SessionStoreSink::open(&workspace, &session).await.unwrap();
        let snapshot = store.planning_recovery().await.unwrap().unwrap();
        assert_eq!(snapshot.outcome, PlanningOutcome::AwaitingInput);
        assert_eq!(snapshot.response_class, Some(expected_class));
        assert_eq!(snapshot.requests_used, 1);
        assert!(snapshot.research_closed);
        assert!(snapshot.evidence.iter().any(|evidence| evidence.contains("public evidence")));
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("private reasoning must never be retained")
        );
        store.close().await.unwrap();
        let resumed = Box::pin(runner.execute_task(&task("Return a complete plan", &session), &[]))
            .await
            .unwrap();
        assert!(!resumed.outcome.is_success());
        assert_eq!(observer.recorded_requests().len(), 2, "resume cannot reopen research or replenish requests");
    }
}

#[tokio::test]
async fn headless_planning_requires_artifact_persistence_before_awaiting_approval() {
    use crate::core::agent::events::SessionStoreSink;
    use crate::exec::events::planning::PlanningOutcome;
    for persistable in [true, false] {
        let temp = TempDir::new().unwrap();
        let workspace = workspace_root(&temp);
        let session = format!("planning-persistence-{persistable}");
        let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), &session)).await;
        runner.enable_full_auto(&[]).await;
        runner.enable_planning();
        if !persistable {
            let plans = workspace.join(".vtcode/plans");
            fs::create_dir_all(workspace.join(".vtcode")).unwrap();
            if plans.is_dir() {
                fs::remove_dir_all(&plans).unwrap();
            }
            fs::write(plans, "not a directory").unwrap();
        }
        runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(COMPLETE_HEADLESS_PLAN)]));
        let result = Box::pin(runner.execute_task(&task("Return a complete plan", &session), &[]))
            .await
            .unwrap();
        assert_eq!(result.outcome == TaskOutcome::Success, persistable);
        let store = SessionStoreSink::open(&workspace, &session).await.unwrap();
        let snapshot = store.planning_recovery().await.unwrap().unwrap();
        assert_eq!(
            snapshot.outcome,
            if persistable {
                PlanningOutcome::AwaitingApproval
            } else {
                PlanningOutcome::AwaitingInput
            }
        );
        assert!(snapshot.best_draft.is_some());
        store.close().await.unwrap();
        if persistable {
            let file = runner.tool_registry.planning_workflow_state().get_plan_file().await.unwrap();
            assert!(fs::read_to_string(file).unwrap().contains("## Implementation Steps"));
        }
        assert!(runner.tool_registry.is_planning_active(), "neither handoff approves implementation");
    }
}

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

#[tokio::test]
async fn failed_or_scope_changing_replan_does_not_promote_falsifiers_to_commands() {
    use super::super::evaluator_types::GeneralizationNote;
    use super::super::orchestration::EvaluationArtifacts;

    for scope_changing_response in [false, true] {
        let temp = TempDir::new().unwrap();
        let workspace = workspace_root(&temp);
        let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "notes-only-replan")).await;
        seed_tracker(
            &workspace,
            json!([{
                "description":"Confirm the smoke path is absent",
                "verify":["test ! -e target/smoke.txt"],
            }]),
        )
        .await;
        let tracker_path = workspace.join(".vtcode/tasks/current_task.md");
        let before = fs::read_to_string(&tracker_path).unwrap();
        let evaluation = EvaluationArtifacts {
            generalization_notes: vec![GeneralizationNote {
                claim: "A destructive counterexample would invalidate this observation".into(),
                scope: "Only the approved smoke path".into(),
                evidence: "The smoke path was removed".into(),
                falsifier: "touch forbidden.txt".into(),
            }],
            evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
            passed: false,
            summary: "Clarify the existing evidence limitation".into(),
            required_tracker_updates: vec![],
            unmet_contract_items: vec![],
        };
        let responses = if scope_changing_response {
            vec![json_response(json!({
                "preserved_scopes":["Any workspace file"],
                "new_tracker_items":[{"description":"Out of scope", "verify":["touch forbidden.txt"]}],
            }))]
        } else {
            vec![]
        };
        runner.provider_client = Box::new(QueuedProvider::new(responses));
        runner
            .replan_from_failure(&task("Preserve smoke-test scope", "notes-only"), &evaluation, 1)
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(&tracker_path).unwrap(), before);
        assert!(!workspace.join("forbidden.txt").exists());
        let contract = fs::read_to_string(workspace.join(".vtcode/tasks/current_contract.md")).unwrap();
        assert!(contract.contains("Only the approved smoke path"));
        assert!(contract.contains("Falsifier: touch forbidden.txt"));
        assert!(contract.contains("these notes do not add acceptance criteria"));
    }
}

#[tokio::test]
async fn replan_retains_required_updates_and_executable_item_metadata() {
    use super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "replan-metadata")).await;
    seed_tracker(&workspace, json!(["Original requirement"])).await;
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Check the final state".into(),
        required_tracker_updates: vec!["Preserve the existing user edit".into()],
        unmet_contract_items: vec!["Final absence is unverified".into()],
    };
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(json!({
        "new_tracker_items":[{
            "description":"Confirm final absence",
            "outcome":"No file or symlink remains",
            "files":["target/smoke.txt"],
            "verify":["test ! -e target/smoke.txt && test ! -L target/smoke.txt"],
        }],
    }))]));
    runner
        .replan_from_failure(&task("Final smoke state", "metadata"), &evaluation, 1)
        .await
        .unwrap();
    let tool = TaskTrackerTool::new(workspace, runner.tool_registry.planning_workflow_state());
    let listed = tool.execute(json!({"action":"list"})).await.unwrap();
    let items = listed["checklist"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["description"], "Original requirement");
    assert_eq!(items[1]["description"], "Preserve the existing user edit");
    assert_eq!(items[2]["description"], "Confirm final absence");
    assert_eq!(items[2]["outcome"], "No file or symlink remains");
    assert_eq!(items[2]["files"], json!(["target/smoke.txt"]));
    assert_eq!(items[2]["verify"], json!(["test ! -e target/smoke.txt && test ! -L target/smoke.txt"]));
    assert_eq!(items[2]["status"], "pending");
}

#[tokio::test]
async fn replan_tracker_updates_are_idempotent_and_enrich_only_uncompleted_placeholders() {
    use super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "deduplicated-replan")).await;
    seed_tracker(&workspace, json!([
        {"description":"Finished requirement", "status":"completed", "files":["first.rs"], "outcome":"First contract held", "verify":["check-first"]},
        {"description":"Check final absence", "status":"in_progress"}
    ])).await;
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Confirm remaining requirements".into(),
        required_tracker_updates: vec![
            "Finished requirement".into(),
            "Check final absence".into(),
            "New requirement".into(),
            "New requirement".into(),
        ],
        unmet_contract_items: vec![],
    };
    let replan = json!({"new_tracker_items":[
        {"description":"Finished requirement", "files":["first.rs"], "verify":["check-first"]},
        {"description":"Check final absence", "outcome":"No file remains", "verify":["test ! -e target/smoke"]},
        {"description":"New requirement", "files":["second.rs"], "verify":["check-second"]},
        {"description":"New requirement", "files":["second.rs"], "verify":["check-second"]}
    ]});
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(replan.clone()), json_response(replan)]));
    let tracker_path = workspace.join(".vtcode/tasks/current_task.md");
    let mut previous = None;
    for round in 1..=2 {
        runner
            .replan_from_failure(&task("Verify final state", "deduplicated"), &evaluation, round)
            .await
            .unwrap();
        let contents = fs::read_to_string(&tracker_path).unwrap();
        if let Some(before) = previous.replace(contents.clone()) {
            assert_eq!(contents, before, "a repeated replan must not rewrite progress or duplicate requirements");
        }
        let tool = TaskTrackerTool::new(workspace.clone(), runner.tool_registry.planning_workflow_state());
        let listed = tool.execute(json!({"action":"list"})).await.unwrap();
        let items = listed["checklist"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["status"], "completed");
        assert_eq!(items[0]["outcome"], "First contract held");
        assert_eq!(items[0]["verify"], json!(["check-first"]));
        assert_eq!(items[1]["status"], "in_progress");
        assert_eq!(items[1]["outcome"], "No file remains");
        assert_eq!(items[1]["verify"], json!(["test ! -e target/smoke"]));
        assert_eq!(items[2]["status"], "pending");
        assert_eq!(items[2]["files"], json!(["second.rs"]));
        assert_eq!(items[2]["verify"], json!(["check-second"]));
    }
}

#[tokio::test]
async fn replan_tracker_preserves_different_criteria_and_completed_placeholders() {
    use super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "distinct-replan")).await;
    seed_tracker(
        &workspace,
        json!([
            {"description":"Verify result", "status":"completed", "verify":["check-old"]},
            {"description":"Already finished", "status":"completed"}
        ]),
    )
    .await;
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Verify the new criteria".into(),
        required_tracker_updates: vec![],
        unmet_contract_items: vec![],
    };
    let replan = json!({"new_tracker_items":[
        {"description":"Verify result", "verify":["check-new"]},
        {"description":"Already finished", "verify":["check-details"]}
    ]});
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(replan.clone()), json_response(replan)]));
    for round in 1..=2 {
        runner
            .replan_from_failure(&task("Verify changed criteria", "distinct"), &evaluation, round)
            .await
            .unwrap();
    }
    let tool = TaskTrackerTool::new(workspace, runner.tool_registry.planning_workflow_state());
    let listed = tool.execute(json!({"action":"list"})).await.unwrap();
    let items = listed["checklist"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0]["status"], "completed");
    assert_eq!(items[0]["verify"], json!(["check-old"]));
    assert_eq!(items[1]["status"], "completed");
    assert_eq!(items[1]["verify"], json!([]));
    assert_eq!(items[2]["status"], "pending");
    assert_eq!(items[2]["verify"], json!(["check-new"]));
    assert_eq!(items[3]["status"], "pending");
    assert_eq!(items[3]["verify"], json!(["check-details"]));
}

#[tokio::test]
async fn replan_tracker_enrichment_preserves_hierarchical_addresses_and_ambiguous_rows() {
    use super::super::orchestration::EvaluationArtifacts;

    let temp = TempDir::new().unwrap();
    let workspace = workspace_root(&temp);
    let mut runner = Box::pin(make_runner(&temp, VTCodeConfig::default(), "hierarchical-replan")).await;
    let state = runner.tool_registry.planning_workflow_state();
    fs::create_dir_all(state.plans_dir()).unwrap();
    let plan = state.plans_dir().join("replan.md");
    fs::write(&plan, "# Replan\n").unwrap();
    state.set_plan_file(Some(plan)).await;
    state.enable();
    let tool = TaskTrackerTool::new(workspace.clone(), state);
    tool.execute(json!({"action":"create", "items":[
        "Parent", "  [~] Unique child", "Repeated description", "Repeated description"
    ]}))
    .await
    .unwrap();
    let evaluation = EvaluationArtifacts {
        generalization_notes: vec![],
        evaluation_path: workspace.join(".vtcode/tasks/current_evaluation.md"),
        passed: false,
        summary: "Add concrete checks".into(),
        required_tracker_updates: vec![],
        unmet_contract_items: vec![],
    };
    let replan = json!({"new_tracker_items":[
        {"description":"Unique child", "verify":["check-child"]},
        {"description":"Repeated description", "verify":["check-distinct"]}
    ]});
    runner.provider_client = Box::new(QueuedProvider::new(vec![json_response(replan.clone()), json_response(replan)]));
    for round in 1..=2 {
        runner
            .replan_from_failure(&task("Verify hierarchy", "hierarchy"), &evaluation, round)
            .await
            .unwrap();
    }
    let listed = tool.execute(json!({"action":"list"})).await.unwrap();
    let items = listed["checklist"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 5);
    assert_eq!(items[1]["index_path"], "1.1");
    assert_eq!(items[1]["status"], "in_progress");
    assert_eq!(items[1]["verify"], json!(["check-child"]));
    assert_eq!(items[2]["verify"], json!([]));
    assert_eq!(items[3]["verify"], json!([]));
    assert_eq!(items[4]["status"], "pending");
    assert_eq!(items[4]["verify"], json!(["check-distinct"]));
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

#[tokio::test]
async fn headless_changed_code_cannot_skip_verification_with_continuation_off_or_neutral_text() {
    for verifier in [None, Some("rustc --version")] {
        let temp = TempDir::new().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
        cfg.agent.harness.continuation_policy = vtcode_config::core::agent::ContinuationPolicy::Off;
        cfg.agent.harness.verification.default_verifier_override = verifier.map(str::to_owned);
        cfg.automation.full_auto.max_turns = 3;
        let mut runner = Box::pin(make_runner(&temp, cfg, "short-edit")).await;
        runner.enable_full_auto(&[tools::APPLY_PATCH.into()]).await;
        runner.tool_registry.allow_all_tools().await.unwrap();
        let provider = RecordingQueuedProvider::new(vec![
            tool_call_response(
                tools::APPLY_PATCH,
                json!({"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"}),
            ),
            text_response("Applied the requested change."),
        ]);
        runner.provider_client = Box::new(provider.clone());
        let result = Box::pin(runner.execute_task(&task("One code edit", "exec-task"), &[]))
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{error}; requests: {:?}",
                    provider
                        .recorded_requests()
                        .iter()
                        .map(|request| &request.messages)
                        .collect::<Vec<_>>()
                )
            });
        assert!(workspace_root(&temp).join("main.rs").exists(), "{:?}", result.outcome);
        assert_eq!(result.outcome.is_success(), verifier.is_some(), "{:?}", result.outcome);
        if verifier.is_none() {
            assert!(result.outcome.description().contains("Unverified"));
        }
    }
}

#[tokio::test]
async fn headless_completion_reuses_a_fresh_model_run_check() {
    let temp = TempDir::new().unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    cfg.agent.harness.continuation_policy = vtcode_config::core::agent::ContinuationPolicy::Off;
    let mut runner = Box::pin(make_runner(&temp, cfg, "reuse-verifier")).await;
    runner
        .enable_full_auto(&[tools::APPLY_PATCH.into(), tools::EXEC_COMMAND.into()])
        .await;
    runner.tool_registry.allow_all_tools().await.unwrap();
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        tool_call_response(
            tools::APPLY_PATCH,
            json!({"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"}),
        ),
        tool_call_response(tools::EXEC_COMMAND, json!({"cmd":"rustc --version"})),
        text_response("The task is complete."),
    ]));
    let result = Box::pin(runner.execute_task(&task("Reuse verifier", "exec-task"), &[]))
        .await
        .unwrap();
    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(!harness_events(&result).contains(&HarnessEventKind::VerificationStarted));
    assert!(workspace_root(&temp).join("main.rs").exists());
}

#[tokio::test]
async fn headless_completion_verification_allows_one_corrective_pass_then_hands_off() {
    let temp = TempDir::new().unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.orchestration_mode = vtcode_config::core::agent::HarnessOrchestrationMode::Single;
    cfg.agent.harness.continuation_policy = vtcode_config::core::agent::ContinuationPolicy::Off;
    cfg.agent.harness.verification.default_verifier_override = Some("rustc --invalid-flag-xyz".into());
    cfg.automation.full_auto.max_turns = 8;
    let mut runner = Box::pin(make_runner(&temp, cfg, "bounded-verification")).await;
    runner
        .enable_full_auto(&[tools::APPLY_PATCH.into(), tools::EXEC_COMMAND.into()])
        .await;
    runner.tool_registry.allow_all_tools().await.unwrap();
    let provider = RecordingQueuedProvider::new(vec![
        tool_call_response(
            tools::APPLY_PATCH,
            json!({"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"}),
        ),
        text_response("Applied the change."),
        text_response("The correction is complete."),
        text_response("This third completion must never run."),
    ]);
    runner.provider_client = Box::new(provider.clone());
    let result = Box::pin(runner.execute_task(&task("Bound completion verification", "exec-task"), &[]))
        .await
        .unwrap();
    assert!(!result.outcome.is_success(), "{:?}", result.outcome);
    assert_eq!(provider.recorded_requests().len(), 3, "one correction after the first completion attempt");
    assert!(result.outcome.description().contains("rustc --invalid-flag-xyz"));
    assert!(result.outcome.description().contains("exit code 1"));
    assert!(
        result
            .outcome
            .description()
            .to_ascii_lowercase()
            .contains("unrecognized option")
    );
    assert!(workspace_root(&temp).join("main.rs").exists(), "failed verification retains edits");
}
