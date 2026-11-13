use super::*;

#[tokio::test]
async fn plan_mode_pseudo_tool_call_markup_is_stripped_and_reprompts_once() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Continue),
        "plan-mode pseudo-tool-call markup should re-prompt instead of ending the turn"
    );

    let assistant_texts: Vec<String> = ctx
        .working_history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().into_owned())
        .collect();
    assert!(
        assistant_texts
            .iter()
            .any(|text| text.contains("I need to inspect the workspace.")),
        "the prose part of the response should be preserved: {assistant_texts:?}"
    );
    assert!(
        assistant_texts.iter().all(|text| !text.contains("<tool_call")),
        "raw tool-call markup must never be stored in history: {assistant_texts:?}"
    );

    let directive_present = ctx
        .working_history
        .iter()
        .any(|message| message.role == uni::MessageRole::System && message.content.as_text().contains("not executed"));
    assert!(directive_present, "a re-prompt directive should be pushed into history");
}
#[tokio::test]
async fn plan_mode_pseudo_tool_call_reprompt_is_bounded() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.enable_planning();
    let mut ctx = backing.turn_processing_context();
    for _ in 0..crate::agent::runloop::unified::planning_workflow_state::MAX_PLAN_PSEUDO_TOOL_CALL_REPROMPTS {
        ctx.plan_session.mark_plan_pseudo_tool_call_reprompt_used();
    }

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(_)),
        "an exhausted reprompt budget must end the turn instead of looping"
    );
    let assistant_texts: Vec<String> = ctx
        .working_history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().into_owned())
        .collect();
    assert!(
        assistant_texts.iter().all(|text| !text.contains("<tool_call")),
        "even with the budget exhausted, raw markup must be stripped from the final answer: {assistant_texts:?}"
    );
}
#[tokio::test]
async fn build_mode_pseudo_tool_call_markup_keeps_existing_behavior() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();

    let outcome = ctx
        .handle_text_response(BROKEN_MARKUP_RESPONSE.to_string(), Vec::new(), None, None, false)
        .await
        .expect("text response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(_)),
        "outside planning, text responses still end the turn (no new reprompt path)"
    );
}
