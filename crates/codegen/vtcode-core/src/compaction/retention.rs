//! Retention scoring, tool-output pruning, and message retention.

use super::*;

/// Per-message cap applied to tool outputs in summarizer inputs. Large dumps
/// (file reads, command output) otherwise evict whole protocol groups from the
/// bounded fork. Call IDs and pairing metadata are preserved by
/// [`bounded_message_preview`], so trimmed groups stay protocol-valid./// Per-message cap applied to tool outputs in summarizer inputs. Large dumps
/// (file reads, command output) otherwise evict whole protocol groups from the
/// bounded fork. Call IDs and pairing metadata are preserved by
/// [`bounded_message_preview`], so trimmed groups stay protocol-valid.
pub(crate) const TOOL_RESULT_PRUNE_TARGET_TOKENS: usize = 4_096;

/// Trim oversized tool outputs to previews before bounding the summarizer
/// input. Only `Tool` messages are touched; everything else passes through
/// verbatim (cloned). Under the cap this is a pure copy.
pub(crate) fn prune_oversized_tool_outputs(history: &[Message]) -> Vec<Message> {
    history
        .iter()
        .map(|message| {
            if message.role == MessageRole::Tool {
                bounded_message_preview(message, TOOL_RESULT_PRUNE_TARGET_TOKENS)
            } else {
                message.clone()
            }
        })
        .collect()
}

pub(crate) fn bounded_message_preview(message: &Message, token_budget: usize) -> Message {
    if message.estimate_tokens() <= token_budget {
        return message.clone();
    }
    let mut preview = message.clone();
    let available = token_budget.saturating_sub(4);
    let text = truncate_to_token_limit(message.content.as_text().as_ref(), available);
    preview.content = MessageContent::Text(text);

    // Tool-call arguments and provider metadata are part of the message's
    // token footprint even when `content` is empty. Preserve call IDs and
    // function names so the protocol group remains correlatable, but replace
    // oversized argument payloads with valid minimal JSON and drop optional
    // reasoning/signature metadata.
    preview.reasoning = preview
        .reasoning
        .map(|reasoning| truncate_to_token_limit(&reasoning, available.min(1_024)));
    preview.reasoning_details = None;
    preview.metadata = None;
    preview.origin_tool = None;
    if let Some(tool_calls) = preview.tool_calls.as_mut() {
        for call in tool_calls {
            if let Some(function) = call.function.as_mut()
                && function.arguments.len() > available.saturating_mul(4)
            {
                function.arguments = "{}".to_string();
            }
            if let Some(text) = call.text.as_mut() {
                *text = truncate_to_token_limit(text, available.min(1_024));
            }
            call.thought_signature = None;
        }
    }

    if preview.estimate_tokens() > token_budget {
        preview.content = MessageContent::Text(String::new());
        preview.reasoning = None;
        preview.reasoning_details = None;
        if let Some(tool_calls) = preview.tool_calls.as_mut() {
            for call in tool_calls {
                if let Some(function) = call.function.as_mut() {
                    function.arguments = "{}".to_string();
                }
                call.text = None;
                call.thought_signature = None;
            }
        }
    }
    preview
}

pub(crate) fn collect_retained_user_messages(
    history: &[Message],
    token_budget: usize,
    max_messages: usize,
) -> Vec<Message> {
    if token_budget == 0 || max_messages == 0 {
        return Vec::new();
    }

    // Phase 1: select up to `max_messages` user messages, scored by importance.
    let total = history.len();
    let mut user_scored: Vec<(usize, f64, &Message)> = history
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == MessageRole::User && !m.content.trim().is_empty())
        .map(|(i, m)| {
            let score = score_message(m, i, total);
            (i, score, m)
        })
        .collect();
    user_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut selected: Vec<(usize, Message)> = Vec::with_capacity(max_messages.min(history.len()));
    let mut remaining = token_budget;

    for (original_idx, _score, message) in &user_scored {
        if selected.len() >= max_messages {
            break;
        }
        let estimated = message.estimate_tokens();
        if estimated <= remaining {
            selected.push((*original_idx, (*message).clone()));
            remaining = remaining.saturating_sub(estimated);
            continue;
        }
        if let Some(truncated) = truncate_user_message(message, remaining) {
            selected.push((*original_idx, truncated));
        }
        break;
    }

    // Phase 2: if budget remains, add high-value non-user messages (tool
    // results, assistant tool calls) that fit within the remaining capacity.
    if selected.len() < max_messages && remaining > 0 {
        let mut non_user_scored: Vec<(usize, f64, &Message)> = history
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role != MessageRole::User && m.role != MessageRole::System && is_retainable_message(m))
            .map(|(i, m)| {
                let score = score_message(m, i, total);
                (i, score, m)
            })
            .collect();
        non_user_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        for (original_idx, _score, message) in &non_user_scored {
            if selected.len() >= max_messages {
                break;
            }
            let estimated = message.estimate_tokens();
            if estimated <= remaining {
                selected.push((*original_idx, (*message).clone()));
                remaining = remaining.saturating_sub(estimated);
            }
        }
    }

    // Re-sort by original conversation order, then enforce tool-call/turn
    // coherence so the compacted history is valid to send back to a provider.
    selected.sort_by_key(|(idx, _)| *idx);
    coherence_tool_call_pairs(history, &selected)
        .into_iter()
        .map(|(_, msg)| msg)
        .collect()
}

/// Keep retained tool-call turns internally consistent.
///
/// A `Tool` message references a tool call the model must have seen, and an
/// `Assistant` message that still carries `tool_calls` must be followed by the
/// results those calls produced. Sending either without its counterpart is
/// invalid: providers reject unmatched tool calls, and orphaned tool results
/// reference a call the model never observed. This pass:
///
/// - **Force-keeps** the `Tool` messages immediately following any retained
///   `Assistant` that carries `tool_calls`, so the model observes each call's
///   return value. A complete turn ends with its results, which survive even
///   if they push past the soft `max_messages` cap.
/// - **Drops** a retained `Tool` message whose calling `Assistant` (the message
///   directly before it in `history`) was *not* retained — an orphaned result
///   the model cannot reconcile.
///
/// Tool results that follow a plain `Assistant` (no `tool_calls`) are ordinary
/// turn output and are kept exactly as selected.
pub(crate) fn coherence_tool_call_pairs(history: &[Message], selected: &[(usize, Message)]) -> Vec<(usize, Message)> {
    let mut keep: std::collections::HashSet<usize> = selected.iter().map(|(i, _)| *i).collect();

    for (idx, msg) in selected {
        if msg.role == MessageRole::Assistant && msg.tool_calls.as_ref().is_some_and(|calls| !calls.is_empty()) {
            let mut j = *idx + 1;
            while let Some(next) = history.get(j) {
                if next.role == MessageRole::Tool {
                    keep.insert(j);
                    j += 1;
                } else {
                    break;
                }
            }
        }
    }

    // Emit every index in `keep` in original history order, dropping orphaned
    // Tool results whose calling Assistant turn was not retained.
    let mut indices: Vec<usize> = keep.iter().copied().collect();
    indices.sort_unstable();

    indices
        .into_iter()
        .filter(|idx| {
            let msg = &history[*idx];
            if msg.role != MessageRole::Tool {
                return true;
            }
            // Walk backward through this contiguous result run to decide
            // coherence against the calling assistant turn.
            let mut cursor = *idx;
            loop {
                match history.get(cursor) {
                    Some(m) if m.role == MessageRole::Tool => {
                        cursor = match cursor.checked_sub(1) {
                            Some(c) => c,
                            None => break,
                        };
                    }
                    Some(m)
                        if m.role == MessageRole::Assistant && m.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) =>
                    {
                        // Reached the calling assistant: coherent only if it
                        // was retained.
                        return keep.contains(&cursor);
                    }
                    _ => {
                        // Plain assistant or boundary: ordinary output.
                        return true;
                    }
                }
            }
            true
        })
        .map(|idx| (idx, history[idx].clone()))
        .collect()
}

/// Score a message for importance-weighted retention during compaction.
///
/// Uses a weighted combination of content importance and recency:
/// - Messages containing errors, corrections, or tool results score higher
/// - Recent messages get a recency bonus
/// - Assistant messages with tool calls are moderately important
fn score_message(message: &Message, index: usize, total: usize) -> f64 {
    let content = message.content.as_text();
    let content_lower = content.to_lowercase();

    // Importance weight based on content signals.
    let importance = match message.role {
        MessageRole::User => {
            if contains_error_signal(&content_lower) {
                3.0
            } else if contains_correction_signal(&content_lower) {
                2.5
            } else {
                1.0
            }
        }
        MessageRole::Tool => {
            // Tool results contain factual data the model may need.
            2.0
        }
        MessageRole::Assistant => {
            if message.tool_calls.is_some() {
                // Assistant messages with tool calls show action taken.
                0.5
            } else {
                0.1
            }
        }
        MessageRole::System => 0.0,
    };

    // Recency bonus: linear from 0.0 (oldest) to 1.0 (newest).
    let recency = if total > 0 { index as f64 / total as f64 } else { 0.0 };

    importance + recency
}

/// Check if content contains error or failure signals.
fn contains_error_signal(content: &str) -> bool {
    content.contains("error")
        || content.contains("failed")
        || content.contains("failure")
        || content.contains("panic")
        || content.contains("bug")
        || content.contains("broken")
        || content.contains("regression")
}

/// Check if content contains user correction signals.
fn contains_correction_signal(content: &str) -> bool {
    content.contains("no,")
        || content.contains("wrong")
        || content.contains("actually")
        || content.contains("fix")
        || content.contains("instead")
        || content.contains("should be")
        || content.contains("don't")
}

/// Whether a message is worth retaining during compaction.
fn is_retainable_message(message: &Message) -> bool {
    match message.role {
        MessageRole::User => !message.content.trim().is_empty(),
        MessageRole::Tool => !message.content.trim().is_empty(),
        MessageRole::Assistant => {
            // Retain assistant messages that contain tool calls (action history).
            message.tool_calls.is_some()
        }
        MessageRole::System => false,
    }
}

fn truncate_user_message(message: &Message, token_budget: usize) -> Option<Message> {
    if token_budget <= 4 {
        return None;
    }

    let available_content_tokens = token_budget.saturating_sub(4);
    let truncated = truncate_to_token_limit(message.content.as_text().as_ref(), available_content_tokens);
    let trimmed = truncated.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(Message::user(trimmed.to_string()))
}
