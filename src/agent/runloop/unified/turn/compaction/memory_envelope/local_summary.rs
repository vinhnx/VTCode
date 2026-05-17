use super::*;

pub(crate) fn configured_retained_user_messages(vt_cfg: Option<&VTCodeConfig>) -> usize {
    vt_cfg
        .map(|cfg| cfg.context.dynamic.retained_user_messages)
        .unwrap_or(4)
}

pub(crate) fn local_compaction_config(
    vt_cfg: Option<&VTCodeConfig>,
    always_summarize: bool,
) -> CompactionConfig {
    CompactionConfig {
        always_summarize,
        retained_user_messages: configured_retained_user_messages(vt_cfg),
        ..CompactionConfig::default()
    }
}

fn collect_zero_cost_retained_user_messages(
    history: &[Message],
    token_budget: usize,
    max_messages: usize,
) -> Vec<Message> {
    if token_budget == 0 || max_messages == 0 {
        return Vec::new();
    }

    let mut kept = Vec::new();
    let mut remaining = token_budget;

    for message in history.iter().rev() {
        if kept.len() >= max_messages {
            break;
        }
        if message.role != MessageRole::User || message.content.trim().is_empty() {
            continue;
        }

        let estimated = message.estimate_tokens();
        if estimated <= remaining {
            kept.push(message.clone());
            remaining = remaining.saturating_sub(estimated);
            continue;
        }

        if remaining > 4 {
            let truncated = truncate_to_token_limit(
                message.content.as_text().as_ref(),
                remaining.saturating_sub(4),
            );
            let trimmed = truncated.trim();
            if !trimmed.is_empty() {
                kept.push(Message::user(trimmed.to_string()));
            }
        }
        break;
    }

    kept.reverse();
    kept
}

pub(crate) fn build_zero_cost_summarized_fork_history(
    source_history: &[Message],
    source_envelope: Option<&SessionMemoryEnvelope>,
    retained_user_messages: usize,
) -> Vec<Message> {
    let summary = source_envelope
        .map(|envelope| normalize_whitespace(&envelope.summary))
        .filter(|summary| !summary.is_empty())
        .unwrap_or_else(|| derive_continuity_summary(source_history, source_envelope));

    let retained_users = collect_zero_cost_retained_user_messages(
        source_history,
        CompactionConfig::default().retained_user_message_tokens,
        retained_user_messages,
    );

    let mut compacted = Vec::with_capacity(retained_users.len().saturating_add(1));
    compacted.push(Message::system(format!(
        "Previous conversation summary:\n{}",
        summary.trim()
    )));
    compacted.extend(retained_users);
    compacted
}
