use super::*;

#[tokio::test]
async fn recovery_pass_progress_only_text_completes_turn() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let outcome = ctx
        .handle_text_response("Let me try a narrower search next.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("recovery response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!ctx.is_recovery_active());
}
#[tokio::test]
async fn recovery_pass_diagnostic_then_next_step_text_completes_turn() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("turn balancer");
    assert!(ctx.consume_recovery_pass());

    let outcome = ctx
        .handle_text_response(
            "The structural search keeps returning empty results. Let me verify the indexer is working and try with a simpler known pattern:".to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("recovery response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!ctx.is_recovery_active());
}
#[tokio::test]
async fn tool_free_recovery_continuation_intent_text_is_terminal() {
    // Regression guard for the post-tool follow-up infinite loop.
    // During tool-free recovery, even when the text expresses continuation
    // intent AND there is recent tool activity (the exact combination that
    // previously produced a non-relaxed `Continue` via "recent_tool_activity",
    // resetting `consecutive_relaxed_continuations` to 0 and re-enabling
    // tools after `finish_recovery_pass()`), the turn must end. The recovery
    // text IS the final answer; allowing continuation re-enables tools and
    // re-triggers recovery when the follow-up fails again — an infinite
    // cycle no existing bound catches.
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history
        .push(uni::Message::user("run cargo nextest and summarize".to_string()));
    ctx.working_history
        .push(uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "command_session".to_string(),
            "{}".to_string(),
        )]));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "test result: ok".to_string()));
    ctx.activate_recovery("post-tool follow-up failure");
    assert!(ctx.consume_recovery_pass());
    assert!(ctx.recovery_is_tool_free());

    // Sanity: without recovery this exact text+history would continue.
    // "Let me continue analyzing the results." is interim progress
    // (<=800 chars, has "let me" intent clause, no question, no
    // conclusive marker) and recent_tool_activity is true → the raw
    // evaluator returns should_continue=true, is_relaxed_continuation=false.
    assert!(
        evaluate_interim_text_continuation(
            true,
            false,
            ctx.working_history,
            "Let me continue analyzing the results.",
            0,
        )
        .should_continue
    );

    // Under tool-free recovery, the turn loop must override to terminal.
    let outcome = ctx
        .handle_text_response("Let me continue analyzing the results.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("recovery response should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
        "tool-free recovery text with continuation intent must end the turn, \
         not re-enable tools and loop"
    );
    // Recovery is finished (not active), and the turn did not continue.
    assert!(!ctx.is_recovery_active());
    assert!(!ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().trim() == AUTONOMOUS_CONTINUE_DIRECTIVE
    }));
}
#[test]
fn detects_new_intent_prefixes_as_interim() {
    assert!(is_interim_progress_update("I'd like to check the next file before proceeding."));
    assert!(is_interim_progress_update("I want to verify the output of the previous step."));
    assert!(is_interim_progress_update("My next step is to run the full test suite."));
    assert!(is_interim_progress_update("Time to fix the remaining lint warnings."));
    assert!(is_interim_progress_update("Let me now inspect the second module for regressions."));
}
#[test]
fn em_dash_boundary_detected_as_interim() {
    assert!(is_interim_progress_update("First check passed—now let me verify the second component."));
    assert!(has_interim_intent_clause("done with the first task—now i'll move to the next"));
}
#[test]
fn ellipsis_boundary_detected_as_interim() {
    assert!(has_interim_intent_clause("checking results…now i need to update the config"));
    assert!(is_interim_progress_update("Scanning the logs…now let me check for error patterns."));
}
#[test]
fn long_text_with_progressive_request_continues_via_relaxed_path() {
    // User asked for progressive work ("fix"), model responds with long analysis
    // (> 800 chars, non-interim pattern), no tool activity.
    // Should continue via "progressive_relaxed" path.
    let long_analysis_base = "The root cause of this issue is a race condition in the connection \
        pool initialization. When multiple threads attempt to acquire connections \
        simultaneously, the pool's internal counter can overflow. This happens because \
        the increment operation is not atomic. The fix should wrap the counter update \
        in a mutex lock. Additionally, we should consider using atomic operations for \
        better performance. ";
    // Pad to exceed 800 chars
    let padding = "x".repeat(820usize.saturating_sub(long_analysis_base.len()));
    let long_text = format!(
        "{long_analysis_base}{padding} Let me now implement the mutex-based fix in the connection pool module."
    );
    assert!(long_text.len() > 800, "test text must exceed 800-char limit to trigger relaxed path");

    let history = vec![
        uni::Message::user("fix the race condition in connection pool".to_string()),
        uni::Message::assistant("I'll look into it.".to_string()),
    ];

    // Without tool activity, the text is > 800 chars → not interim → hits relaxed path
    // last_user_requested_progressive_work is true → should continue
    assert!(evaluate_interim_text_continuation(false, false, &history, &long_text, 0).should_continue);
}
#[test]
fn long_text_with_tool_activity_and_not_conclusive_continues() {
    let long_analysis = "I've reviewed the output from the linter. There are several \
        warnings in the networking module. The main issues are unused imports and \
        a potential memory leak in the connection handler. The fixes are \
        straightforward: remove the unused imports and add proper cleanup in the \
        deinit method. Let me apply these changes to the affected files now. \
        Starting with the networking module, I'll remove the unused imports and \
        then fix the memory leak in the connection handler. After that, I'll \
        run the linter again to verify the warnings are resolved.";
    assert!(long_analysis.len() > 280, "test text must exceed original 280-char limit");

    let history = vec![
        uni::Message::user("run cargo clippy and fix warnings".to_string()),
        uni::Message::assistant("Running clippy now.".to_string()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "command_session".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
    ];

    assert!(evaluate_interim_text_continuation(true, false, &history, long_analysis, 0).should_continue);
}
#[test]
fn short_completion_after_tool_activity_does_not_continue_via_relaxed_path() {
    let history = vec![
        uni::Message::user("create a simple rust hello world program".to_string()),
        uni::Message::assistant("Let me compile and run it to confirm it works:".to_string()).with_tool_calls(vec![
            uni::ToolCall::function("call_1".to_string(), "command_session".to_string(), "{}".to_string()),
        ]),
        uni::Message::tool_response("call_1".to_string(), "Hello, World!".to_string()),
    ];

    assert!(
        !evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "It works! The program compiled and ran successfully, printing `Hello, World!`.",
            0
        )
        .should_continue
    );
}
#[test]
fn short_completion_after_progressive_request_does_not_continue_via_relaxed_path() {
    let history = vec![
        uni::Message::user("fix the parser regression".to_string()),
        uni::Message::assistant("I'll inspect the parser.".to_string()),
    ];

    assert!(
        !evaluate_interim_text_continuation(
            false,
            false,
            &history,
            "I updated the parser logic and the targeted regression test now passes.",
            0
        )
        .should_continue
    );
}
#[tokio::test]
async fn tool_enabled_recovery_pass_can_continue_after_interim_progress() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history
        .push(uni::Message::user("run cargo fmt and follow up".to_string()));
    ctx.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);
    assert!(ctx.consume_recovery_pass());

    let outcome = ctx
        .handle_text_response(
            "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
                .to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("tool-enabled recovery response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(!ctx.is_recovery_active());
    assert!(ctx.recovery_pass_used());
}
#[tokio::test]
async fn continuing_text_response_is_recorded_as_commentary_phase() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history
        .push(uni::Message::user("fix the parser regression".to_string()));

    let outcome = ctx
        .handle_text_response(
            "Now I need to update the parser branch and rerun the targeted test.".to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("continuing text response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    let last_assistant = ctx
        .working_history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
        .expect("assistant message should be recorded");
    assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::Commentary));
}
#[tokio::test]
async fn completed_text_response_is_recorded_as_final_answer_phase() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history
        .push(uni::Message::user("explain the core loop".to_string()));

    let outcome = ctx
        .handle_text_response(
            "The core loop requests model output, dispatches tool calls, and ends once a final textual answer is produced."
                .to_string(),
            Vec::new(),
            None,
            None,
            false,
        )
        .await
        .expect("completed text response should be handled");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    let last_assistant = ctx
        .working_history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
        .expect("assistant message should be recorded");
    assert_eq!(last_assistant.phase, Some(uni::AssistantPhase::FinalAnswer));
}
