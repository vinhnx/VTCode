use super::post_tool_recovery::prepare_post_tool_tool_free_recovery;
use super::post_tool_recovery::{ensure_post_tool_resume_directive, has_tool_response_since};
use super::{
    HarnessUsage, POST_TOOL_RECOVERY_REASON, POST_TOOL_RESUME_DIRECTIVE, PostToolFailureRecovery,
    RECOVERY_CONTRACT_VIOLATION_REASON, RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER,
    accumulate_turn_usage, complete_turn_after_failed_tool_free_recovery,
    count_assistant_text_responses_for_guard, count_assistant_text_responses_in_turn,
    has_turn_usage, maybe_recover_after_post_tool_llm_failure,
    normalize_tool_free_recovery_break_outcome, run_turn_loop,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use anyhow::anyhow;
use serde_json::json;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::InlineHandle;

#[test]
fn recovery_synthesis_fallback_says_no_tool_call_was_applied() {
    assert!(RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER.contains("no tool call applied"));
}

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

    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_RECOVERY_REASON);
    prepare_post_tool_tool_free_recovery(&mut history, POST_TOOL_RECOVERY_REASON);

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
                && message.content.as_text() == POST_TOOL_RECOVERY_REASON
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
        uni::Message::tool_response(
            "call_1".to_string(),
            "{\"critical_note\":\"reuse output\"}".to_string(),
        ),
    ];

    let mut state = HarnessTurnState::new(
        TurnRunId("run-1".to_string()),
        TurnId("turn-1".to_string()),
        4,
        10,
        1,
    );
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
    )
    .expect("recovery classification should succeed");
    assert_eq!(action, PostToolFailureRecovery::RetryToolFree);

    // The caller (turn_loop.rs) then switches to tool-free recovery. Before
    // the fix this was a no-op on the phase because it was Inactive.
    assert!(state.switch_to_tool_free_recovery());

    // The next loop iteration consumes the pass — this is the gate that
    // decides `tool_free_recovery = true` and disables tools at the API level.
    assert!(
        state.consume_recovery_pass(),
        "consume_recovery_pass must succeed after switch from Inactive"
    );
    assert!(state.recovery_is_tool_free());
}

#[test]
fn complete_turn_after_failed_tool_free_recovery_appends_fallback_once() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        Some(&anyhow!("Network error")),
        None,
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
        complete_turn_after_failed_tool_free_recovery(&mut history, "test.stage", None, None);
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
fn complete_turn_after_failed_tool_free_recovery_prefers_salvaged_prose() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = complete_turn_after_failed_tool_free_recovery(
        &mut history,
        "test.stage",
        None,
        Some("Here is the launch-time plan: reduce config IO.".to_string()),
    );
    assert!(matches!(outcome, TurnLoopResult::Completed));
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
    );
    assert!(matches!(outcome, TurnLoopResult::Completed));
    assert_eq!(
        history.last().unwrap().content.as_text(),
        RECOVERY_SYNTHESIS_FALLBACK_FINAL_ANSWER
    );
}

#[test]
fn normalize_tool_free_recovery_break_outcome_converts_contract_violation_to_completed() {
    let mut history = vec![uni::Message::user("summarize".to_string())];
    let outcome = normalize_tool_free_recovery_break_outcome(
        &mut history,
        TurnLoopResult::Blocked {
            reason: Some(RECOVERY_CONTRACT_VIOLATION_REASON.to_string()),
        },
        true,
        None,
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
        None,
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
            iterations: None,
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
            iterations: None,
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

#[test]
fn count_assistant_text_responses_in_turn_zero_for_empty_history() {
    let history: Vec<uni::Message> = Vec::new();
    assert_eq!(count_assistant_text_responses_in_turn(&history, 0), 0);
}

#[test]
#[allow(clippy::vec_init_then_push)]
fn count_assistant_text_responses_in_turn_skips_tool_call_messages() {
    let mut history: Vec<uni::Message> = Vec::new();
    // First assistant message has a tool call -> not counted
    history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "tool_call_0".to_string(),
            "unified_search".to_string(),
            "{}".to_string(),
        )],
    ));
    // Second assistant message is plain text -> counted
    history.push(uni::Message::assistant(
        "Functions and structs.".to_string(),
    ));
    // System message -> not counted
    history.push(uni::Message::system("Tools disabled.".to_string()));
    // Third assistant message is plain text -> counted
    history.push(uni::Message::assistant(
        "Functions and structs again.".to_string(),
    ));
    // Empty assistant content -> not counted
    history.push(uni::Message::assistant(String::new()));
    // Whitespace-only assistant content -> not counted
    history.push(uni::Message::assistant("   \n  ".to_string()));

    assert_eq!(count_assistant_text_responses_in_turn(&history, 0), 2);
}

#[test]
fn count_assistant_text_responses_in_turn_ignores_history_before_baseline() {
    let mut history = vec![
        uni::Message::user("previous request".to_string()),
        uni::Message::assistant("previous answer one".to_string()),
        uni::Message::assistant("previous answer two".to_string()),
    ];
    let turn_history_start_len = history.len();

    assert_eq!(
        count_assistant_text_responses_in_turn(&history, turn_history_start_len),
        0,
        "historical assistant text before the current turn must not count"
    );
    assert_eq!(
        count_assistant_text_responses_for_guard(&history, turn_history_start_len, 0),
        0,
        "the guard must not count historical assistant text when the per-turn floor is empty"
    );

    history.push(uni::Message::assistant("current answer one".to_string()));
    assert_eq!(
        count_assistant_text_responses_in_turn(&history, turn_history_start_len),
        1
    );

    history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "tool_call_0".to_string(),
            "unified_search".to_string(),
            "{}".to_string(),
        )],
    ));
    history.push(uni::Message::assistant("current answer two".to_string()));

    assert_eq!(
        count_assistant_text_responses_in_turn(&history, turn_history_start_len),
        super::MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN,
        "current-turn assistant text after the baseline still trips the cap"
    );
}

#[test]
fn count_assistant_text_responses_in_turn_counts_after_compaction_rebase() {
    let stale_turn_history_start_len = 5;
    let mut compacted_history = vec![
        uni::Message::system("Compacted history summary.".to_string()),
        uni::Message::user("current request".to_string()),
    ];
    let rebased_turn_history_start_len = compacted_history.len();

    compacted_history.push(uni::Message::assistant(
        "current answer after compaction".to_string(),
    ));

    assert_eq!(
        count_assistant_text_responses_in_turn(&compacted_history, stale_turn_history_start_len),
        0,
        "a stale pre-compaction baseline misses newly appended assistant text"
    );
    assert_eq!(
        count_assistant_text_responses_in_turn(&compacted_history, rebased_turn_history_start_len),
        1,
        "rebasing to the compacted length counts current-turn assistant text promptly"
    );
}

#[test]
fn count_assistant_text_responses_for_guard_preserves_pre_compaction_turn_floor() {
    let compacted_history = vec![
        uni::Message::system("Compacted history summary.".to_string()),
        uni::Message::user("current request".to_string()),
    ];
    let rebased_turn_history_start_len = compacted_history.len();
    let recorded_text_responses_in_turn = 1;

    assert_eq!(
        count_assistant_text_responses_in_turn(&compacted_history, rebased_turn_history_start_len),
        0,
        "history slice cannot see same-turn assistant text removed by compaction"
    );
    assert_eq!(
        count_assistant_text_responses_for_guard(
            &compacted_history,
            rebased_turn_history_start_len,
            recorded_text_responses_in_turn,
        ),
        recorded_text_responses_in_turn,
        "guard uses the per-turn counter as a compaction-safe floor"
    );
}

#[test]
fn count_assistant_text_responses_for_guard_counts_post_compaction_growth_above_floor() {
    let mut compacted_history = vec![
        uni::Message::system("Compacted history summary.".to_string()),
        uni::Message::user("current request".to_string()),
    ];
    let rebased_turn_history_start_len = compacted_history.len();
    let recorded_text_responses_in_turn = 1;

    compacted_history.push(uni::Message::assistant(
        "current answer after compaction".to_string(),
    ));
    compacted_history.push(uni::Message::assistant(
        "second current answer after compaction".to_string(),
    ));

    assert_eq!(
        count_assistant_text_responses_for_guard(
            &compacted_history,
            rebased_turn_history_start_len,
            recorded_text_responses_in_turn,
        ),
        2,
        "new assistant text appended after compaction still counts promptly"
    );
}

#[test]
fn count_assistant_text_responses_in_turn_matches_observed_pattern() {
    // Reproduces the checkpoint turn_594 failure mode: 4 long identical
    // outline responses in a single turn.  The count must be 4 so the
    // anti-runaway guard at MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN (2) trips
    // on the third iteration.
    let mut history: Vec<uni::Message> = Vec::new();
    for _ in 0..4 {
        history.push(uni::Message::assistant(
            "# Functions and Structs in vtcode-core/src/tools/registry\n\
             \n\
             The directory has 70 files, 23 structs, 69 functions, 11 enums.\n\
             \n\
             ## Structs (23)\n\
             \n\
             | File | Struct |\n\
             |---|---|\n\
             | mod.rs | ToolRegistry |\n\
             | distributed.rs | ToolConfigSnapshot |\n\
             ... (and many more rows)\n"
                .to_string(),
        ));
    }
    assert_eq!(count_assistant_text_responses_in_turn(&history, 0), 4);
    assert!(
        count_assistant_text_responses_in_turn(&history, 0)
            >= super::MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN,
        "anti-runaway guard would trip on this history"
    );
}
