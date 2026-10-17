//! Continuity-tail selection and context-budget bounding.

use super::*;

pub(crate) fn build_summary_prompt(history: &[Message], instructions: &str) -> String {
    // Pre-size for the header plus every (non-empty) message body, avoiding
    // repeated reallocations while the summary prompt is assembled.
    let estimated_len =
        instructions.len() + history.iter().map(|m| m.content.as_text().len()).sum::<usize>() + history.len() * 16;
    let mut formatted = String::with_capacity(estimated_len);
    let now: DateTime<Utc> = Utc::now();
    let _ = writeln!(&mut formatted, "Summary requested at {}.\n{}", now.to_rfc3339(), instructions);

    for message in history {
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        let content = message.content.as_text();
        if content.trim().is_empty() {
            continue;
        }
        let _ = writeln!(&mut formatted, "\n[{}]\n{}", role, content.trim());
    }

    formatted
}

pub(crate) fn compaction_history_budget(
    provider: &dyn LLMProvider,
    model: &str,
    context_budget: Option<usize>,
) -> Option<usize> {
    let context_size = context_budget
        .filter(|value| *value > 0)
        .unwrap_or_else(|| provider.effective_context_size(model));
    (context_size > 0).then(|| {
        context_size
            .saturating_sub(context_size / COMPACTION_CONTEXT_OVERHEAD_FRACTION_DENOMINATOR)
            .saturating_sub(COMPACTION_CONTEXT_FIXED_OVERHEAD_TOKENS)
    })
}

pub(crate) fn context_bounded_compaction_config(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
    context_budget: Option<usize>,
) -> CompactionConfig {
    let mut bounded = config.clone();
    if let Some(history_budget) = compaction_history_budget(provider, model, context_budget) {
        let tail_target = route_tail_target_tokens(provider, model, context_budget);
        let continuity_tokens = continuity_tail_with_target(history, tail_target)
            .iter()
            .map(Message::estimate_tokens)
            .sum::<usize>();
        bounded.retained_user_message_tokens = bounded
            .retained_user_message_tokens
            .min(history_budget.saturating_sub(continuity_tokens));
    }
    bounded
}

/// Bound a locally managed compacted history to the resolved context budget,
/// including any caller-supplied session ceiling. Standalone native responses
/// must bypass this helper: their retained items and opaque continuation state
/// are the provider's canonical next context window.
pub fn bound_compacted_history_to_context(
    compacted: Vec<Message>,
    provider: &dyn LLMProvider,
    model: &str,
    context_budget: Option<usize>,
) -> Vec<Message> {
    let Some(history_budget) = compaction_history_budget(provider, model, context_budget) else {
        return compacted;
    };
    if compacted.iter().map(Message::estimate_tokens).sum::<usize>() <= history_budget {
        return compacted;
    }

    let Some((tail_start, tail_end, _)) =
        continuity_tail_selection_with_target(&compacted, route_tail_target_tokens(provider, model, context_budget))
    else {
        return compacted
            .into_iter()
            .scan(history_budget, |remaining, message| {
                if *remaining < 4 {
                    return None;
                }
                let bounded = bounded_message_preview(&message, *remaining);
                let used = bounded.estimate_tokens();
                if used > *remaining {
                    return None;
                }
                *remaining -= used;
                Some(bounded)
            })
            .collect();
    };

    let raw_tail = &compacted[tail_start..tail_end];
    let raw_tail_tokens = raw_tail.iter().map(Message::estimate_tokens).sum::<usize>();
    let tail = if raw_tail_tokens > history_budget {
        bounded_protocol_group(raw_tail, history_budget.max(4))
    } else {
        raw_tail.to_vec()
    };
    let tail_tokens = tail.iter().map(Message::estimate_tokens).sum::<usize>();
    let mut remaining = history_budget.saturating_sub(tail_tokens);
    let mut bounded_prefix = Vec::new();

    for (index, message) in compacted[..tail_start].iter().enumerate() {
        if remaining < 4 {
            break;
        }
        // Keep the leading summary/envelope message readable, but cap it so
        // the newest complete protocol groups retain priority.
        let message_budget = if index == 0 { remaining.min(4_096) } else { remaining };
        let bounded = bounded_message_preview(message, message_budget);
        let used = bounded.estimate_tokens();
        if used > remaining {
            if index == 0 {
                let fallback = Message::system(truncate_to_token_limit(
                    message.content.as_text().as_ref(),
                    remaining.saturating_sub(4),
                ));
                let fallback_tokens = fallback.estimate_tokens();
                if fallback_tokens <= remaining {
                    bounded_prefix.push(fallback);
                }
            }
            break;
        }
        remaining -= used;
        bounded_prefix.push(bounded);
    }

    bounded_prefix.extend(tail);
    bounded_prefix
}

pub(crate) fn build_local_compacted_history(
    history: &[Message],
    summary: &str,
    retained_user_message_tokens: usize,
    retained_user_messages: usize,
    include_continuity_tail: bool,
    tail_target_tokens: usize,
) -> Vec<Message> {
    let (retention_history, continuity) = if include_continuity_tail {
        split_continuity_history_with_target(history, tail_target_tokens)
    } else {
        (history, Vec::new())
    };
    let retained_users =
        collect_retained_user_messages(retention_history, retained_user_message_tokens, retained_user_messages);
    let mut new_history = Vec::with_capacity(retained_users.len().saturating_add(1));
    new_history.push(Message::system(format!("{SUMMARY_PREFIX}{}", summary.trim())));
    new_history.extend(retained_users);

    // Continuity anchor: retain the newest complete protocol groups verbatim
    // within the route tail budget. Duplicate message text is still valid
    // across turns, so preserve the sequence by index rather than deduplicating
    // on role/content.
    if include_continuity_tail {
        for message in continuity {
            new_history.push(message);
        }
    }
    new_history
}

/// Return the newest complete user-anchored protocol groups that fit the fixed
/// continuity budget. The returned messages are owned because an oversized
/// individual group may need a bounded preview.
#[cfg(test)]
pub(crate) fn continuity_tail(history: &[Message]) -> Vec<Message> {
    continuity_tail_with_target(history, CONTINUITY_TAIL_TARGET_TOKENS)
}

/// [`continuity_tail`] with a route-scaled budget so small windows keep a
/// usable summary instead of a tail-only history.
pub(crate) fn continuity_tail_with_target(history: &[Message], tail_target_tokens: usize) -> Vec<Message> {
    let Some((start, end, oversized)) = continuity_tail_selection_with_target(history, tail_target_tokens) else {
        return Vec::new();
    };
    if oversized {
        bounded_protocol_group(&history[start..end], tail_target_tokens.max(4))
    } else {
        history[start..end].to_vec()
    }
}

/// Find one contiguous suffix of complete protocol groups. An incomplete
/// trailing assistant tool-call group is truncated at the assistant message,
/// preserving its user anchor while excluding the invalid protocol suffix.
fn continuity_tail_selection_with_target(
    history: &[Message],
    tail_target_tokens: usize,
) -> Option<(usize, usize, bool)> {
    if history.is_empty() {
        return None;
    }
    let group_starts: Vec<usize> = history
        .iter()
        .enumerate()
        .filter_map(|(index, message)| (message.role == MessageRole::User).then_some(index))
        .collect();
    let last_group_index = group_starts.len().saturating_sub(1);
    let last_group_start = *group_starts.last()?;
    let last_group_end = history.len();
    let last_group = &history[last_group_start..last_group_end];
    let last_group_prefix_end = complete_protocol_group_prefix(last_group);
    let tail_end = last_group_start.saturating_add(last_group_prefix_end);

    let mut selected_start = None;
    let mut estimated_tokens = 0usize;
    for group_index in (0..group_starts.len()).rev() {
        let start = group_starts[group_index];
        let natural_end = group_starts.get(group_index + 1).copied().unwrap_or(history.len());
        let end = if group_index == last_group_index {
            tail_end
        } else {
            natural_end
        };
        if start == end {
            continue;
        }
        let group = &history[start..end];
        if group_index != last_group_index && !protocol_group_is_complete(group) {
            break;
        }
        let group_tokens = group.iter().map(Message::estimate_tokens).sum::<usize>();
        if selected_start.is_none() && group_tokens > tail_target_tokens {
            return Some((start, end, true));
        }
        if estimated_tokens.saturating_add(group_tokens) > tail_target_tokens {
            break;
        }
        selected_start = Some(start);
        estimated_tokens += group_tokens;
    }

    selected_start.map(|start| (start, tail_end, false))
}

fn protocol_group_is_complete(group: &[Message]) -> bool {
    complete_protocol_group_prefix(group) == group.len()
}

/// Return the length of the valid protocol prefix in a user-anchored group.
/// When a tool call is still pending, the prefix ends before the assistant
/// message that introduced it. This handles parallel tool calls and prevents
/// a partially answered group from entering the continuity tail.
pub(crate) fn complete_protocol_group_prefix(group: &[Message]) -> usize {
    if group.first().is_none_or(|message| message.role != MessageRole::User) {
        return 0;
    }

    let mut pending_tool_call_ids: Vec<&str> = Vec::new();
    let mut pending_origin = None;

    for (index, message) in group.iter().enumerate() {
        if !pending_tool_call_ids.is_empty() {
            if message.role != MessageRole::Tool {
                return pending_origin.unwrap_or(index);
            }
            let Some(tool_call_id) = message.tool_call_id.as_deref() else {
                return pending_origin.unwrap_or(index);
            };
            let Some(pending_index) = pending_tool_call_ids.iter().position(|pending_id| *pending_id == tool_call_id)
            else {
                return pending_origin.unwrap_or(index);
            };
            pending_tool_call_ids.swap_remove(pending_index);
            if pending_tool_call_ids.is_empty() {
                pending_origin = None;
            }
            continue;
        }

        // Older persisted histories may contain a tool result without the
        // assistant call metadata that introduced it. Preserve that legacy
        // pair rather than discarding an otherwise complete protocol group.
        if message.role == MessageRole::Tool {
            continue;
        }

        if message.role == MessageRole::Assistant
            && let Some(tool_calls) = message.tool_calls.as_ref()
            && !tool_calls.is_empty()
        {
            for (call_index, call) in tool_calls.iter().enumerate() {
                if call.id.is_empty() || tool_calls[..call_index].iter().any(|prior| prior.id == call.id) {
                    return index;
                }
            }
            pending_origin = Some(index);
            pending_tool_call_ids.extend(tool_calls.iter().map(|call| call.id.as_str()));
        }
    }

    pending_origin.unwrap_or(group.len())
}

pub(crate) fn split_continuity_history_with_target(
    history: &[Message],
    tail_target_tokens: usize,
) -> (&[Message], Vec<Message>) {
    let Some((tail_start, _, _)) = continuity_tail_selection_with_target(history, tail_target_tokens) else {
        return (history, Vec::new());
    };
    (&history[..tail_start], continuity_tail_with_target(history, tail_target_tokens))
}
