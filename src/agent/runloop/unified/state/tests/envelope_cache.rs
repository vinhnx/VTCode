use super::*;

#[test]
fn mcp_ready_notice_is_claimed_exactly_once_per_session() {
    let mut stats = SessionStats::default();
    assert!(stats.take_mcp_ready_notice(), "first catalogue wiring must render");
    assert!(
        !stats.take_mcp_ready_notice(),
        "agent switches and manual /mcp refreshes must not repeat the notice"
    );
}
#[test]
fn mcp_ready_notice_presuppression_does_not_render_later() {
    let mut stats = SessionStats::default();
    stats.mark_mcp_ready_notice_shown();
    assert!(!stats.take_mcp_ready_notice(), "a catalogue attached before the interaction loop must stay silent");
}
#[test]
fn compaction_parent_context_reuses_arcs_only_for_matching_identity() {
    let mut stats = SessionStats::default();
    assert!(stats.compaction_parent_context("openai", "model-a").is_none());
    let envelope = stats.request_envelope(
        "model-a",
        "openai",
        "build",
        "fixed instructions".into(),
        vec![function_tool("zeta"), function_tool("alpha")],
        7,
    );
    assert!(stats.compaction_parent_context("anthropic", "model-a").is_none());
    assert!(stats.compaction_parent_context("openai", "model-b").is_none());
    let parent = stats.compaction_parent_context("openai", "model-a").unwrap();
    assert!(Arc::ptr_eq(parent.system_prompt.as_ref().unwrap(), &envelope.system_prompt()));
    assert!(Arc::ptr_eq(parent.tools.as_ref().unwrap(), &envelope.ordered_tools()));
    stats.begin_request_segment(SegmentBoundaryReason::Compaction);
    assert!(stats.compaction_parent_context("openai", "model-a").is_none());
    stats.request_envelope("model-b", "openai", "build", "new instructions".into(), vec![], 7);
    let parent = stats.compaction_parent_context("openai", "model-b").unwrap();
    assert_eq!(parent.system_prompt.as_deref(), Some("new instructions"));
    assert!(parent.tools.is_none());
}
#[test]
fn preflight_internal_followups_preserve_episode_and_user_input_resets_it() {
    let mut stats = SessionStats::default();
    stats.record_tracker_continuation_turn_with_limit(3);
    stats.preflight_recovery.reject(
        "task_tracker",
        ValidationCategory::Arguments,
        Some(&serde_json::json!({"action":"list","index":1})),
        6,
        false,
    );
    stats.preflight_recovery.synthesis();
    stats.preflight_recovery.begin_corrective_turn();
    stats.register_follow_up_prompt(&format!("{PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX} repair arguments"));
    stats.preflight_recovery.finish_corrective_turn();
    assert!(stats.preflight_recovery.exhausted());
    stats.register_follow_up_prompt("The task tracker still has incomplete steps: verify");
    assert!(stats.preflight_recovery.exhausted());
    assert_eq!(stats.tracker_continuation_turns(), 1);
    stats.register_follow_up_prompt("continue");
    assert!(!stats.preflight_recovery.exhausted());
    assert!(stats.preflight_recovery.is_clear());
    assert_eq!(stats.tracker_continuation_turns(), 1, "user resume retains existing tracker no-progress accounting");
}
#[test]
fn decisions_probe_accounting_preserves_unknown_cost_and_does_not_feed_cache_health() {
    let mut stats = SessionStats::default();
    let usage = Some(vtcode_core::llm::provider::Usage {
        prompt_tokens: 2048,
        completion_tokens: 0,
        total_tokens: 2048,
        cached_prompt_tokens: Some(1),
        cache_read_tokens: Some(1),
        cache_creation_tokens: None,
        iterations: None,
    });
    for _ in 0..3 {
        stats.record_probe_attempt(
            "openai",
            usage.as_ref(),
            usage.as_ref().map(vtcode_core::llm::usage_cost::estimate_decisions_cost),
        );
    }
    assert_eq!(stats.total_usage().input_tokens, 6144);
    assert!((stats.total_cost_usd().unwrap() - 0.0006144).abs() < 1e-12);
    assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_none());
    assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_none());
    assert!(stats.record_cache_turn_health("openai", usage.as_ref()).is_some());
    stats.record_probe_attempt("openai", None, None);
    assert!(stats.total_cost_usd().is_none());
    stats.record_probe_attempt(
        "openai",
        usage.as_ref(),
        usage.as_ref().map(vtcode_core::llm::usage_cost::estimate_decisions_cost),
    );
    assert!(stats.total_cost_usd().is_none());
}
#[test]
fn compaction_boundary_does_not_join_cache_misses_across_replay_windows() {
    let mut stats = SessionStats::default();
    let usage = |input, cached, creation| {
        Some(vtcode_core::llm::provider::Usage {
            prompt_tokens: input,
            cached_prompt_tokens: Some(cached),
            cache_read_tokens: Some(cached),
            cache_creation_tokens: Some(creation),
            ..Default::default()
        })
    };
    for sample in [usage(12_645, 0, 12_642), usage(12_669, 5_710, 5_575)] {
        stats.record_usage("openai", sample.as_ref());
        assert!(stats.record_cache_turn_health("openai", sample.as_ref()).is_none());
    }
    stats.begin_request_segment(SegmentBoundaryReason::Compaction);
    let sample = usage(12_740, 0, 5_212);
    stats.record_usage("openai", sample.as_ref());
    assert!(stats.record_cache_turn_health("openai", sample.as_ref()).is_none());
    assert_eq!(stats.total_usage().input_tokens, 38_054);
    assert_eq!(stats.total_usage().cached_input_tokens, 5_710);
    assert!(stats.record_cache_turn_health("openai", sample.as_ref()).is_none());
    assert!(stats.record_cache_turn_health("openai", sample.as_ref()).is_some());
}
