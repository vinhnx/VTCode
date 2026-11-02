use super::*;

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
