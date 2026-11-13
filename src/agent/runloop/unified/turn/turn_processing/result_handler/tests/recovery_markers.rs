use super::*;

/// Regression test for TD-015: text containing malformed `<tool_call>` tags
/// that `detect_textual_tool_call` cannot parse must still be caught by the
/// recovery guard via `contains_pseudo_tool_call_markers`.
#[tokio::test]
async fn recovery_non_parseable_tool_call_marker_breaks_turn() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    // Malformed markup: <tool_call> wrapping <function=...> with <parameter=>
    // tags that `detect_textual_tool_call` cannot fully parse, but which
    // clearly represent tool-call intent.
    let malformed = "Since tools are disabled in this recovery pass, here is what I would \
                     apply:\n\
                     <tool_call>\n\
                     <function=apply_patch>\n\
                     <parameter=patch>--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old\n+new\n</parameter=patch>\n\
                     </function=apply_patch>\n\
                     </tool_call>";

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: malformed.to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("malformed tool-call markup should not panic");

    // The guard must fire (Break) even though detect_textual_tool_call
    // cannot parse the payload. After stripping, either clean prose is
    // returned (Completed) or the text was only markup (Blocked) — either
    // way it must not fall through to showing raw markup as a final answer.
    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(_)),
        "recovery with non-parseable tool_call tag should break, not continue"
    );
}
/// Regression test for the approved-plan "no file changes" failure: a
/// tool-free recovery response that bundles a prose preamble with a
/// COMPLETE, parseable `<tool_call>` block must take the
/// contract-violation path (salvage + Blocked) instead of publishing the
/// stripped preamble ("Applying the section rewrite now.") as a final
/// answer that fabricates completion.
#[tokio::test]
async fn recovery_parseable_tool_call_with_preamble_breaks_turn() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let text = "•   I have sufficient evidence from prior reads. Applying the section rewrite now.\
                <tool_call>exec_command<arg_key>cmd\n\
                </arg_key><arg_value>grep -n \"## Why VT Code\" README.md</arg_value></tool_call>";

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: text.to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("parseable tool-call markup should not panic");

    assert!(
        matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
            if reason == RECOVERY_CONTRACT_VIOLATION_REASON
        ),
        "recovery with a complete textual tool call must break with a contract violation, not complete with the preamble"
    );
    assert!(
        !backing.last_history_message_contains("Applying the section rewrite"),
        "stripped preamble must not be published as the final answer"
    );
    let salvaged = backing
        .take_recovery_rejected_synthesis_for_test()
        .expect("violation should record salvaged prose");
    assert!(
        salvaged.contains("Applying the section rewrite"),
        "preamble must be preserved for the labeled fallback, got: {salvaged}"
    );
}
/// Anti-gameability pin: appending a disabled-tools disclosure to an action
/// preamble must not launder a complete tool call into a completion. The
/// retry directive itself contains "tools are disabled", so a disclosure
/// exception would be echoable from history on the very next pass.
#[tokio::test]
async fn recovery_complete_tool_call_with_appended_disclosure_still_breaks() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let text = "Applying the section rewrite now.\
                <tool_call>exec_command<arg_key>cmd</arg_key><arg_value>grep -n x README.md</arg_value></tool_call> \
                (tools were disabled, so this could not run)";

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: text.to_string(),
            reasoning: Vec::new(),
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("appended disclosure must not bypass the violation path");

    assert!(
        matches!(
            outcome,
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
            if reason == RECOVERY_CONTRACT_VIOLATION_REASON
        ),
        "a complete textual tool call with appended disclosure must still break with a contract violation"
    );
}
/// Planning-mode complete calls take the planning violation tail (bounded
/// repair or resumable handoff), never a normal text completion.
#[tokio::test]
async fn recovery_complete_tool_call_in_planning_uses_shared_allowance_before_handoff() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    backing.activate_tool_free_recovery_for_test("loop detector");
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let text = "I have sufficient evidence from prior reads. Applying the section rewrite now. <tool_call>exec_command<arg_key>cmd</arg_key><arg_value>grep -n x README.md</arg_value></tool_call>";
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    for request in 1..=3 {
        let mut ctx = backing.turn_processing_context();
        let model = ctx.config.model.clone();
        assert!(
            ctx.admit_planning_recovery_request(&model)
                .await
                .expect("admit synthesis request")
        );
        assert!(ctx.consume_recovery_pass());
        let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: text.into(),
                reasoning: Vec::new(),
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: request,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: 4,
            tool_repeat_limit: 4,
        })
        .await
        .expect("planning tool-call markup must use bounded recovery");
        if request < 3 {
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        } else {
            assert!(
                matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) if reason == "planning awaiting input")
            );
        }
    }
    assert!(turn_modified_files.is_empty());
    assert_eq!(backing.turn_processing_context().plan_session.recovery.requests_used, 3);
    assert!(backing.last_history_message_contains("Planning needs your input. Saved progress is available."));
    let notices = diagnostics.lock().expect("diagnostics");
    let notice = notices
        .iter()
        .find(|notice| notice.code == "planning.awaiting_input")
        .expect("canonical awaiting-input diagnostic");
    assert!(notice.agent_details.contains("tool-call markup"));
}
