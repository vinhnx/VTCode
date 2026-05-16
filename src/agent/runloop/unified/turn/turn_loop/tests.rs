use super::{
    HarnessUsage, POST_TOOL_RECOVERY_REASON, POST_TOOL_RESUME_DIRECTIVE,
    POST_TOOL_TIMEOUT_RECOVERY_REASON, PostToolFailureRecovery,
    RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER, accumulate_turn_usage,
    complete_turn_after_failed_tool_free_recovery, has_turn_usage,
    maybe_recover_after_post_tool_llm_failure, normalize_tool_free_recovery_break_outcome,
    prepare_post_tool_tool_free_recovery, run_turn_loop,
};
use super::post_tool_recovery::{ensure_post_tool_resume_directive, has_tool_response_since};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use anyhow::anyhow;
use serde_json::json;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::InlineHandle;

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
    let messages = vec![uni::Message::tool_response(
        "call_1".to_string(),
        "ok".to_string(),
    )];

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
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
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

    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_TIMEOUT_RECOVERY_REASON);
    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_TIMEOUT_RECOVERY_REASON);

    let resume_directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(resume_directive_count, 1);

    let recovery_reason_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_TIMEOUT_RECOVERY_REASON
        })
        .count();
    assert_eq!(recovery_reason_count, 1);
}

#[test]
fn retryable_post_tool_follow_up_failure_schedules_tool_free_recovery_once() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut history = vec![
        uni::Message::user("run cargo nextest".to_string()),
        uni::Message::assistant("".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            "{\"critical_note\":\"reuse output\"}".to_string(),
        ),
    ];

    let action = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        2,
        1,
        "streaming",
        true,
    )
    .expect("recovery should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolFree);

    let action_again = maybe_recover_after_post_tool_llm_failure(
        &mut renderer,
        &mut history,
        &anyhow!("Network error"),
        3,
        1,
        "streaming",
        true,
    )
    .expect("repeat recovery should succeed");
    assert_eq!(action_again, PostToolFailureRecovery::RetryToolFree);

    let directive_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_RESUME_DIRECTIVE
        })
        .count();
    assert_eq!(directive_count, 1);

    let recovery_reason_count = history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text() == POST_TOOL_RECOVERY_REASON
        })
        .count();
    assert_eq!(recovery_reason_count, 1);
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
    )
    .expect("recovery classification should succeed");

    assert_eq!(action, PostToolFailureRecovery::StopAfterDirective);
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message.content.as_text() == POST_TOOL_RECOVERY_REASON
    }));
}

#[test]
fn complete_turn_after_failed_tool_free_recovery_appends_fallback_once() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
    );
    assert!(matches!(outcome, TurnLoopResult::Completed));
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
        complete_turn_after_failed_tool_free_recovery(&mut history, "test.stage", None);
    assert!(matches!(outcome_again, TurnLoopResult::Completed));
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

#[test]
fn normalize_tool_free_recovery_break_outcome_converts_contract_violation_to_completed() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = normalize_tool_free_recovery_break_outcome(
        &mut history,
        TurnLoopResult::Blocked {
            reason: Some(
                "Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls."
                    .to_string(),
            ),
        },
        true,
    );

    assert!(matches!(outcome, TurnLoopResult::Completed));
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text() == RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    }));
}

#[test]
fn normalize_tool_free_recovery_break_outcome_keeps_non_recovery_blocked_result() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = normalize_tool_free_recovery_break_outcome(
        &mut history,
        TurnLoopResult::Blocked {
            reason: Some("Stopped after reaching budget limit.".to_string()),
        },
        true,
    );

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

#[test]
fn accumulate_turn_usage_merges_prompt_completion_and_cached_tokens() {
    let mut total = HarnessUsage::default();

    accumulate_turn_usage(
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
            cached_prompt_tokens: Some(15),
            cache_creation_tokens: None,
            cache_read_tokens: Some(15),
        }),
    );
    accumulate_turn_usage(
        &mut total,
        &Some(uni::Usage {
            prompt_tokens: 40,
            completion_tokens: 10,
            total_tokens: 50,
            cached_prompt_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        }),
    );

    assert_eq!(total.input_tokens, 140);
    assert_eq!(total.cached_input_tokens, 15);
    assert_eq!(total.output_tokens, 30);
    assert!(has_turn_usage(&total));
}

#[tokio::test]
async fn turn_loop_preserves_legacy_loop_detector_state() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_loop_limit(tool_names::READ_FILE, 2);
    let seeded_args = json!({"path":"sample.txt"});
    assert!(
        backing
            .record_tool_call(tool_names::READ_FILE, &seeded_args)
            .is_none()
    );
    let _ = backing.record_tool_call(tool_names::READ_FILE, &seeded_args);
    let warning = backing.record_tool_call(tool_names::READ_FILE, &seeded_args);
    assert!(warning.is_some());
    assert!(backing.is_hard_limit_exceeded(tool_names::READ_FILE));

    let mut history = vec![uni::Message::user("continue".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("turn loop should complete");

    assert!(backing.is_hard_limit_exceeded(tool_names::READ_FILE));
}