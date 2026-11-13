use super::*;

#[test]
fn interim_progress_detection_requires_non_conclusive_intent_text() {
    assert!(is_interim_progress_update("Let me fix the second collapsible if statement:"));
    assert!(is_interim_progress_update(
        "Let me fix the second collapsible if statement in the Anthropic provider:"
    ));
    assert!(is_interim_progress_update(
        "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:"
    ));
    assert!(is_interim_progress_update("I'll continue with the next fix."));
    assert!(is_interim_progress_update(
        "Running formatter now, then I'll do a quick follow-up check (`cargo check`) to confirm nothing regressed."
    ));
    assert!(is_interim_progress_update(
        "The structural search keeps returning empty results. Let me verify the indexer is working and try with a simpler known pattern:"
    ));
    assert!(!is_interim_progress_update("I need you to choose which option to apply."));
    assert!(!is_interim_progress_update("Let me know what you'd like to dig into next."));
    assert!(!is_interim_progress_update("Running cargo fmt uses rustfmt to rewrite the source files."));
    assert!(!is_interim_progress_update("Completed. All requested fixes are done."));
    assert!(!is_interim_progress_update("Final review: two blockers remain with next action."));
}
#[test]
fn autonomous_continue_triggers_for_follow_up_and_interim_text() {
    let history = vec![uni::Message::user("continue".to_string())];
    assert!(evaluate_interim_text_continuation(true, false, &history, "Let me fix the next issue.", 0).should_continue);
    assert!(!evaluate_interim_text_continuation(true, true, &history, "Let me fix the next issue.", 0).should_continue);
    assert!(
        evaluate_interim_text_continuation(false, false, &history, "Let me fix the next issue.", 0).should_continue
    );
}
#[test]
fn autonomous_continue_triggers_for_interim_text_after_tool_activity() {
    let history = vec![
        uni::Message::user("run cargo clippy and fix".to_string()),
        uni::Message::assistant("I will run cargo clippy now.".to_string()).with_tool_calls(vec![
            uni::ToolCall::function("call_1".to_string(), "command_session".to_string(), "{}".to_string()),
        ]),
        uni::Message::tool_response("call_1".to_string(), "warning: ...".to_string()),
    ];

    assert!(
        evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:",
            0
        )
        .should_continue
    );
}
#[test]
fn autonomous_continue_triggers_for_execution_request_without_prior_tool_activity() {
    let history = vec![
        uni::Message::user("run cargo clippy and fix".to_string()),
        uni::Message::assistant("I will start now.".to_string()),
    ];

    assert!(
        evaluate_interim_text_continuation(
            true,
            false,
            &history,
            "Now I need to update the function body to use settings.reasoning_effort and settings.verbosity:",
            0
        )
        .should_continue
    );
}
#[test]
fn full_auto_continues_after_non_conclusive_text_without_tool_activity() {
    let history = vec![uni::Message::user("work through the requested task".to_string())];
    let decision = evaluate_interim_text_continuation(
        true,
        false,
        &history,
        "I reviewed the request and have more work to do.",
        0,
    );

    assert!(decision.should_continue);
    assert_eq!(decision.reason, "full_auto_continuation");
    assert!(decision.is_relaxed_continuation);
}
#[test]
fn full_auto_continues_after_inspect_fix_and_check_updates() {
    let history = vec![uni::Message::user("complete the requested work".to_string())];
    for text in [
        "I'll inspect the remaining files now.",
        "I'll fix the parser branch next.",
        "I'll check the targeted test output before summarizing.",
    ] {
        let decision = evaluate_interim_text_continuation(true, false, &history, text, 0);
        assert!(decision.should_continue, "expected continuation for {text:?}");
    }
}
#[test]
fn full_auto_stops_after_a_read_only_summary_request_without_keyword_markers() {
    let history = vec![uni::Message::user(
        "Explore the codebase and summarize what makes this project special.".to_string(),
    )];
    let answer = "Here's a consolidated table of the project's differentiators:\n\n| Area | Meaning |\n|---|---|\n| Runtime | The harness coordinates tools and context. |\n| Safety | Commands are checked before execution. |";

    let decision = evaluate_interim_text_continuation(true, false, &history, answer, 0);

    assert!(!decision.should_continue);
    assert_eq!(decision.reason, "full_auto_read_only_answer");
}
#[test]
fn full_auto_keeps_action_requests_on_the_autonomous_path() {
    let history = vec![uni::Message::user(
        "Explore the parser, summarize the issue, and fix the regression.".to_string(),
    )];

    let decision = evaluate_interim_text_continuation(true, false, &history, "I reviewed the request.", 0);

    assert!(decision.should_continue);
    assert_eq!(decision.reason, "full_auto_continuation");
}
#[test]
fn full_auto_stops_for_an_informational_how_to_fix_question() {
    let history = vec![uni::Message::user("How do I fix the parser regression?".to_string())];
    let decision = evaluate_interim_text_continuation(
        true,
        false,
        &history,
        "The parser loses the trailing token when the input ends with a delimiter.",
        0,
    );

    assert!(!decision.should_continue);
    assert_eq!(decision.reason, "full_auto_read_only_answer");
}
#[tokio::test]
async fn read_only_summary_is_published_as_final_without_a_follow_up_directive() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.full_auto = true;
    ctx.working_history.push(uni::Message::user(
        "Explore the codebase and summarize what makes this project special.".to_string(),
    ));

    let outcome = ctx
        .handle_text_response(
            "Here's a consolidated table of the project's differentiators:\n\n| Area | Meaning |\n|---|---|\n| Runtime | The harness coordinates tools and context. |\n| Safety | Commands are checked before execution. |"
                .to_string(),
            Vec::new(),
            None,
            None,
            true,
        )
        .await
        .expect("read-only summary should complete");

    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains(AUTONOMOUS_CONTINUE_DIRECTIVE)
    }));
    assert_eq!(
        ctx.working_history
            .iter()
            .rev()
            .find(|message| message.role == uni::MessageRole::Assistant)
            .and_then(|message| message.phase),
        Some(uni::AssistantPhase::FinalAnswer)
    );
}
#[test]
fn full_auto_stops_on_completion_summaries() {
    let history = vec![uni::Message::user("finish the requested task".to_string())];
    for text in [
        "Completed the requested changes. All targeted tests passed.",
        "Summary:\n- Updated the parser and verified the targeted test.",
        "The result is complete.",
    ] {
        let decision = evaluate_interim_text_continuation(true, false, &history, text, 0);
        assert!(!decision.should_continue, "expected terminal completion for {text:?}");
        assert_eq!(decision.reason, "full_auto_final_completion");
    }
}
#[test]
fn full_auto_stops_on_questions_approval_requests_and_blockers() {
    let history = vec![uni::Message::user("complete the requested task".to_string())];
    let cases = [
        ("Should I continue with the migration?", "full_auto_user_input_handoff"),
        ("I need your approval before I proceed.", "full_auto_user_input_handoff"),
        // Credentials are a true safety handoff (S2A): the shared
        // `tracker_final_text_is_safety_handoff` classifier wins over the
        // generic blocker bucket, so the turn ends as a safety handoff
        // rather than a plain blocked handoff.
        ("I’m blocked by missing credentials.", "full_auto_safety_handoff"),
    ];

    for (text, reason) in cases {
        let decision = evaluate_interim_text_continuation(true, false, &history, text, 0);
        assert!(!decision.should_continue, "expected terminal handoff for {text:?}");
        assert_eq!(decision.reason, reason);
    }
}
#[test]
fn interactive_mode_keeps_non_conclusive_text_terminal() {
    let history = vec![uni::Message::user("work through the requested task".to_string())];
    let decision = evaluate_interim_text_continuation(
        false,
        false,
        &history,
        "I reviewed the request and have more work to do.",
        0,
    );

    assert!(!decision.should_continue);
    assert_eq!(decision.reason, "non_interim_text");
}
#[test]
fn continuation_telemetry_distinguishes_terminal_paths() {
    let history = vec![uni::Message::user("complete the requested task".to_string())];
    let continuing = evaluate_interim_text_continuation(true, false, &history, "I reviewed the request.", 0);
    assert_eq!(continuation_telemetry_outcome(true, &continuing), "full_auto_continuation");

    let completed = evaluate_interim_text_continuation(true, false, &history, "The result is complete.", 0);
    assert_eq!(continuation_telemetry_outcome(true, &completed), "final_completion");

    let input = evaluate_interim_text_continuation(true, false, &history, "Should I continue?", 0);
    assert_eq!(continuation_telemetry_outcome(true, &input), "user_input_handoff");

    let capped = evaluate_interim_text_continuation(true, false, &history, "I have more work to do.", 3);
    assert_eq!(capped.reason, "full_auto_relaxed_cap");
    assert_eq!(continuation_telemetry_outcome(true, &capped), "safety_cap_handoff");

    let blocked = evaluate_interim_text_continuation(true, false, &history, "I cannot proceed without access.", 0);
    assert_eq!(continuation_telemetry_outcome(true, &blocked), "blocked_handoff");

    // Safety/verification handoffs must not be counted as completions.
    let safety = evaluate_interim_text_continuation(
        true,
        false,
        &history,
        "Verification is still pending; run cargo check next.",
        0,
    );
    assert_eq!(safety.reason, "full_auto_safety_handoff");
    assert_eq!(continuation_telemetry_outcome(true, &safety), "safety_handoff");
}
#[test]
fn autonomous_continue_triggers_for_exploration_request_without_full_auto() {
    let history = vec![
        uni::Message::user("explore about vtcode core agent loop".to_string()),
        uni::Message::assistant("I can help.".to_string()),
    ];

    assert!(evaluate_interim_text_continuation(
        false,
        false,
        &history,
        "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code.",
        0
    )
    .should_continue);
}
#[test]
fn autonomous_continue_does_not_trigger_for_explanatory_request_without_full_auto() {
    let history = vec![
        uni::Message::user("tell me about core agent loop".to_string()),
        uni::Message::assistant("I can help.".to_string()),
    ];

    assert!(!evaluate_interim_text_continuation(
        false,
        false,
        &history,
        "I'll quickly inspect the actual vtcode-core runloop files and then summarize the core agent loop concretely from code.",
        0
    )
    .should_continue);
}
