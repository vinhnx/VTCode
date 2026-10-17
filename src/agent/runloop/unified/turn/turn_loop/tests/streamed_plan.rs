use super::*;

#[tokio::test]
async fn streamed_invalid_plan_uses_one_bounded_repair_without_approval_artifacts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::InvalidThenProse,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("invalid streamed plan should finish after bounded repair");

    assert_eq!(calls.load(Ordering::SeqCst), 2, "invalid plans get exactly one repair response");
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message
                    .content
                    .as_text()
                    .contains("Planning recovery: the proposed plan was rejected")
        }),
        "validator feedback must schedule the bounded repair"
    );

    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        !plans_dir.exists() || fs::read_dir(&plans_dir).expect("read plans directory").next().is_none(),
        "invalid streamed plans must not create approval artifacts"
    );

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_))),
        "rejected streamed plans must not publish approval requests"
    );
}

#[tokio::test]
async fn streamed_invalid_plan_repairs_to_valid_plan_and_persists_it() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::InvalidThenValid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("a valid repair should reach the approval handoff");

    assert_eq!(calls.load(Ordering::SeqCst), 2, "one invalid draft should get one repair request");
    assert!(outcome.plan_approved_execution_pending, "the repaired plan should be approval-ready");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System)
            .filter(|message| message.content.as_text().contains("proposed plan was rejected"))
            .count(),
        1,
        "exactly one bounded repair directive should be recorded"
    );
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::Assistant
                && message.content.as_text().contains("<proposed_plan>")
                && message.content.as_text().contains("1. Do the thing")
        }),
        "the rejected draft must remain in assistant history for the repair request"
    );
    assert!(
        history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("proposed plan was rejected")
                && !message.content.as_text().contains("1. Do the thing")
        }),
        "repair directives must not elevate rejected draft text into the system role"
    );

    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    let plan_text = fs::read_dir(&plans_dir)
        .expect("repaired plan should create the plans directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.extension().is_some_and(|extension| extension == "md")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
        })
        .map(|path| fs::read_to_string(path).expect("read repaired plan"))
        .expect("repaired plan should be persisted");
    assert!(plan_text.contains("Preserve the streamed planning handoff."));
    assert!(
        history
            .iter()
            .all(|message| !message.content.as_text().contains("Rejected plan draft:")),
        "recoverable invalid drafts must not be rendered as user-facing warning text"
    );
}

#[tokio::test]
async fn streamed_invalid_plan_repair_is_admitted_after_prior_plain_response() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::ProseThenInvalidThenValid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user(
        "make a plan after the interview fallback".to_string(),
    )];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("a repair after an earlier plain response should reach the approval handoff");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "the invalid plan must get a repair despite the earlier prose response"
    );
    assert!(outcome.plan_approved_execution_pending, "the repaired plan should be approval-ready");
    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        plans_dir.exists()
            && fs::read_dir(&plans_dir)
                .expect("repaired plan should create the plans directory")
                .any(|entry| entry.ok().is_some_and(|entry| {
                    entry.path().extension().is_some_and(|extension| extension == "md")
                        && !entry
                            .path()
                            .file_name()
                            .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
                })),
        "the repair admitted after prior prose must persist its validated plan"
    );
}

#[tokio::test]
async fn streamed_invalid_plan_gets_a_second_repair_before_terminal_fallback() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::InvalidThenInvalidThenProse,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("the bounded second repair should finish the turn");

    assert_eq!(calls.load(Ordering::SeqCst), 3, "two invalid drafts should receive two repair passes");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System)
            .filter(|message| message.content.as_text().contains("proposed plan was rejected"))
            .count(),
        2,
        "the second invalid draft must schedule the second repair"
    );
    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        !plans_dir.exists() || fs::read_dir(&plans_dir).expect("read plans directory").next().is_none(),
        "recoverable invalid drafts must not create approval artifacts"
    );
}

#[tokio::test]
async fn streamed_third_invalid_plan_is_terminal_and_retains_each_rejected_draft_in_assistant_history() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::ThreeInvalid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("the third invalid draft should terminate the bounded repair loop");

    assert!(matches!(&outcome.result, TurnLoopResult::Blocked { reason: Some(reason) }
        if reason == "plan artifact rejected; revise the plan before continuing"));
    assert!(!outcome.plan_approved_execution_pending);
    assert!(outcome.turn_modified_files.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 3, "the third invalid draft must not trigger a fourth request");
    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System)
            .filter(|message| message.content.as_text().contains("proposed plan was rejected"))
            .count(),
        2,
        "only the first two invalid drafts should schedule repairs"
    );
    let rejected_drafts = history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text())
        .filter(|text| text.contains("<proposed_plan>"))
        .count();
    assert_eq!(rejected_drafts, 3, "each rejected draft must remain available in assistant history");
    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    assert!(
        !plans_dir.exists() || fs::read_dir(&plans_dir).expect("read plans directory").next().is_none(),
        "terminally rejected plans must not create approval artifacts"
    );
}

#[tokio::test]
async fn streamed_valid_plan_is_persisted_and_publishes_approval_ready_events() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(StreamedPlanProvider {
        script: StreamedPlanScript::Valid,
        calls: calls.clone(),
    }));

    let mut history = vec![uni::Message::user("make a plan for the streamed handoff".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("valid streamed plan should reach the approval handoff");

    assert_eq!(calls.load(Ordering::SeqCst), 1, "a valid streamed plan must not re-enter synthesis");
    assert!(outcome.plan_approved_execution_pending, "the existing automatic approval route should be selected");
    assert_eq!(
        outcome.pending_plan_execution_target,
        Some(crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget::build(
            PlanExecutionContext::Current,
            true,
        )),
        "automatic approval must retain the typed current-session execution handoff"
    );

    let plans_dir = backing.workspace_path().join(".vtcode").join("plans");
    let plan_text = fs::read_dir(&plans_dir)
        .expect("valid streamed plan should create the plans directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.extension().is_some_and(|extension| extension == "md")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
        })
        .map(|path| fs::read_to_string(path).expect("read persisted streamed plan"))
        .expect("valid streamed plan should be persisted");
    assert!(plan_text.contains("Preserve the streamed planning handoff."));
    assert!(plan_text.contains("src/agent/runloop/unified/ui_interaction_stream.rs"));
    assert!(plan_text.contains("target/release/vtcode --version"));
    assert!(plan_text.contains("cargo nextest run -p vtcode --bin vtcode"));

    let tracker_text = fs::read_dir(&plans_dir)
        .expect("valid streamed plan should create the plans directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".tasks.md"))
        })
        .map(|path| fs::read_to_string(path).expect("read persisted streamed task tracker"))
        .expect("automatic approval should persist a task tracker before execution handoff");
    assert!(tracker_text.contains("Keep the semantic plan"));

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    assert!(
        events.iter().any(|event| matches!(event, ThreadEvent::PlanDelta(delta) if delta.delta.contains("Preserve the streamed planning handoff."))),
        "the persisted plan must flow through the canonical plan delta event"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ThreadEvent::PlanApprovalRequested(_))),
        "the existing approval-ready event path must be published"
    );
}
