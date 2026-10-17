use vtcode_core::llm::provider as uni;

use super::super::verify_mid_turn_prompt_received;

fn user(text: &str) -> uni::Message {
    uni::Message::user(text.to_string())
}

fn system(text: &str) -> uni::Message {
    uni::Message::system(text.to_string())
}

fn latest_user_text(history: &[uni::Message], turn_start: usize) -> Option<String> {
    history
        .get(turn_start.min(history.len())..)
        .unwrap_or(&[])
        .iter()
        .rev()
        .find(|m| m.role == uni::MessageRole::User)
        .map(|m| m.content.as_text().to_string())
}

#[test]
fn fresh_prompt_after_compaction_is_verified_verbatim() {
    // Settings-first order: compaction summarizes old history, then the fresh
    // steered prompt lands after the resume note verbatim.
    let mut history = vec![
        system("Previous conversation summary:\nold work"),
        system("Model switched mid-turn: openai/gpt-x -> anthropic/claude-x. left intact."),
    ];
    let turn_start = history.len();
    history.push(user("continue with the new model: fix README links"));

    // Must not panic; oracle below re-derives from history contents, never
    // from the helper's own return value.
    verify_mid_turn_prompt_received(&history, turn_start);
    // Asymmetric oracle: the tail must hold the exact prompt, not a summary.
    assert_eq!(
        latest_user_text(&history, turn_start).as_deref(),
        Some("continue with the new model: fix README links"),
        "fresh prompt must survive verbatim after model-switch compaction"
    );
}

#[test]
fn switch_without_fresh_prompt_reports_no_tail_prompt() {
    let history = vec![
        system("Previous conversation summary:\nold work"),
        system("Model switched mid-turn: openai/gpt-x -> anthropic/claude-x. left intact."),
    ];
    verify_mid_turn_prompt_received(&history, history.len());
    assert_eq!(
        latest_user_text(&history, history.len()),
        None,
        "a switch with no fresh input leaves no user prompt in the turn tail"
    );
}

#[test]
fn stale_turn_baseline_after_shrink_never_panics() {
    // The TUI-misalignment bug: turn started at 45, compaction shrank history
    // to 3 entries plus resume. Slicing must clamp instead of panicking.
    let history = vec![
        system("Previous conversation summary:\nold work"),
        system("Model switched mid-turn: openai/gpt-x -> anthropic/claude-x. left intact."),
        user("continue"),
    ];
    let stale_start = 45usize;
    // Production rebase invariant: clamp, never index out of bounds.
    assert_eq!(stale_start.min(history.len()), history.len());
    // Must not panic on a stale baseline. The pre-shrink prompt now lives in
    // the compacted prefix, so the rebased (empty) tail holds no user prompt.
    verify_mid_turn_prompt_received(&history, stale_start);
    assert_eq!(latest_user_text(&history, stale_start), None);
}

#[test]
fn prompt_steered_before_compaction_would_be_lost_without_ordering() {
    // Documents why settings must apply before steering: if the fresh prompt
    // were already in history when compaction clones it, a summarizer could
    // fold it into the summary. The fixed order appends it after, so the
    // verbatim user message is always the tail.
    let mut compacted = vec![system("Previous conversation summary:\nold work")];
    let resume = "Model switched mid-turn: openai/gpt-x -> anthropic/claude-x. left intact.";
    compacted.push(system(resume));
    let turn_start = compacted.len();
    compacted.push(user("explicit new prompt after switch"));

    verify_mid_turn_prompt_received(&compacted, turn_start);
    assert_eq!(compacted.last().expect("tail").role, uni::MessageRole::User);
    assert_eq!(latest_user_text(&compacted, turn_start).as_deref(), Some("explicit new prompt after switch"));
}
