use super::{Message, bound_history_for_native_compaction, bound_history_for_summarization};

const INSTRUCTIONS: &str = "Summarize now.";

fn total_tokens(messages: &[Message]) -> usize {
    messages.iter().map(Message::estimate_tokens).sum()
}

#[test]
fn keeps_history_verbatim_when_it_already_fits() {
    let history = vec![Message::user("a".repeat(4_000)), Message::user("b".repeat(4_000))];
    let bounded = bound_history_for_summarization(&history, INSTRUCTIONS, Some(10_000_000));
    assert_eq!(bounded.len(), history.len());
    assert_eq!(total_tokens(&bounded), total_tokens(&history));
}

#[test]
fn keeps_history_verbatim_when_budget_is_unknown() {
    let history = vec![Message::user("x".repeat(40_000))];
    let bounded = bound_history_for_summarization(&history, INSTRUCTIONS, None);
    assert_eq!(bounded.len(), 1);
    assert_eq!(total_tokens(&bounded), total_tokens(&history));
}

#[test]
fn trims_oldest_groups_when_over_budget_and_keeps_the_newest_turn() {
    let history = vec![
        Message::user("old".repeat(2_000)),
        Message::user("middle".repeat(2_000)),
        Message::user("newest".repeat(64)),
    ];
    // Derive the limit from the measured fixture so this remains a
    // genuine over-budget case across tokenizer changes.
    let budget = total_tokens(&history).saturating_sub(1);
    let bounded = bound_history_for_summarization(&history, INSTRUCTIONS, Some(budget));
    assert!(bounded.len() < history.len(), "expected trimming, kept {}", bounded.len());
    assert_eq!(
        bounded.last().unwrap().content.as_text().as_ref(),
        history.last().unwrap().content.as_text().as_ref(),
        "the newest turn must survive the trim"
    );
    assert!(total_tokens(&bounded) <= budget);
}

#[test]
fn falls_back_to_protocol_previews_when_no_group_fits() {
    let history = vec![Message::user("x".repeat(40_000))];
    // Derive the limit from the measured fixture so this remains a
    // genuine no-group-fits case across tokenizer changes.
    let budget = total_tokens(&history) / 4;
    let bounded = bound_history_for_summarization(&history, INSTRUCTIONS, Some(budget));
    assert_eq!(bounded.len(), 1);
    assert!(total_tokens(&bounded) < total_tokens(&history), "an oversized single group must still be reduced");
    assert!(
        total_tokens(&bounded) <= budget,
        "fallback must respect budget {budget}, used {}",
        total_tokens(&bounded)
    );
}

#[test]
fn native_bound_preserves_latest_provider_compaction_marker() {
    let marker = Message::assistant(String::new()).with_reasoning_details(Some(vec![serde_json::json!({
        "type": "compaction",
        "content": null,
        "encrypted_content": "opaque-state",
    })]));
    let newest = Message::user("newest".repeat(64));
    let history = vec![marker.clone(), Message::user("old".repeat(4_000)), newest.clone()];
    let instruction_tokens = Message::user(INSTRUCTIONS.to_string()).estimate_tokens();
    let history_budget = marker.estimate_tokens() + newest.estimate_tokens() + 4;
    let bounded =
        bound_history_for_native_compaction(&history, INSTRUCTIONS, Some(instruction_tokens + history_budget));

    assert_eq!(bounded.first(), Some(&marker));
    assert_eq!(bounded.last(), Some(&newest));
    assert!(bounded.len() < history.len(), "expected old pre-compaction history to be dropped");
}
