use super::*;

#[test]
fn tool_catalog_observability_is_change_only() {
    let mut stats = SessionStats::default();
    let tools = vec!["exec_command".to_string(), "search_tools".to_string()];
    let skills = vec!["rust".to_string()];

    assert!(stats.note_tool_catalog_observability_change(&tools, 5, 2, 3, &skills));
    assert!(!stats.note_tool_catalog_observability_change(&tools, 5, 2, 3, &skills));
    assert!(stats.note_tool_catalog_observability_change(&tools, 6, 2, 4, &skills));
}
#[test]
fn stream_timeout_billing_advisory_fires_once() {
    let mut stats = SessionStats::default();
    let first = stats.stream_timeout_billing_advisory("merge-gateway");
    assert!(first.is_some());
    let second = stats.stream_timeout_billing_advisory("merge-gateway");
    assert!(second.is_none());
    let mut other = SessionStats::default();
    assert!(other.stream_timeout_billing_advisory("ollama").is_none());
    let mut remote = SessionStats::default();
    let openai = remote.stream_timeout_billing_advisory("openai");
    assert!(openai.is_some());
    assert!(openai.unwrap().contains("full prompt"));
    assert!(remote.stream_timeout_billing_advisory("anthropic").is_none());
}
#[test]
fn model_change_advisory_fires_once_per_switch() {
    let mut stats = SessionStats::default();
    // Session start records "model" but must not advise: there is no
    // prior cache to invalidate.
    assert_eq!(stats.record_prompt_cache_fingerprint("model-a", 1, Some(2)), "model");
    assert_eq!(stats.model_change_advisory(), None);
    // Same model, stable prefix: no advisory.
    assert_eq!(stats.record_prompt_cache_fingerprint("model-a", 1, Some(2)), "unchanged");
    assert_eq!(stats.model_change_advisory(), None);
    // Genuine switch: advisory fires on the first request carrying it.
    assert_eq!(stats.record_prompt_cache_fingerprint("model-b", 1, Some(2)), "model");
    let advisory = stats.model_change_advisory().expect("switch must advise");
    assert!(advisory.contains("Model changed mid-session"));
    // Settled on the new model: advisory clears without any flag.
    assert_eq!(stats.record_prompt_cache_fingerprint("model-b", 1, Some(2)), "unchanged");
    assert_eq!(stats.model_change_advisory(), None);
    // Switching back advises again: each genuine change re-pays.
    assert_eq!(stats.record_prompt_cache_fingerprint("model-a", 1, Some(2)), "model");
    assert!(stats.model_change_advisory().is_some());
}
#[test]
fn first_call_composition_captures_once() {
    use super::super::FirstCallComposition;
    let mut stats = SessionStats::default();
    assert!(stats.first_call_composition().is_none());
    let first = FirstCallComposition {
        system_prompt_tokens: 1_100,
        tool_schema_tokens: 2_100,
        message_history_tokens: 40,
        on_wire_tools: 4,
    };
    assert!(stats.record_first_call_composition(first));
    assert_eq!(stats.first_call_composition(), Some(first));
    // Later assemblies must not overwrite the first-call snapshot.
    let later = FirstCallComposition {
        system_prompt_tokens: 9_000,
        tool_schema_tokens: 9_000,
        message_history_tokens: 9_000,
        on_wire_tools: 40,
    };
    assert!(!stats.record_first_call_composition(later));
    assert_eq!(stats.first_call_composition(), Some(first));
    assert_eq!(first.fixed_overhead_tokens(), 3_200);
}
#[test]
fn internal_continuations_cannot_reset_cross_turn_budgets() {
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
        plan_mode_continue_follow_up, recoverable_blocked_continue_follow_up, tracker_continue_follow_up,
    };

    let prompts = [
        tracker_continue_follow_up(&["#1 README (blocked)".to_owned()]),
        plan_mode_continue_follow_up(),
        recoverable_blocked_continue_follow_up("preview budget exhausted"),
    ];
    for prompt in prompts {
        let mut stats = SessionStats::default();
        for turn in 1..=3 {
            assert!(stats.record_tracker_continuation_turn_with_limit(3));
            assert!(stats.record_plan_continuation_turn_with_limit(3));
            assert!(stats.record_verification_auto_recovery_turn_with_limit(3));
            assert_eq!(stats.register_follow_up_prompt(&prompt), FollowUpPromptAction::None);
            assert_eq!(stats.tracker_continuation_turns(), turn);
            assert_eq!(stats.plan_continuation_turns(), turn);
            assert_eq!(stats.verification_auto_recovery_turns(), turn);
        }
        assert!(!stats.record_tracker_continuation_turn_with_limit(3));
        assert!(!stats.record_plan_continuation_turn_with_limit(3));
        assert!(!stats.record_verification_auto_recovery_turn_with_limit(3));
        assert_eq!(stats.register_follow_up_prompt("Fix the README links"), FollowUpPromptAction::None);
        assert_eq!(stats.tracker_continuation_turns(), 0);
        assert_eq!(stats.verification_auto_recovery_turns(), 0);
    }
}
#[test]
fn record_tool_normalizes_exec_aliases() {
    let mut stats = SessionStats::default();
    stats.record_tool(tools::UNIFIED_EXEC);
    stats.record_tool("shell");
    stats.record_tool("exec_pty_cmd");
    stats.record_tool(tools::EXEC_COMMAND);

    assert_eq!(stats.sorted_tools(), vec![tools::UNIFIED_EXEC.to_string()]);
}
/// Thin delegation check: `SessionStats` forwards to the embedded
/// `RequestGapTracker` correctly. Exhaustive edge-case coverage
/// (no-prior-request, below-threshold, above-threshold) lives on
/// `RequestGapTracker`'s own unit tests in
/// `vtcode_core::llm::request_gap`.
#[test]
fn cache_gap_exceeds_delegates_to_request_gap_tracker() {
    let mut stats = SessionStats::default();
    assert!(!stats.has_sent_request());
    assert_eq!(stats.cache_gap_exceeds(Duration::from_millis(1)), None);

    stats.note_request_sent();
    assert!(stats.has_sent_request());
    thread::sleep(Duration::from_millis(15));
    let gap = stats.cache_gap_exceeds(Duration::from_millis(5));
    assert!(gap.is_some_and(|elapsed| elapsed >= Duration::from_millis(15)));
}
