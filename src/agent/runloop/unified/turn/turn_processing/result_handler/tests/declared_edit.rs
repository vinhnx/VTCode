use super::*;

fn tracker_list_calls() -> Vec<PreparedAssistantToolCall> {
    vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
        "declare-poll".to_string(),
        "task_tracker".to_string(),
        serde_json::json!({"action": "list"}).to_string(),
    ))]
}

async fn drive_tool_calls(
    backing: &mut TestTurnProcessingBacking,
    tracker: &mut LoopTracker,
    modified: &mut BTreeSet<std::path::PathBuf>,
    assistant_text: &str,
) {
    let mut ctx = backing.turn_processing_context();
    handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::ToolCalls {
            tool_calls: tracker_list_calls(),
            assistant_text: assistant_text.to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: tracker,
        turn_modified_files: modified,
        tool_repeat_limit: 3,
    })
    .await
    .expect("tracker poll dispatches");
}

async fn drive_text(
    backing: &mut TestTurnProcessingBacking,
    tracker: &mut LoopTracker,
    modified: &mut BTreeSet<std::path::PathBuf>,
    text: &str,
) {
    let mut ctx = backing.turn_processing_context();
    handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: text.to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: tracker,
        turn_modified_files: modified,
        tool_repeat_limit: 3,
    })
    .await
    .expect("text dispatches");
}

#[tokio::test]
async fn tool_calls_with_declaration_arms_declared_edit() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_approved_plan_execution_for_test(true);
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();

    drive_tool_calls(&mut backing, &mut tracker, &mut modified, "mapping the card").await;
    assert!(
        backing
            .turn_processing_context()
            .harness_state
            .declared_edit_pending()
            .is_none(),
        "research chatter must not arm enforcement"
    );

    drive_tool_calls(&mut backing, &mut tracker, &mut modified, "now applying the spacing fixes").await;
    let pending = backing
        .turn_processing_context()
        .harness_state
        .declared_edit_pending()
        .expect("declaration arms enforcement");
    assert_eq!(pending.grace_used, 0);
    assert_eq!(pending.declaration.as_str(), "now applying the spacing fixes");
}

#[tokio::test]
async fn declaration_arming_requires_approved_execution() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();

    drive_tool_calls(&mut backing, &mut tracker, &mut modified, "now applying the spacing fixes").await;
    assert!(
        backing
            .turn_processing_context()
            .harness_state
            .declared_edit_pending()
            .is_none(),
        "ordinary turns never arm declared-edit enforcement"
    );
}

#[test]
fn declared_edit_arm_keeps_first_declaration_and_count() {
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 10, 1);
    assert!(state.declared_edit_pending().is_none());
    state.arm_declared_edit("now applying the spacing fixes".to_string());
    state.record_declared_edit_inspection();
    state.record_declared_edit_inspection();
    // A re-declaration never resets the grace: declare/read cycles cannot
    // mint fresh inspection budget.
    state.arm_declared_edit("now applying something else".to_string());
    let pending = state.declared_edit_pending().expect("armed");
    assert_eq!(pending.grace_used, 2);
    assert_eq!(pending.declaration.as_str(), "now applying the spacing fixes");
    state.clear_declared_edit();
    assert!(state.declared_edit_pending().is_none());
}

#[tokio::test]
async fn recovery_synthesis_does_not_lend_the_valve() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_approved_plan_execution_for_test(true);
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();

    drive_tool_calls(&mut backing, &mut tracker, &mut modified, "now applying the spacing fixes").await;
    {
        let mut ctx = backing.turn_processing_context();
        ctx.activate_recovery("loop detector");
        assert!(ctx.consume_recovery_pass());
    }
    drive_text(&mut backing, &mut tracker, &mut modified, "summary of gathered evidence").await;
    assert!(
        backing
            .turn_processing_context()
            .harness_state
            .declared_edit_pending()
            .is_some(),
        "recovery synthesis must not launder a pending declaration"
    );
}

#[tokio::test]
async fn text_answer_without_declaration_clears_pending_edit() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_approved_plan_execution_for_test(true);
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();

    drive_text(&mut backing, &mut tracker, &mut modified, "now applying the spacing fixes").await;
    assert!(
        backing
            .turn_processing_context()
            .harness_state
            .declared_edit_pending()
            .is_some(),
        "text-only declaration arms"
    );

    drive_text(&mut backing, &mut tracker, &mut modified, "checking one more alignment detail first").await;
    assert!(
        backing
            .turn_processing_context()
            .harness_state
            .declared_edit_pending()
            .is_none(),
        "a text-only answer without a declaration is the pressure valve"
    );
}
