use super::*;

#[test]
fn relaxed_continuation_does_not_trigger_for_user_directed_questions() {
    // This is the exact scenario from the infinite loop bug:
    // Agent tries to fetch a URL, both tools fail, agent explains blockers
    // and asks "How would you like to proceed?" - this should NOT trigger
    // the relaxed continuation path.
    let blocker_explanation = "I don't have a direct web-fetch tool available in this session. \
        My available tools are scoped to local file/code operations, and `exec_command` \
        requires explicit safety approval \
        for outbound network requests. A few options: 1. Approve the network call 2. Use a \
        subagent 3. Paste the content. How would you like to proceed? If you want me to fetch \
        it, please confirm and I'll retry with the appropriate sandbox permission.";
    let history =
        vec![
            uni::Message::user("can you fetch https://www.google.com.vn/".to_string()),
            uni::Message::assistant("I'll try to fetch it.".to_string()).with_tool_calls(vec![
                uni::ToolCall::function("call_1".to_string(), "exec_command".to_string(), "{}".to_string()),
            ]),
            uni::Message::tool_response("call_1".to_string(), "Tool preflight validation failed".to_string()),
        ];

    // Should NOT continue - the model is asking the user a question
    assert!(
        !evaluate_interim_text_continuation(true, false, &history, blocker_explanation, 0).should_continue,
        "User-directed questions should not trigger relaxed continuation"
    );
}
#[test]
fn relaxed_continuation_does_not_trigger_for_first_turn_handoff_offer() {
    let repo_overview = checkpoint_shaped_repo_overview();
    let history = vec![
        uni::Message::user("what's in this repo?".to_string()),
        uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "exec_command".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "README summary".to_string()),
    ];

    let decision = evaluate_interim_text_continuation(true, false, &history, repo_overview, 0);

    assert!(
        !decision.should_continue,
        "handoff offers after answering a first-turn question should end the turn"
    );
}
#[tokio::test]
async fn first_turn_handoff_offer_completes_without_continue_directive() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("what's in this repo?".to_string()));
    ctx.working_history
        .push(uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "file_operation".to_string(),
            "{}".to_string(),
        )]));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "README summary".to_string()));

    let outcome = ctx
        .handle_text_response(checkpoint_shaped_repo_overview().to_string(), Vec::new(), None, None, false)
        .await
        .expect("repo overview response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    let last_assistant = ctx
        .working_history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
        .expect("assistant message should be recorded");
    assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::FinalAnswer));
    assert!(!ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().trim() == AUTONOMOUS_CONTINUE_DIRECTIVE
    }));
}
#[test]
fn relaxed_continuation_still_works_for_genuine_interim_progress() {
    // Legitimate interim progress should still trigger continuation
    let interim_text = "I've reviewed the linter output. There are several warnings. \
        Let me apply these changes to the affected files now. Starting with the networking \
        module, I'll remove the unused imports and then fix the memory leak.";
    let history = vec![
        uni::Message::user("run cargo clippy and fix warnings".to_string()),
        uni::Message::assistant("Running clippy now.".to_string()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "command_session".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
    ];

    // Should continue - this is genuine interim progress with no user question
    assert!(
        evaluate_interim_text_continuation(true, false, &history, interim_text, 0).should_continue,
        "Genuine interim progress should still trigger continuation"
    );
}
#[test]
fn relaxed_continuation_stops_after_budget_exhausted() {
    // After MAX_CONSECUTIVE_RELAXED_CONTINuations, relaxed path should stop.
    // Use a long text (> 800 chars) to trigger the relaxed path (not interim progress).
    let long_analysis = "I've reviewed the output from the linter. There are several \
        warnings in the networking module. The main issues are unused imports and \
        a potential memory leak in the connection handler. The fixes are \
        straightforward: remove the unused imports and add proper cleanup in the \
        deinit method. Let me apply these changes to the affected files now. \
        Starting with the networking module, I'll remove the unused imports and \
        then fix the memory leak in the connection handler. After that, I'll \
        run the linter again to verify the warnings are resolved. The key changes \
        involve updating the connection pool initialization and adding proper \
        resource cleanup in the deinitialization path. I will also review the \
        authentication module for similar issues and ensure all error paths are \
        properly handled with appropriate cleanup routines to prevent resource leaks.";
    assert!(long_analysis.len() > 800, "test text must exceed 800-char limit to trigger relaxed path");
    let history = vec![
        uni::Message::user("run cargo clippy and fix warnings".to_string()),
        uni::Message::assistant("Running clippy now.".to_string()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "command_session".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
    ];

    // Should continue with budget = 0
    assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 0).should_continue);

    // Should continue with budget = 1 (still under limit)
    assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 1).should_continue);

    // Should continue with budget = 2 (still under limit)
    assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 2).should_continue);

    // Should NOT continue with budget = 3 (at limit)
    assert!(
        !evaluate_interim_text_continuation(true, false, &history, long_analysis, 3).should_continue,
        "Relaxed continuation should stop after MAX_CONSECUTIVE_RELAXED_CONTINuations"
    );

    // Should NOT continue with budget = 4 (over limit)
    assert!(!evaluate_interim_text_continuation(true, false, &history, long_analysis, 4).should_continue);
}
