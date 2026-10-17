use super::*;
use crate::agent::runloop::unified::run_loop_context::RecoveryMode;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

#[test]
fn system_directive_is_not_duplicated_after_intervening_messages() {
    let directive = "Synthesize a final response now.";
    let mut history = vec![uni::Message::system(directive.to_string())];
    for index in 0..4 {
        history.push(uni::Message::assistant(format!("intervening message {index}")));
    }

    push_system_directive_once(&mut history, directive);

    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System && message.content.as_text() == directive)
            .count(),
        1
    );
}

#[test]
fn system_directive_is_reissued_for_a_new_user_turn() {
    let directive = "Synthesize a final response now.";
    let mut history = vec![
        uni::Message::system(directive.to_string()),
        uni::Message::user("first request".to_string()),
    ];

    push_system_directive_once(&mut history, directive);

    assert_eq!(
        history
            .iter()
            .filter(|message| message.role == uni::MessageRole::System && message.content.as_text() == directive)
            .count(),
        2
    );
}

#[test]
fn follow_up_prompt_detection_accepts_continue_variants() {
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue"));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("continue."));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("go on"));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like("please continue"));
    assert!(crate::agent::runloop::unified::state::is_follow_up_prompt_like(
        "Continue autonomously from the last stalled turn. Stall reason: x."
    ));
    assert!(!crate::agent::runloop::unified::state::is_follow_up_prompt_like("run cargo clippy and fix"));
}

#[test]
fn tracker_adoption_requires_tracker_use_or_explicit_continuation() {
    // Fresh informational question with no tool activity must not adopt
    // unrelated workspace tracker work (session-vtcode-20261008T094713Z:
    // `what is vtcode` redirected into README edits).
    let fresh_info = vec![uni::Message::user("what is vtcode".to_string())];
    assert!(!tracker_continuation_adoption_allowed(&fresh_info));
    // A work request alone does not identify the existing checklist.
    let progressive = vec![uni::Message::user("please fix the README table".to_string())];
    assert!(!tracker_continuation_adoption_allowed(&progressive));
    // Ordinary tool activity cannot adopt unrelated tracker work.
    let with_tools = vec![
        uni::Message::user("what is vtcode".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![vtcode_core::llm::provider::ToolCall::function(
                "call_1".to_string(),
                "exec_command".to_string(),
                "{}".to_string(),
            )],
        ),
    ];
    assert!(!tracker_continuation_adoption_allowed(&with_tools));
    // Explicit follow-up adopts.
    let follow_up = vec![uni::Message::user("continue".to_string())];
    assert!(tracker_continuation_adoption_allowed(&follow_up));
    let mut approved_plan = vec![uni::Message::user(
        vtcode_core::prompts::system::PLANNING_WORKFLOW_IMPLEMENTATION_PROMPT.to_string(),
    )];
    assert!(tracker_continuation_adoption_allowed(&approved_plan));
    approved_plan.push(uni::Message::user(
        crate::agent::runloop::unified::turn::session_loop::BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX
            .to_string(),
    ));
    assert!(tracker_continuation_adoption_allowed(&approved_plan));
    assert!(!tracker_continuation_adoption_allowed(&[]));
}

#[test]
fn tracker_adoption_requires_successful_matching_current_request_result() {
    let request = uni::Message::user("fix the README table".to_string());
    for (action, status, expected) in [
        ("create", "created", true),
        ("create", "replaced", true),
        ("create", "unchanged", true),
        ("update", "updated", true),
        ("add", "added", true),
        ("list", "ok", false),
        ("create", "error", false),
    ] {
        let call = uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "tracker_1".to_string(),
                "task_tracker".to_string(),
                serde_json::json!({"action": action}).to_string(),
            )],
        );
        let mut history = vec![request.clone(), call];
        assert!(!tracker_continuation_adoption_allowed(&history), "unanswered {action}");
        history.push(uni::Message::tool_response(
            "different_id".to_string(),
            serde_json::json!({"status": status}).to_string(),
        ));
        assert!(!tracker_continuation_adoption_allowed(&history), "unmatched {action}");
        history.push(uni::Message::tool_response(
            "tracker_1".to_string(),
            serde_json::json!({"status": status}).to_string(),
        ));
        assert_eq!(tracker_continuation_adoption_allowed(&history), expected, "{action}/{status}");
        history.push(uni::Message::user(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_continue_follow_up(&[
                "#1 table (pending)".to_string(),
            ]),
        ));
        assert_eq!(tracker_continuation_adoption_allowed(&history), expected, "internal {action}/{status}");
        history.push(uni::Message::user("what is vtcode".to_string()));
        assert!(!tracker_continuation_adoption_allowed(&history), "new request {action}/{status}");
    }
}

#[test]
fn tracker_adoption_does_not_match_reused_ids_from_another_batch() {
    let history = vec![
        uni::Message::user("fix README".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "reused".to_string(),
                "task_tracker".to_string(),
                r#"{"action":"create"}"#.to_string(),
            )],
        ),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "reused".to_string(),
                "exec_command".to_string(),
                "{}".to_string(),
            )],
        ),
        uni::Message::tool_response("reused".to_string(), r#"{"status":"created"}"#.to_string()),
    ];
    assert!(!tracker_continuation_adoption_allowed(&history));
}

#[tokio::test]
async fn informational_answer_with_tool_activity_does_not_resume_workspace_tracker() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_vt_cfg_for_test(vtcode_core::config::loader::VTCodeConfig::default());
    let mut ctx = backing.turn_processing_context();
    let tracker = ctx.tool_registry.get_tool("task_tracker").expect("tracker");
    tracker
        .execute(serde_json::json!({
            "action": "create", "title": "Unrelated README work", "items": ["Rewrite README"]
        }))
        .await
        .expect("existing workspace tracker");
    ctx.working_history.extend([
        uni::Message::user("what is vtcode".to_string()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "read_1".to_string(),
                "read_file".to_string(),
                r#"{"path":"README.md"}"#.to_string(),
            )],
        ),
        uni::Message::tool_response("read_1".to_string(), "VT Code is a coding agent.".to_string()),
    ]);
    let outcome = ctx
        .handle_text_response("VT Code is a Rust terminal coding agent.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("informational answer");
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains(AUTONOMOUS_CONTINUE_DIRECTIVE)
    }));
    let output = tracker
        .execute(serde_json::json!({"action": "list"}))
        .await
        .expect("tracker unchanged");
    assert_eq!(output["checklist"]["title"], "Unrelated README work");
    assert_eq!(output["checklist"]["pending"], 1);
}

#[tokio::test]
async fn explicit_continuation_resumes_incomplete_workspace_tracker() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_vt_cfg_for_test(vtcode_core::config::loader::VTCodeConfig::default());
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry
        .get_tool("task_tracker")
        .expect("tracker")
        .execute(serde_json::json!({"action": "create", "title": "README", "items": ["Repair table"]}))
        .await
        .expect("existing tracker");
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.tool_registry
        .begin_tracker_request(tracker_request_explicitly_adopts("continue"));
    let outcome = ctx
        .handle_text_response("The README table still needs repair.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("continuation response");
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains(AUTONOMOUS_CONTINUE_DIRECTIVE)
    }));
}

#[tokio::test]
async fn interactive_tracker_adoption_survives_compaction_and_resets_for_a_fresh_request() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_vt_cfg_for_test(vtcode_core::config::loader::VTCodeConfig::default());
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry.begin_tracker_request(false);
    ctx.working_history
        .push(uni::Message::user("repair the README table".to_string()));
    ctx.tool_registry
        .execute_tool(
            "task_tracker",
            serde_json::json!({
                "action":"create", "title":"README", "items":["Repair table"]
            }),
        )
        .await
        .expect("current request adopts tracker");
    // No matching call/result pair remains when completion is assessed.
    ctx.working_history.clear();
    ctx.working_history
        .push(uni::Message::user("Compacted current task progress".to_string()));
    assert!(!tracker_continuation_adoption_allowed(ctx.working_history));
    let outcome = ctx
        .handle_text_response("The README table still needs repair.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("compacted response");
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));

    ctx.tool_registry.begin_tracker_request(false);
    ctx.working_history.clear();
    ctx.working_history.push(uni::Message::user("what is vtcode".to_string()));
    let outcome = ctx
        .handle_text_response("VT Code is a Rust terminal coding agent.".to_string(), Vec::new(), None, None, false)
        .await
        .expect("fresh response");
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
}

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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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

#[tokio::test]
async fn recovery_pass_progress_only_text_completes_turn() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
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

fn checkpoint_shaped_repo_overview() -> &'static str {
    "Here's a quick overview of the repo:\n\n\
    **VT Code** — a Rust coding agent for long-running autonomous workflows, with OS-native sandboxing, multi-provider LLM support, open protocols, and extensible Skills.\n\n\
    **Layout**\n\
    - Root crate `vtcode` (binary) + workspace of ~30 member crates under `vtcode-*` (e.g. `vtcode-core`, `vtcode-ui`, `vtcode-llm`, `vtcode-mcp`, `vtcode-safety`, `vtcode-exec-events`, `vtcode-indexer`, `vtcode-skills`, `vtcode-config`, `vtcode-a2a`, `vtcode-acp`, etc.)\n\
    - Rust stable, MSRV 1.98.1, edition 2024; CI runs `RUSTFLAGS=\"-D warnings\"` with `--locked`\n\
    - `default-members` = root, `vtcode-core`, `vtcode-ui`\n\n\
    **Capabilities**\n\
    - Agent runtime: interactive TUI, slash commands, streaming, `ask`/`exec` CLI, session resume\n\
    - Coding tools: safe file ops, ripgrep search, fuzzy discovery, code intelligence, project indexing, terminal execution\n\
    - Extensibility: Agent Skills, MCP client/server, lifecycle hooks, subagents, custom providers, Zed ACP, Claude Code\n\
    - Model providers: 21+ LLMs (Anthropic, OpenAI, Gemini, OpenRouter, Ollama, LM Studio, etc.)\n\
    - Safety: restricted shell sandbox, tool guardrails, subprocess isolation, full audit logging\n\
    - Protocols: Open Responses, Agent2Agent (A2A), ATIF, Anthropic Messages API\n\n\
    **Default model**: MiMo V2.6 Pro (Xiaomi), 1M-token context (`mimo-v2.6-pro`).\n\n\
    **Other top-level dirs**: `docs/`, `plans/`, `rules/`, `examples/`, `tests/`, `evals/`, `fuzz/`, `scripts/`, `crates/codegen/xtask/`, `homebrew/`.\n\n\
    Quick start:\n\
    ```shell\n\
    curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash\n\
    vtcode init\n\
    vtcode\n\
    ```\n\n\
    Let me know what you'd like to dig into next — a specific crate, the agent loop, the TUI, sandboxing, or something else."
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

#[test]
fn contains_user_input_request_detects_various_patterns() {
    assert!(contains_user_input_request("how would you like to proceed?"));
    assert!(contains_user_input_request("what would you like me to do?"));
    assert!(contains_user_input_request("which option should i choose?"));
    assert!(contains_user_input_request("shall i continue?"));
    assert!(contains_user_input_request("do you want me to retry?"));
    assert!(contains_user_input_request("please confirm and i'll retry"));
    assert!(contains_user_input_request("let me know how to proceed"));
    assert!(contains_user_input_request("let me know what you'd like to dig into next"));
    assert!(contains_user_input_request("tell me which area you want next"));
    assert!(contains_user_input_request("waiting for your approval"));
    assert!(!contains_user_input_request(
        "The compiler errors tell me what to fix next. Let me update the parser branch now."
    ));
    assert!(!contains_user_input_request("Let me check whether this should initialize the cache before use."));
    assert!(!contains_user_input_request("let me fix the next issue"));
    assert!(!contains_user_input_request("i'll apply these changes now"));
}

#[test]
fn contains_user_input_request_detects_closing_offers() {
    assert!(contains_user_input_request(
        "verified and done. if you want a deeper pass, say the word and i'll do a larger pass."
    ));
    assert!(contains_user_input_request("all checks pass; happy to iterate further."));
    assert!(contains_user_input_request("the docs are updated — just ask if you want the full diff."));
}

#[test]
fn verified_recap_with_trailing_offer_does_not_continue() {
    // Regression guard for session-vtcode-20260912T083718Z: a conclusive
    // verification recap that closes by offering optional follow-up work
    // ("say the word and I'll do a larger pass") was relaxed-continued,
    // re-prompted into a second recap, and the turn ended Blocked on the
    // text cap after the verification gate had already cleared.
    let mut recap = String::from(
        "Verification passed: `cargo fmt --all -- --check && cargo check --locked -p vtcode` → exit 0.\n\n\
         **Recap**\n\n\
         **Changed (README.md):** tightened Overview prose, sharpened the pillar table, \
         streamlined Quick start phrasing, condensed the Documentation table, compacted the \
         Providers paragraph, and fixed a hyphen→em-dash in the TUI tip. Contributor and \
         sponsor HTML blocks preserved verbatim.\n\n\
         **Verified:** all referenced doc paths exist; the fmt and check commands exit 0.\n\n",
    );
    while recap.len() <= 800 {
        recap.push_str("Extra verified detail line that keeps the recap above the interim length cap.\n");
    }
    recap.push_str(
        "**Remaining risk:** none functional. If you want a deeper revamp (restructured \
         sections, feature highlights, refreshed screenshots), say the word and I'll do a \
         larger pass.",
    );
    assert!(recap.len() > 800, "recap must exceed the interim length cap to reach the relaxed path");

    let history = vec![
        uni::Message::user("polish the README intro".to_string()),
        uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "exec_command".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "Finished `dev` profile".to_string()),
    ];

    assert!(
        !evaluate_interim_text_continuation(true, false, &history, &recap, 0).should_continue,
        "a conclusive recap closing with an optional-work offer must end the turn"
    );
}

#[test]
fn tracker_incomplete_override_continues_status_recaps() {
    use super::apply_tracker_continuation_override;
    let history: Vec<uni::Message> = Vec::new();
    let recap = "## Status\nBlocked by turn budget. Next step on resume: read design/diff.rs.";
    let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
    assert!(!base.should_continue);
    let overridden = apply_tracker_continuation_override(base, true, false, recap);
    assert!(overridden.should_continue);
    assert_eq!(overridden.reason, "tracker_incomplete_continuation");
    assert!(!overridden.is_relaxed_continuation);

    let question = "Should I proceed with the remaining steps?";
    let q_base = evaluate_interim_text_continuation(true, false, &history, question, 0);
    let q_over = apply_tracker_continuation_override(q_base, true, false, question);
    assert!(!q_over.should_continue);

    let plan_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, true, &history, recap, 0),
        true,
        true,
        recap,
    );
    assert!(!plan_over.should_continue);

    let complete_tracker = apply_tracker_continuation_override(base, false, false, recap);
    assert!(!complete_tracker.should_continue);

    // Permission/safety handoffs stay terminal for the user.
    for blocker in [
        "Permission denied for exec_command. Next step: retry after access is granted.",
        "Access denied to the sandbox; policy block.",
        "I hit the tool-call safety fuse mid-verification; policy block.",
        "Requires manual intervention from the workspace owner.",
        "Missing credentials for the provider route.",
    ] {
        let b_base = evaluate_interim_text_continuation(true, false, &history, blocker, 0);
        let b_over = apply_tracker_continuation_override(b_base, true, false, blocker);
        assert!(!b_over.should_continue, "must not continue on blocker: {blocker}");
    }

    // Budget-like "blocked by …" still continues when tracker work remains.
    let budget = "## Status\nBlocked by turn budget. Next step: read design/diff.rs.";
    let budget_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, budget, 0),
        true,
        false,
        budget,
    );
    assert!(budget_over.should_continue);
    assert_eq!(budget_over.reason, "tracker_incomplete_continuation");

    // Tool-loop / tool-budget recaps continue even with "blocked by".
    for recap in [
        "## Status\nBlocked by turn tool budget. Next step: patch FileChangeItem.",
        "## Status\nTool loop budget exhausted this turn. Next: run cargo check.",
        "Task 3 mid-flight — hit the per-file read cap before editing. Next: one targeted read then patch.",
    ] {
        let over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, false, &history, recap, 0),
            true,
            false,
            recap,
        );
        assert!(over.should_continue, "must continue on recoverable recap: {recap}");
    }

    // Mid-text `?` in a status section is not a user handoff when tracker incomplete.
    let status_question = "## Status\nTask 2 done — is the next step clear? Continuing with task 3 now using tools.";
    let s_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, status_question, 0),
        true,
        false,
        status_question,
    );
    assert!(s_over.should_continue, "mid-text question in status recap must continue");

    // Broad mid-text "can you" / "need your" in status prose is not a handoff.
    let status_prose =
        "## Status\nNext we need your workspace-relative path in CONFIG; can you see it in the dump? Patching now.";
    let prose_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, status_prose, 0),
        true,
        false,
        status_prose,
    );
    assert!(prose_over.should_continue, "status prose mentioning need-your/can-you must continue");

    // Clause-start interview ask without a trailing `?` still ends the turn.
    let clause_ask = "Can you confirm which migration path to take before I edit the schema.";
    let c_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, clause_ask, 0),
        true,
        false,
        clause_ask,
    );
    assert!(!c_over.should_continue);

    // Trailing `?` stays terminal.
    let trailing_q = "Should I proceed with the remaining steps?";
    let t_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, trailing_q, 0),
        true,
        false,
        trailing_q,
    );
    assert!(!t_over.should_continue);

    // Verification-pending recaps are true handoffs — outer verification
    // recovery owns that path; in-turn tracker continuation must not race
    // past the anti-blind gate, even when full-auto already continued.
    for recap in [
        "Turn blocked after repeated unverified assistant responses; verification is still pending.",
        "## Status\nBlocked by turn budget. Verification is still pending; run cargo check --locked.",
        "Anti-blind checkpoint: verification is still pending before further edits.",
    ] {
        let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
        let over = apply_tracker_continuation_override(base, true, false, recap);
        assert!(!over.should_continue, "must not continue past verification gate: {recap}");
    }

    // Session evidence: budget/status recaps without a gate still continue.
    // Full-auto may already continue some of these; the tracker override
    // must not reverse that. Assert `tracker_incomplete_continuation` only
    // when the base classifier would have stopped.
    for recap in [
        "## Status\nTask 7 blocked by the turn's preview budget before I could read docs.",
        "## Status\nTool budget ran out after evidence gathering; next step is the patch.",
        "Task 3 mid-flight — hit the per-file read cap before editing.",
        "Fix 1 partially applied, needs verification; continuing with the remaining checks now.",
    ] {
        let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
        let over = apply_tracker_continuation_override(base, true, false, recap);
        assert!(over.should_continue, "must continue on session budget recap: {recap}");
        if !base.should_continue {
            assert_eq!(over.reason, "tracker_incomplete_continuation");
        }
    }

    // When the base classifier would end the turn, tracker override forces
    // non-relaxed continuation with the tracker reason.
    let status_only =
        "## Status\nBlocked by the turn's preview budget. Next step on resume: read docs/development/README.md.";
    let s_base = evaluate_interim_text_continuation(false, false, &history, status_only, 0);
    assert!(!s_base.should_continue, "interactive status recap must not auto-continue without tracker");
    let s_over = apply_tracker_continuation_override(s_base, true, false, status_only);
    assert!(s_over.should_continue);
    assert_eq!(s_over.reason, "tracker_incomplete_continuation");
    assert!(!s_over.is_relaxed_continuation);
}

#[test]
fn recoverable_status_recap_vocabulary_matches_outer_classifier() {
    use vtcode_core::core::agent::completion::recoverable_status_recap_phrasing;
    for phrase in [
        "blocked by the turn's preview budget",
        "tool budget ran out",
        "per-file read cap",
        "preview budget exhausted",
        "tool loop budget exhausted",
        "recovery fallback",
        "reached the safety cap",
    ] {
        let lower = phrase.to_ascii_lowercase();
        assert!(recoverable_status_recap_phrasing(&lower), "shared vocab must cover {phrase}");
        assert!(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(
                Some(phrase)
            ),
            "outer classifier must cover {phrase}"
        );
    }
    for phrase in [
        "Turn blocked after repeated unverified assistant responses; verification is still pending.",
        "exec_command is denied by permission policy",
        "request_user_input is required",
    ] {
        assert!(
            !crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(
                Some(phrase)
            ),
            "outer classifier must deny {phrase}"
        );
    }
}
