use super::*;

#[test]
fn route_policy_defaults_to_status_quo_shape() {
    use super::CompactionRoutePolicy;

    let policy = CompactionRoutePolicy::resolve("unknown-provider", "unknown-model");
    assert_eq!(policy, CompactionRoutePolicy::default());
    assert!((policy.threshold_ratio - 1.0).abs() < f64::EPSILON);
    assert!((policy.retain_ratio - 0.16).abs() < f64::EPSILON);
    assert_eq!(policy.max_overflow_retries, 1);
}

#[test]
fn route_tail_target_scales_with_window() {
    use super::{CONTINUITY_TAIL_TARGET_TOKENS, CompactionRoutePolicy};

    let policy = CompactionRoutePolicy::default();
    // Unknown windows keep the legacy constant.
    assert_eq!(policy.tail_target_tokens(0), CONTINUITY_TAIL_TARGET_TOKENS);
    // Large windows keep the legacy constant exactly.
    assert_eq!(policy.tail_target_tokens(1_000_000), CONTINUITY_TAIL_TARGET_TOKENS);
    assert_eq!(policy.tail_target_tokens(128_000), CONTINUITY_TAIL_TARGET_TOKENS);
    // Small windows scale down so the summary survives output bounding.
    assert_eq!(policy.tail_target_tokens(32_768), 5_242);
    // Tiny windows keep a meaningful floor instead of collapsing to zero.
    assert_eq!(policy.tail_target_tokens(4_096), 1_024);
}

#[test]
fn compacted_history_shrinks_rejects_growth() {
    use super::compacted_history_shrinks;
    use crate::exec::events::CompactionMode;

    // Reported `86 -> 88` regression: a rebuild that keeps every message
    // plus framing must be discarded, accounting for the envelope message
    // Local persistence injects.
    assert!(!compacted_history_shrinks(86, 87, CompactionMode::Local));
    assert!(!compacted_history_shrinks(12, 12, CompactionMode::Local));
    assert!(!compacted_history_shrinks(0, 0, CompactionMode::Local));
    // Genuine compression passes, including the envelope slot. Note a
    // one-message reduction is still rejected for Local mode: the
    // envelope re-adds it, netting zero.
    assert!(compacted_history_shrinks(88, 11, CompactionMode::Local));
    assert!(compacted_history_shrinks(12, 10, CompactionMode::Local));
    assert!(!compacted_history_shrinks(12, 11, CompactionMode::Local));
    // Provider-native windows carry no envelope framing, so the raw
    // lengths compare directly.
    assert!(compacted_history_shrinks(12, 11, CompactionMode::Provider));
    assert!(!compacted_history_shrinks(12, 12, CompactionMode::Provider));
    assert!(!compacted_history_shrinks(12, 13, CompactionMode::Provider));
}

#[test]
fn threshold_cap_only_fires_earlier() {
    use super::CompactionRoutePolicy;

    let policy = CompactionRoutePolicy::default();
    assert_eq!(policy.apply_threshold_cap(900_000, 1_000_000), 900_000);
    let eager = CompactionRoutePolicy {
        threshold_ratio: 0.7,
        ..CompactionRoutePolicy::default()
    };
    assert_eq!(eager.apply_threshold_cap(900_000, 1_000_000), 700_000);
    assert_eq!(eager.apply_threshold_cap(500_000, 1_000_000), 500_000);
    assert_eq!(eager.apply_threshold_cap(900_000, 0), 900_000);
}

#[test]
fn prune_oversized_tool_outputs_trims_only_tool_dumps() {
    use super::{TOOL_RESULT_PRUNE_TARGET_TOKENS, prune_oversized_tool_outputs};

    let user = Message::user("do the thing".to_string());
    let mut assistant = Message::assistant("calling tool".to_string());
    assistant.tool_calls = Some(vec![ToolCall::function(
        "call-big".to_string(),
        "read_file".to_string(),
        "{}".to_string(),
    )]);
    let huge = {
        let mut message = Message::tool_response("call-big".to_string(), "data ".repeat(20_000));
        message.tool_call_id = Some("call-big".to_string());
        message
    };
    let small = Message::tool_response("call-small".to_string(), "ok".to_string());
    let history = vec![user.clone(), assistant.clone(), huge.clone(), small.clone()];

    let pruned = prune_oversized_tool_outputs(&history);
    assert_eq!(pruned.len(), history.len());
    // Untouched roles pass through verbatim.
    assert_eq!(pruned[0].content.as_text(), user.content.as_text());
    assert_eq!(pruned[1].content.as_text(), assistant.content.as_text());
    // The dump shrinks within budget while the small result is identical.
    // Previews keep the head: the start survives, the tail does not.
    let huge_text = huge.content.as_text();
    let pruned_text = pruned[2].content.as_text();
    assert!(pruned[2].estimate_tokens() < huge.estimate_tokens(), "oversized tool output must shrink");
    assert!(
        pruned[2].estimate_tokens() <= TOOL_RESULT_PRUNE_TARGET_TOKENS + 32,
        "pruned output must respect the cap, used {}",
        pruned[2].estimate_tokens()
    );
    assert!(pruned_text.len() < huge_text.len(), "preview must be shorter than the dump");
    assert!(huge_text.starts_with(pruned_text.trim_end_matches('.')), "preview must be a head truncation");
    assert_eq!(pruned[2].tool_call_id.as_deref(), Some("call-big"), "pairing ID must survive");
    assert_eq!(pruned[3].content.as_text(), small.content.as_text());
}
