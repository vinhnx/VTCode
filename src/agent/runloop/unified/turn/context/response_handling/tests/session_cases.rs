use super::*;

#[tokio::test]
async fn pseudo_reprompt_queues_bounded_cap_allowance() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    assert!(!ctx.plan_session.bounded_planning_follow_up_allowed());

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");
    assert!(matches!(outcome, TurnHandlerOutcome::Continue), "pseudo markup should schedule a bounded reprompt");
    assert!(
        ctx.plan_session.bounded_planning_follow_up_allowed(),
        "the scheduled reprompt must queue one cap allowance so the next request is not immediately blocked"
    );
}
#[tokio::test]
async fn denied_interview_retry_queues_bounded_cap_allowance() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let mut ctx = backing.turn_processing_context();
    assert!(!ctx.plan_session.bounded_planning_follow_up_allowed());

    let outcome = ctx
        .handle_text_response("Here is a research summary.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");
    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "denied-interview prose should schedule a bounded synthesis retry"
    );
    assert!(
        ctx.plan_session.bounded_planning_follow_up_allowed(),
        "the synthesis retry must queue one cap allowance"
    );
}
#[tokio::test]
async fn explicit_plan_mode_requires_approval_with_full_auto_enabled() {
    use vtcode_ui::tui::app::{
        InlineCommand, InlineEvent, InlineHandle, InlineListSelection, InlineSession, TransientEvent, TransientRequest,
    };

    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let mut session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
        worker: None,
    };
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    event_tx
        .send(InlineEvent::Transient(TransientEvent::Cancelled))
        .expect("cancel approval");
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let events_path = backing.enable_harness_emitter();
    let mut ctx = backing.turn_processing_context();
    ctx.plan_session.mark_explicit_plan_request();
    ctx.handle = &handle;
    ctx.renderer = &mut renderer;
    ctx.session = &mut session;
    ctx.skip_confirmations = true;
    ctx.full_auto = true;
    ctx.working_history
        .push(uni::Message::user("use plan mode, refine @README.md".into()));

    let outcome = ctx
        .handle_text_response(String::new(), Vec::new(), None, Some(EXECUTION_REVISION_PLAN.into()), false)
        .await
        .expect("present explicit plan for approval");
    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
    ));
    assert!(ctx.tool_registry.is_planning_active());
    let mut approval_found = false;
    while let Ok(command) = command_rx.try_recv() {
        if let InlineCommand::ShowTransient { request } = command
            && let TransientRequest::List(list) = *request
            && list
                .items
                .iter()
                .any(|item| item.selection == Some(InlineListSelection::PlanApprovalExecute))
        {
            approval_found = true;
        }
    }
    assert!(approval_found);
    let events = tokio::fs::read_to_string(events_path).await.expect("approval events");
    assert_eq!(events.matches("\"type\":\"plan.approval.requested\"").count(), 1);
    assert!(!events.contains("auto_accept"));
}

#[tokio::test]
async fn explicit_plan_mode_waits_without_inline_approval_ui() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let events_path = backing.enable_harness_emitter();
    let mut renderer = AnsiRenderer::stdout();
    let mut ctx = backing.turn_processing_context();
    ctx.renderer = &mut renderer;
    ctx.plan_session.mark_explicit_plan_request();
    ctx.skip_confirmations = true;
    ctx.full_auto = true;
    let outcome = ctx
        .handle_text_response(String::new(), Vec::new(), None, Some(EXECUTION_REVISION_PLAN.into()), false)
        .await
        .expect("wait for explicit approval");
    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
    ));
    assert!(ctx.tool_registry.is_planning_active());
    assert!(last_final_answer_text(&ctx).contains("awaiting approval"));
    let events = tokio::fs::read_to_string(events_path).await.expect("approval events");
    assert_eq!(events.matches("\"type\":\"plan.approval.requested\"").count(), 1);
    assert!(!events.contains("\"type\":\"plan.approval.resolved\""));
}

#[tokio::test]
async fn session_161453_draft_reaches_repair_then_persisted_approval_request() {
    use vtcode_ui::tui::app::{
        InlineCommand, InlineEvent, InlineHandle, InlineListSelection, InlineSession, TransientEvent, TransientRequest,
    };
    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let mut session = InlineSession {
        handle: handle.clone(),
        events: event_rx,
        worker: None,
    };
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    // Dismiss only after the runtime has emitted the approval overlay.
    event_tx.send(InlineEvent::Transient(TransientEvent::Cancelled)).unwrap();
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.set_skip_confirmations_for_test(false);
    let events_path = backing.enable_harness_emitter();
    let mut ctx = backing.turn_processing_context();
    ctx.handle = &handle;
    ctx.renderer = &mut renderer;
    ctx.session = &mut session;
    ctx.session_stats.record_tool(vtcode_core::config::constants::tools::READ_FILE);
    ctx.plan_session.increment_turns();

    let TurnProcessingResult::TextResponse { text, proposed_plan, .. } =
        process_planning_candidate(&mut ctx, SESSION_161453_PLAN)
    else {
        panic!("a draft must reach validation instead of a fabricated interview");
    };
    assert_eq!(proposed_plan.as_deref(), Some(SESSION_161453_PLAN.trim()));
    assert!(!ctx.plan_session.interview_shown());
    assert!(!validate_plan_content(SESSION_161453_PLAN).is_ready());
    let result = ctx
        .handle_text_response(text, Vec::new(), None, proposed_plan, false)
        .await
        .expect("repair draft");
    assert!(matches!(result, TurnHandlerOutcome::Continue));
    assert!(!persisted_plan_is_ready(&ctx.tool_registry.planning_workflow_state()).await);
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.role == uni::MessageRole::System
                && message.content.as_text().contains("Repair the listed defects")
                && message.content.as_text().contains(CANONICAL_STEP_FORMAT))
    );

    let repaired = "## Summary\nRefine README prose.\n\n## Implementation Steps\n1. Tighten README wording -> files: [README.md] -> verify: [npx markdownlint-cli2 README.md]\n\n## Test Cases and Validation\n- Markdown lint exits zero; review links and the contributor block.\n\n## Assumptions and Defaults\n- Preserve commands and generated content.\n";
    let TurnProcessingResult::TextResponse { text, proposed_plan, .. } = process_planning_candidate(&mut ctx, repaired)
    else {
        panic!("repaired plan must reach the approval gate");
    };
    let result = ctx
        .handle_text_response(text, Vec::new(), None, proposed_plan, false)
        .await
        .expect("persist repaired plan");
    assert!(matches!(
        result,
        TurnHandlerOutcome::Break(TurnLoopResult::Completed { plan_approved_execution_pending: false })
    ));
    assert!(persisted_plan_is_ready(&ctx.tool_registry.planning_workflow_state()).await);
    assert!(last_final_answer_text(&ctx).contains("Plan review ended without starting implementation"));
    assert!(ctx.tool_registry.planning_workflow_state().is_active());
    let events = tokio::fs::read_to_string(events_path).await.expect("approval events");
    assert_eq!(events.matches("\"type\":\"plan.approval.requested\"").count(), 1);
    assert_eq!(events.matches("\"type\":\"plan.approval.resolved\"").count(), 1);
    assert!(!events.contains("auto_accept"));
    let mut approval_overlays = 0;
    while let Ok(command) = command_rx.try_recv() {
        if let InlineCommand::ShowTransient { request } = command {
            let TransientRequest::List(list) = *request else {
                panic!("expected approval list");
            };
            assert!(
                list.items
                    .iter()
                    .any(|item| item.selection == Some(InlineListSelection::PlanApprovalExecute))
            );
            approval_overlays += 1;
        }
    }
    assert_eq!(approval_overlays, 1, "the repaired draft must show exactly one HITL approval overlay");
}
#[tokio::test]
async fn session_161453_repeated_invalid_draft_stops_with_validator_feedback() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    ctx.session_stats.record_tool(vtcode_core::config::constants::tools::READ_FILE);
    ctx.plan_session.increment_turns();
    for attempt in 0..=crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_VALIDATION_REPAIR_REPROMPTS {
        let TurnProcessingResult::TextResponse { text, proposed_plan, .. } =
            process_planning_candidate(&mut ctx, SESSION_161453_PLAN)
        else {
            panic!("draft was replaced with interview");
        };
        let result = ctx
            .handle_text_response(text, Vec::new(), None, proposed_plan, false)
            .await
            .expect("bounded repair");
        if attempt < crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_VALIDATION_REPAIR_REPROMPTS {
            assert!(matches!(result, TurnHandlerOutcome::Continue));
        } else {
            assert_terminal_plan_rejection_is_blocked(&result);
            assert_history_carries_validator_feedback(&last_final_answer_text(&ctx));
        }
    }
    assert!(!persisted_plan_is_ready(&ctx.tool_registry.planning_workflow_state()).await);
}
