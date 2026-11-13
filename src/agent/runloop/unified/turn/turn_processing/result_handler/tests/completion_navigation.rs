use super::*;

/// Regression test: recovery text that contains only prose (no markers)
/// must pass through unmodified — the guard must not fire on clean text.
#[tokio::test]
async fn recovery_clean_prose_is_not_intercepted_by_marker_guard() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: "The search found 3 matches. Next step: update the failing test.".to_string(),
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
    .expect("clean recovery prose should be handled");

    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
        "clean prose in recovery should complete the turn normally"
    );
    assert!(backing.last_history_message_contains("3 matches"));
}
#[tokio::test]
async fn denied_interview_retries_plain_text_once_before_approval() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();

    let mut ctx = backing.turn_processing_context();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let response = TurnProcessingResult::TextResponse {
        text: "Next open decision: none from available evidence. Type yes to start this plan.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let first = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: response,
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("denied interview response should schedule synthesis");
    assert!(matches!(first, TurnHandlerOutcome::Continue));
}
#[tokio::test]
async fn completion_verification_short_edit_arms_gate_without_completion_wording() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_verification_override_for_test("rustc --version");
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "apply_patch",
        r#"{"input":"*** Begin Patch\n*** Add File: main.rs\n+fn main() {}\n*** End Patch"}"#,
    )
    .await;
    assert!(tracker.verification_is_pending(), "tracker mirrors registry debt after tool outcomes");
    assert!(
        backing
            .turn_processing_context()
            .tool_registry
            .completion_verification_pending()
    );
    let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 4).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!tracker.verification_is_pending());
    assert!(
        !backing
            .turn_processing_context()
            .tool_registry
            .completion_verification_pending()
    );
}
#[tokio::test]
async fn navigation_synthesis_resumes_execution_once_and_second_stall_blocks() {
    use crate::agent::runloop::unified::state::NavigationRecovery;
    use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD;

    let mut backing = TestTurnProcessingBacking::new().await;
    backing.set_skip_confirmations_for_test(true);
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let summary = "The evidence identifies the affected parser. I will apply the correction and run the verifier.";
    let outcome = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history
            .push(uni::Message::user("Implement the fix and verify it.".into()));
        tracker.total_low_signal_navigations = EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD;
        assert!(matches!(handle_turn_balancer(&mut ctx, 8, &mut tracker, 40, 3).await, TurnHandlerOutcome::Continue));
        assert_eq!(ctx.session_stats.navigation_recovery, NavigationRecovery::Synthesizing);
        assert!(ctx.consume_recovery_pass());
        handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: summary.into(),
                reasoning: vec![],
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 8,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
            max_tool_loops: 40,
            tool_repeat_limit: 3,
        })
        .await
        .unwrap()
    };
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    {
        let ctx = backing.turn_processing_context();
        assert!(!ctx.is_recovery_active());
        assert!(!ctx.harness_state.recovery_is_tool_free());
        assert_eq!(ctx.session_stats.navigation_recovery, NavigationRecovery::Consumed);
        let message = ctx.working_history.iter().find(|m| m.content.as_text() == summary).unwrap();
        assert_eq!(message.phase, Some(uni::AssistantPhase::Commentary));
    }
    // A real admitted verifier remains executable after synthesis.
    let outcome = {
        let mut ctx = backing.turn_processing_context();
        handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::ToolCalls {
                tool_calls: vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
                    "navigation-verifier".into(),
                    "exec_command".into(),
                    serde_json::json!({"cmd":"rustc --version", "yield_time_ms":10000}).to_string(),
                ))],
                assistant_text: String::new(),
                reasoning: vec![],
                reasoning_details: None,
            },
            response_streamed: false,
            step_count: 9,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
            max_tool_loops: 40,
            tool_repeat_limit: 3,
        })
        .await
        .unwrap()
    };
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    let mut ctx = backing.turn_processing_context();
    let replies: Vec<_> = ctx
        .working_history
        .iter()
        .filter(|m| m.tool_call_id.as_deref() == Some("navigation-verifier"))
        .collect();
    assert_eq!(replies.len(), 1);
    assert!(replies[0].content.as_text().contains("rustc"));
    let payload: serde_json::Value = serde_json::from_str(&replies[0].content.as_text()).unwrap();
    assert_eq!(payload["exit_code"], 0, "the verifier must finish successfully");

    tracker.total_low_signal_navigations = EXECUTION_TOTAL_LOW_SIGNAL_THRESHOLD;
    assert!(matches!(
        handle_turn_balancer(&mut ctx, 16, &mut tracker, 40, 3).await,
        TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })
    ));
    assert!(ctx.session_stats.navigation_recovery.blocked());
    ctx.session_stats
        .register_follow_up_prompt("Preflight corrective turn: fix arguments");
    assert!(ctx.session_stats.navigation_recovery.blocked(), "internal continuation cannot refill recovery");
    ctx.session_stats.register_follow_up_prompt("continue");
    assert_eq!(ctx.session_stats.navigation_recovery, NavigationRecovery::Ready);
}
#[tokio::test]
async fn navigation_synthesis_respects_terminal_handoffs_and_existing_limits() {
    use crate::agent::runloop::unified::state::NavigationRecovery;
    for case in [
        "policy",
        "wallclock",
        "validation",
        "loops",
        "disabled",
        "approval",
        "informational",
    ] {
        let mut backing = TestTurnProcessingBacking::new().await;
        let mut cfg = vtcode_core::config::loader::VTCodeConfig::default();
        cfg.agent.harness.continuation.auto_continue_tracker = case != "disabled";
        backing.set_vt_cfg_for_test(cfg);
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let outcome = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user(if case == "informational" {
                "Explain the parser.".into()
            } else {
                "Fix the parser.".into()
            }));
            if case != "policy" {
                ctx.session_stats.navigation_recovery.arm();
            }
            ctx.activate_recovery("synthesis");
            assert!(ctx.consume_recovery_pass());
            if case == "wallclock" {
                ctx.harness_state.wall_clock_exhausted_emitted = true;
            }
            if case == "validation" {
                ctx.session_stats.preflight_recovery.reject(
                    "exec_command",
                    crate::agent::runloop::unified::state::ValidationCategory::Arguments,
                    Some(&serde_json::json!({})),
                    3,
                    false,
                );
            }
            let text = match case {
                "approval" => "I need your approval before proceeding.",
                "informational" => "The parser converts input into a syntax tree.",
                _ => "I will apply the correction next.",
            };
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::TextResponse {
                    text: text.into(),
                    reasoning: vec![],
                    reasoning_details: None,
                    proposed_plan: None,
                },
                response_streamed: false,
                step_count: if case == "loops" { 20 } else { 4 },
                repeated_tool_attempts: &mut tracker,
                turn_modified_files: &mut modified,
                max_tool_loops: 20,
                tool_repeat_limit: 3,
            })
            .await
            .unwrap()
        };
        assert!(matches!(outcome, TurnHandlerOutcome::Break(_)), "{case}");
        let ctx = backing.turn_processing_context();
        assert!(ctx.harness_state.recovery_is_tool_free(), "{case} must not restore tools");
        assert_ne!(ctx.session_stats.navigation_recovery, NavigationRecovery::Blocked);
    }
}
#[tokio::test]
async fn navigation_synthesis_uses_only_adopted_tracker_and_nonempty_summary() {
    for (adopted, text, resumes) in [
        (true, "Evidence retained.", true),
        (false, "Evidence retained.", false),
        (true, "  ", false),
    ] {
        let mut backing = TestTurnProcessingBacking::new().await;
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let outcome = {
            let mut ctx = backing.turn_processing_context();
            ctx.tool_registry.allow_all_tools().await.unwrap();
            ctx.tool_registry
                .execute_tool(
                    "task_tracker",
                    serde_json::json!({"action":"create", "items":[{"description":"Fix parser"}]}),
                )
                .await
                .unwrap();
            ctx.tool_registry.begin_tracker_request(adopted);
            ctx.working_history.push(uni::Message::user("Fix the parser.".into()));
            ctx.session_stats.navigation_recovery.arm();
            ctx.activate_recovery("navigation synthesis");
            assert!(ctx.consume_recovery_pass());
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::TextResponse {
                    text: text.into(),
                    reasoning: vec![],
                    reasoning_details: None,
                    proposed_plan: None,
                },
                response_streamed: false,
                step_count: 4,
                repeated_tool_attempts: &mut tracker,
                turn_modified_files: &mut modified,
                max_tool_loops: 20,
                tool_repeat_limit: 3,
            })
            .await
            .unwrap()
        };
        assert_eq!(matches!(outcome, TurnHandlerOutcome::Continue), resumes, "adopted={adopted}, text={text:?}");
        let ctx = backing.turn_processing_context();
        assert_eq!(!ctx.harness_state.recovery_is_tool_free(), resumes);
    }
}
#[tokio::test]
async fn repeated_successes_cannot_refill_recovery_for_a_missing_requirement() {
    use vtcode_core::config::constants::tools;
    let mut backing = TestTurnProcessingBacking::new().await;
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    assert!(tracker.record_verification_auto_recovery_with_limit(1));
    tracker.record_auto_verification_executed();
    let mut modified = BTreeSet::new();
    {
        let ctx = backing.turn_processing_context();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        ctx.tool_registry
            .execute_tool(
                tools::TASK_TRACKER,
                serde_json::json!({
                    "action":"create", "items":[{"description":"Check", "verify":["grep -n absent fixture.txt"]}]
                }),
            )
            .await
            .unwrap();
        ctx.tool_registry.restore_completion_verification_debt();
    }
    for _ in 0..2 {
        drive_model_tool_call(
            &mut backing,
            &mut tracker,
            &mut modified,
            tools::EXEC_COMMAND,
            r#"{"cmd":"rustc --version"}"#,
        )
        .await;
        assert!(tracker.verification_is_pending());
        assert!(!tracker.should_auto_execute_verifier());
        assert_eq!(tracker.verification_auto_recovery_attempts(), 1);
    }
}
