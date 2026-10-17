//! History bounding ahead of summarization or native compaction.

use super::*;

/// Bound the summarizer's input to the resolved context budget.
///
/// The local-summary path forks the parent's entire conversation verbatim and
/// appends the compaction instruction, but nothing bounded that fork. On a
/// near-full context (the normal reason to run `/compact`) or after switching
/// to a model with a smaller window, the summary request itself exceeded the
/// summarizer's window and the provider rejected it, failing the whole
/// compaction. Local output paths are bounded by
/// [`bound_compacted_history_to_context`]; native provider output is treated as
/// canonical and must not be rewritten. Bound the input so local requests fit:
/// keep the newest complete protocol groups that fit and drop the oldest.
///
/// Returns the history to append the instruction to (see
/// [`build_cache_safe_compaction_history`]), not the finished fork./// Bound the summarizer's input to the resolved context budget.
///
/// The local-summary path forks the parent's entire conversation verbatim and
/// appends the compaction instruction, but nothing bounded that fork. On a
/// near-full context (the normal reason to run `/compact`) or after switching
/// to a model with a smaller window, the summary request itself exceeded the
/// summarizer's window and the provider rejected it, failing the whole
/// compaction. Local output paths are bounded by
/// [`bound_compacted_history_to_context`]; native provider output is treated as
/// canonical and must not be rewritten. Bound the input so local requests fit:
/// keep the newest complete protocol groups that fit and drop the oldest.
///
/// Returns the history to append the instruction to (see
pub(crate) fn bound_history_for_summarization(
    history: &[Message],
    instructions: &str,
    budget: Option<usize>,
) -> Vec<Message> {
    let Some(budget) = budget.filter(|value| *value > 0) else {
        return history.to_vec();
    };
    let instruction_tokens = Message::user(instructions.to_string()).estimate_tokens();
    let history_budget = budget.saturating_sub(instruction_tokens).max(4);
    bound_history_to_token_budget(history, history_budget)
}

/// Bound a native compaction request while retaining the latest provider
/// compaction marker. Provider-native windows use that opaque marker as the
/// continuity anchor; dropping it while selecting a recent suffix loses the
/// state needed to replay the window on the next request.
pub(crate) fn bound_history_for_native_compaction(
    history: &[Message],
    instructions: &str,
    budget: Option<usize>,
) -> Vec<Message> {
    let Some(budget) = budget.filter(|value| *value > 0) else {
        return history.to_vec();
    };
    let instruction_tokens = Message::user(instructions.to_string()).estimate_tokens();
    let history_budget = budget.saturating_sub(instruction_tokens).max(4);
    let total_tokens = history.iter().map(Message::estimate_tokens).sum::<usize>();
    if total_tokens <= history_budget {
        return history.to_vec();
    }

    let Some(marker_index) = history.iter().rposition(is_provider_compaction_message) else {
        return bound_history_to_token_budget(history, history_budget);
    };
    let marker = history[marker_index].clone();
    let marker_tokens = marker.estimate_tokens();
    let mut bounded = vec![marker];
    if marker_tokens < history_budget {
        bounded.extend(bound_history_to_token_budget(&history[marker_index + 1..], history_budget - marker_tokens));
    }
    bounded
}

fn bound_history_to_token_budget(history: &[Message], history_budget: usize) -> Vec<Message> {
    let total_tokens = history.iter().map(Message::estimate_tokens).sum::<usize>();
    if total_tokens <= history_budget {
        return history.to_vec();
    }

    let group_starts: Vec<usize> = history
        .iter()
        .enumerate()
        .filter_map(|(index, message)| (message.role == MessageRole::User).then_some(index))
        .collect();
    let last_group_index = group_starts.len().saturating_sub(1);

    let mut selected_start = history.len();
    let mut selected_end = history.len();
    let mut selected_tokens = 0usize;
    for (position, &start) in group_starts.iter().enumerate().rev() {
        let natural_end = group_starts.get(position + 1).copied().unwrap_or(history.len());
        // The trailing group may end on an unanswered tool call; drop that
        // invalid protocol suffix instead of shipping it to the provider.
        let end = if position == last_group_index {
            start.saturating_add(complete_protocol_group_prefix(&history[start..natural_end]))
        } else {
            natural_end
        };
        if start >= end {
            continue;
        }
        let group_tokens = history[start..end].iter().map(Message::estimate_tokens).sum::<usize>();
        if selected_tokens.saturating_add(group_tokens) > history_budget {
            break;
        }
        if selected_start == history.len() {
            selected_end = end;
        }
        selected_tokens += group_tokens;
        selected_start = start;
    }

    if selected_start >= selected_end {
        // No complete protocol group fits (for example a single oversized tool
        // result). Degrade to protocol-bounded previews so the request still
        // goes out instead of failing the entire compaction.
        return bounded_protocol_group(history, history_budget);
    }
    history[selected_start..selected_end].to_vec()
}

fn is_provider_compaction_message(message: &Message) -> bool {
    message.role == MessageRole::Assistant
        && message
            .reasoning_details
            .as_ref()
            .is_some_and(|details| details.iter().any(is_compaction_detail))
}
