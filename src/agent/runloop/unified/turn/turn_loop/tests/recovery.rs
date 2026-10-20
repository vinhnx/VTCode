use super::*;

#[test]
fn has_tool_response_since_detects_new_tool_message() {
    let messages = vec![
        uni::Message::user("run script".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "ok".to_string()),
    ];

    assert!(has_tool_response_since(&messages, 1));
}

#[test]
fn has_tool_response_since_ignores_non_tool_messages() {
    let messages = vec![
        uni::Message::user("hello".to_string()),
        uni::Message::assistant("done".to_string()),
    ];

    assert!(!has_tool_response_since(&messages, 0));
}

#[test]
fn has_tool_response_since_handles_baseline_past_end() {
    let messages = vec![uni::Message::tool_response("call_1".to_string(), "ok".to_string())];

    assert!(!has_tool_response_since(&messages, 10));
}

#[test]
fn ensure_post_tool_resume_directive_is_idempotent_near_history_tail() {
    let mut history = vec![
        uni::Message::user("run cargo nextest".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"success\":false}".to_string()),
    ];

    ensure_post_tool_resume_directive(&mut history);
    ensure_post_tool_resume_directive(&mut history);

    let directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(directive_count, 1);
}

#[test]
fn prepare_post_tool_tool_free_recovery_is_idempotent_near_history_tail() {
    let mut history = vec![
        uni::Message::user("summarize the existing tool outputs".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
    ];

    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_RECOVERY_REASON);
    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_RECOVERY_REASON);

    // The resume directive must NOT be injected for tool-free recovery: it
    // instructs the model to follow tool-output guidance, contradicting the
    // tools-disabled synthesis contract.
    let resume_directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(resume_directive_count, 0);

    let recovery_reason_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
        })
        .count();
    assert_eq!(recovery_reason_count, 1);
}

#[test]
fn retryable_post_tool_follow_up_failure_schedules_one_tool_enabled_recovery() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("run cargo nextest".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"critical_note\":\"reuse output\"}".to_string()),
    ];

    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "streaming",
        true,
        true,
        false,
    )
    .expect("recovery should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolEnabled);

    let action_again = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        3,
        1,
        "streaming",
        false,
        false,
        false,
    )
    .expect("repeat recovery should succeed");
    assert_eq!(action_again, PostToolFailureRecovery::StopAfterDirective);

    let enabled_retry_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE
        })
        .count();
    assert_eq!(enabled_retry_count, 1);

    let directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(directive_count, 1);

    let recovery_reason_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
        })
        .count();
    assert_eq!(recovery_reason_count, 0);
}

#[test]
fn plan_mode_recovery_uses_plan_aware_directive() {
    // In plan mode the tool-free recovery pass must inject the plan-aware
    // reason (which demands a `<proposed_plan>` from gathered research) instead
    // of the generic "respond with text" reason — otherwise the model treats
    // the pass as another research step and emits `<invoke>` tool-call markup
    // instead of finalizing the plan (checkpoints turn_648 / turn_650).
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("plan launch-time optimization".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
    ];

    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "streaming",
        true,
        false,
        true,
    )
    .expect("recovery should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolFree);

    let plan_directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON_PLAN_MODE
        })
        .count();
    assert_eq!(plan_directive_count, 1);

    // The generic reason must NOT be injected in plan mode.
    let generic_directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
        })
        .count();
    assert_eq!(generic_directive_count, 0);
}

#[test]
fn retryable_post_tool_follow_up_failure_stops_after_recovery_pass_is_spent() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("summarize the tool output".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
    ];

    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "streaming",
        false,
        false,
        false,
    )
    .expect("recovery classification should succeed");

    assert_eq!(action, PostToolFailureRecovery::StopAfterDirective);
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RECOVERY_REASON
    }));
    // Turn-ending path keeps the resume directive for the next turn.
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
    }));
}

#[test]
fn post_tool_follow_up_failure_chain_consumes_tool_free_recovery_pass() {
    // End-to-end regression guard for the infinite loop: starting from a fresh
    // (non-recovery) turn state (phase == Inactive), a retryable post-tool
    // follow-up failure must schedule a tool-free recovery pass that is
    // actually consumable. Before the fix, `switch_to_tool_free_recovery`
    // left the phase as Inactive, so `consume_recovery_pass()` returned false,
    // `tool_free_recovery` evaluated to false, and tools were never disabled
    // at the API level — the model kept emitting tool calls and the follow-up
    // kept failing, looping until the wall-clock timeout.
    use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("run cargo nextest".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response("call_1".to_string(), "{\"critical_note\":\"reuse output\"}".to_string()),
    ];

    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 4, 10, 1);
    // Fresh turn: recovery is inactive.
    assert!(!state.is_recovery_active());

    // Simulate the turn-loop error path: the follow-up LLM phase failed after
    // tool execution. `allow_tool_free_retry = !tool_free_recovery = true`
    // because this is a non-recovery turn.
    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "execute_llm_request",
        true,
        false,
        false,
    )
    .expect("recovery classification should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolFree);

    // The caller (turn_loop.rs) then switches to tool-free recovery. Before
    // the fix this was a no-op on the phase because it was Inactive.
    assert!(state.switch_to_tool_free_recovery());

    // The next loop iteration consumes the pass — this is the gate that
    // decides `tool_free_recovery = true` and disables tools at the API level.
    assert!(state.consume_recovery_pass(), "consume_recovery_pass must succeed after switch from Inactive");
    assert!(state.recovery_is_tool_free());
}

#[tokio::test]
async fn empty_model_response_after_recovery_is_visible_and_blocked() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_tool_free_recovery_for_test("post-tool follow-up failure");
    let mut history = vec![uni::Message::user("summarize the tool outputs".to_string())];

    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("recovery should produce a visible fallback");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert!(outcome.final_response_was_fallback);
    let final_text = history
        .iter()
        .rev()
        .find(|message| message.role == uni::MessageRole::Assistant)
        .map(|message| message.content.as_text().trim().to_string())
        .unwrap_or_default();
    assert!(!final_text.is_empty(), "recovery must not leave an empty final response");
}

#[tokio::test]
async fn exhausted_budget_survives_empty_recovery_and_final_publication() {
    use crate::agent::runloop::unified::run_loop_context::{ToolBudgetExhaustion, ToolWallClockExhaustion};

    for (directive, cause) in [
        ("Tool loop budget exhausted before a final response (60/60). Tools are disabled for one bounded synthesis pass.".to_string(), "Tool loop budget exhausted before a final response (60/60)"),
        (ToolBudgetExhaustion { used: 32, max: 32, remaining: 0 }.synthesis_directive_message(), "Tool-call budget exhausted for this turn (32/32)"),
        (ToolWallClockExhaustion { max_secs: 73 }.synthesis_directive_message(), "Tool wall-clock budget exhausted for this turn (73s)"),
    ] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let harness_path = backing.enable_harness_emitter();
        backing.activate_tool_free_recovery_for_test(&directive);
        let mut history = vec![uni::Message::user("continue the pending change".to_string())];
        let outcome = run_turn_loop(&mut history, backing.turn_loop_context()).await.unwrap();
        let TurnLoopResult::Blocked { reason: Some(reason) } = outcome.result else { panic!("expected budget handoff") };
        assert!(reason.starts_with(cause), "{reason}");
        assert!(outcome.final_response_was_fallback);
        let text = final_answer_text(&history);
        assert!(text.contains(&cause.replacen("Tool", "tool", 1)), "{text}");
        assert!(text.contains("retained conversation") && text.contains("verification"));
        assert!(!text.contains("Re-state") && !text.contains("please retry"), "{text}");
        assert_blocked_response_surfaces(&mut backing, &history, &harness_path, &cause.replacen("Tool", "tool", 1));
        let harness = fs::read_to_string(harness_path).unwrap();
        assert!(harness.contains(cause), "canonical event must retain budget cause: {harness}");
    }
}

#[tokio::test]
async fn planning_budget_survives_failed_synthesis_on_all_final_surfaces() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let path = backing.enable_harness_emitter();
    let cause = "Tool loop budget exhausted before a final response (7/7)";
    backing.activate_tool_free_recovery_for_test(&format!("{cause}. Tools are disabled for synthesis."));
    let mut history = vec![uni::Message::user("plan the pending change".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context()).await.unwrap();
    let TurnLoopResult::Blocked { reason: Some(reason) } = outcome.result else {
        panic!("expected resumable planning handoff");
    };
    assert_eq!(reason, "planning awaiting input");
    assert!(outcome.final_response_was_fallback);
    assert!(final_answer_text(&history).contains("Planning needs your input"));
    let saved = fs::read_to_string(path).unwrap();
    assert!(saved.contains(cause), "canonical checkpoint must retain the guard cause");
    let ctx = backing.turn_loop_context();
    assert_eq!(ctx.plan_session.recovery.requests_used, 3);
    assert_eq!(
        ctx.plan_session.recovery.outcome,
        vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput
    );
    assert!(ctx.tool_registry.is_planning_active());
}

#[tokio::test]
async fn budget_fallback_keeps_salvage_and_planning_state_without_duplicate_finals() {
    for planning in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        if planning {
            backing.activate_planning_for_test();
        }
        backing.activate_tool_free_recovery_for_test(
            "Tool-call budget exhausted for this turn (32/32). Tools are disabled for the rest of this turn.",
        );
        let mut history = vec![
            uni::Message::user("implement the pending step".to_string()),
            uni::Message::assistant("Parser updated; validation remains pending.".to_string())
                .with_phase(Some(uni::AssistantPhase::FinalAnswer)),
        ];
        let mut ctx = backing.turn_loop_context();
        ctx.harness_state.mark_final_response_fallback();
        assert!(ensure_completed_turn_response(&mut ctx, &mut history, 1).unwrap());
        assert!(ensure_completed_turn_response(&mut ctx, &mut history, 1).unwrap());
        assert_eq!(history.len(), 2);
        let text = history[1].content.as_text();
        assert_eq!(text.matches("tool-call budget exhausted").count(), 1);
        assert!(text.contains("Parser updated; validation remains pending."));
        assert_eq!(text.contains("planning remains active"), planning);
        let reason = completed_recovery_fallback_reason(planning, ctx.harness_state);
        assert!(reason.starts_with("Tool-call budget exhausted for this turn (32/32)."));
        assert_eq!(reason.contains("planning remains active"), planning);
    }
}

#[tokio::test]
async fn genuine_synthesis_after_budget_recovery_is_not_rewritten() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_tool_free_recovery_for_test(
        "Tool-call budget exhausted for this turn (32/32). Tools are disabled for the rest of this turn.",
    );
    let answer = "The requested analysis is complete; no edits were needed.";
    let mut history =
        vec![uni::Message::assistant(answer.to_string()).with_phase(Some(uni::AssistantPhase::FinalAnswer))];
    let mut ctx = backing.turn_loop_context();
    assert!(!ensure_completed_turn_response(&mut ctx, &mut history, 0).unwrap());
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_text(), answer);
    assert!(!ctx.harness_state.final_response_was_fallback());
}

#[tokio::test]
async fn complete_turn_after_failed_tool_free_recovery_appends_fallback_once() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
        None,
        None,
        None,
    )
    .await;
    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    let fallback_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
        })
        .count();
    assert_eq!(fallback_count, 1);

    let outcome_again =
        complete_turn_after_failed_tool_free_recovery(&mut history, "test.stage", None, None, None, None).await;
    assert!(matches!(outcome_again, TurnLoopResult::Completed { .. }));
    let fallback_count_again = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::Assistant
                && message.phase == Some(uni::AssistantPhase::FinalAnswer)
                && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
        })
        .count();
    assert_eq!(fallback_count_again, 1);
}

#[tokio::test]
async fn complete_turn_after_failed_tool_free_recovery_prefers_salvaged_prose() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        None,
        Some("Here is the launch-time plan: reduce config IO.".to_string()),
        None,
        None,
    )
    .await;
    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    let last = history.last().unwrap();
    assert_eq!(last.role, uni::MessageRole::Assistant);
    assert_eq!(last.phase, Some(uni::AssistantPhase::FinalAnswer));
    let text = last.content.as_text();
    assert!(text.contains("reduce config IO"));
    assert!(text != RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER);

    // Whitespace-only salvage falls back to the canned answer.
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        None,
        Some("   \n".to_string()),
        None,
        None,
    )
    .await;
    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert_eq!(history.last().unwrap().content.as_text(), RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER);
}

#[tokio::test]
async fn normalize_tool_free_recovery_break_outcome_converts_contract_violation_to_completed() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = normalize_tool_free_recovery_break_outcome(
        &mut history,
        TurnLoopResult::Blocked {
            reason: Some(RECOVERY_CONTRACT_VIOLATION_REASON.to_string()),
        },
        true,
        None,
        None,
        None,
    )
    .await;

    assert!(matches!(outcome, TurnLoopResult::Completed { .. }));
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    }));
}

#[tokio::test]
async fn normalize_tool_free_recovery_break_outcome_keeps_non_recovery_blocked_result() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = normalize_tool_free_recovery_break_outcome(
        &mut history,
        TurnLoopResult::Blocked {
            reason: Some("Stopped after reaching budget limit.".to_string()),
        },
        true,
        None,
        None,
        None,
    )
    .await;

    assert!(matches!(
        outcome,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason == "Stopped after reaching budget limit."
    ));
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    }));
}

/// End-to-end regression test for the tool-free recovery contract-violation
/// retry (checkpoint turn_621): when the model emits textual tool-call markup
/// during a tool-free synthesis pass instead of prose, the turn loop must
/// retry up to `MAX_RECOVERY_RETRIES` times with a corrective directive rather
/// than immediately concluding with the canned fallback answer. After retries
/// are exhausted, the turn must conclude with the salvaged prose from the
/// rejected synthesis response.
#[tokio::test]
async fn tool_free_recovery_retries_on_contract_violation_then_salvages() {
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct ContractViolationProvider {
        requests: Arc<Mutex<usize>>,
        content: String,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for ContractViolationProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            *self.requests.lock().expect("requests lock") += 1;
            Ok(uni::LLMResponse {
                content: Some(self.content.clone()),
                model: request.model.clone(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }
        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_tool_free_recovery_for_test("post-tool follow-up failure");

    // Markup with surrounding prose so the salvage step has non-trivial text.
    // A dangling `</tool_call>` close tag trips `contains_pseudo_tool_call_markers`
    // (so the recovery guard fires) but has no matching opening `<tool_call>`
    // for `strip_textual_tool_call_regions` to remove, so the response cannot
    // be "cleaned" into a valid final answer. The turn must retry, then salvage.
    let markup = "Here is my plan: the change was not applied because tools were disabled. \
                  </tool_call> Please re-run with tools enabled.";
    let requests = Arc::new(Mutex::new(0usize));
    backing.set_provider(Box::new(ContractViolationProvider {
        requests: requests.clone(),
        content: markup.to_string(),
    }));

    let mut history = vec![uni::Message::user("summarize the tool outputs".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("turn loop should complete after recovery retries");

    // Exactly MAX_RECOVERY_RETRIES retries: 1 initial recovery pass + 3 retries.
    assert_eq!(
        *requests.lock().expect("requests lock"),
        MAX_RECOVERY_RETRIES as usize + 1,
        "recovery must retry exactly MAX_RECOVERY_RETRIES times before falling back"
    );

    // The turn must conclude with the salvaged prose, not the canned string.
    let final_text = history
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::Assistant)
        .map(|m| m.content.as_text().to_string())
        .unwrap_or_default();
    assert!(final_text.contains("Here is my plan:"), "expected salvaged prose, got: {final_text}");
    assert!(
        !final_text.contains(RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER),
        "must not emit canned fallback when salvage is available"
    );
    assert!(backing.recovery_is_tool_free());
}

#[tokio::test]
async fn navigation_blocked_internal_turn_cannot_reopen_execution() {
    use crate::agent::runloop::unified::state::NAVIGATION_RECOVERY_BLOCK_REASON;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
    let mut backing = TestTurnProcessingBacking::new(20).await;
    {
        let ctx = backing.turn_processing_context();
        ctx.session_stats.navigation_recovery.arm();
        assert!(ctx.session_stats.navigation_recovery.consume());
        ctx.session_stats.navigation_recovery.arm();
        ctx.session_stats
            .register_follow_up_prompt("Preflight corrective turn: continue the task");
    }
    let mut history = vec![uni::Message::user(
        "Preflight corrective turn: continue the task".into(),
    )];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context()).await.unwrap();
    assert!(
        matches!(outcome.result, TurnLoopResult::Blocked { reason: Some(ref reason) } if reason == NAVIGATION_RECOVERY_BLOCK_REASON)
    );
    let ctx = backing.turn_processing_context();
    assert_eq!(ctx.harness_state.tool_calls, 0);
    assert!(!ctx.is_recovery_active());
}
