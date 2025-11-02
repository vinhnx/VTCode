use vtcode_core::config::constants::context as context_defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;

#[derive(Clone, Copy)]
pub(crate) struct ContextTrimConfig {
    pub(crate) max_tokens: usize,
    pub(crate) trim_to_percent: u8,
    pub(crate) preserve_recent_turns: usize,
}

impl ContextTrimConfig {
    pub(crate) fn target_tokens(&self) -> usize {
        let percent = (self.trim_to_percent as u128).clamp(
            context_defaults::MIN_TRIM_RATIO_PERCENT as u128,
            context_defaults::MAX_TRIM_RATIO_PERCENT as u128,
        );
        ((self.max_tokens as u128) * percent / 100) as usize
    }
}

#[derive(Default)]
pub(crate) struct ContextTrimOutcome {
    pub(crate) removed_messages: usize,
}

impl ContextTrimOutcome {
    pub(crate) fn is_trimmed(&self) -> bool {
        self.removed_messages > 0
    }
}

pub(crate) fn prune_unified_tool_responses(
    history: &mut Vec<uni::Message>,
    preserve_recent_turns: usize,
) -> usize {
    if history.is_empty() {
        return 0;
    }

    let keep_from = history.len().saturating_sub(preserve_recent_turns);
    if keep_from == 0 {
        return 0;
    }

    let mut removed = 0usize;
    let mut index = 0usize;
    history.retain(|message| {
        let contains_tool_payload = message.is_tool_response() || message.has_tool_calls();
        let keep = index >= keep_from || !contains_tool_payload;
        if !keep {
            removed += 1;
        }
        index += 1;
        keep
    });
    removed
}

pub(crate) fn apply_aggressive_trim_unified(
    history: &mut Vec<uni::Message>,
    config: ContextTrimConfig,
) -> usize {
    if history.is_empty() {
        return 0;
    }

    let keep_turns = config
        .preserve_recent_turns
        .clamp(
            context_defaults::MIN_PRESERVE_RECENT_TURNS,
            context_defaults::AGGRESSIVE_PRESERVE_RECENT_TURNS,
        )
        .min(history.len());

    let remove = history.len().saturating_sub(keep_turns);
    if remove == 0 {
        return 0;
    }

    history.drain(0..remove);
    remove
}

pub(crate) fn enforce_unified_context_window(
    history: &mut Vec<uni::Message>,
    config: ContextTrimConfig,
) -> ContextTrimOutcome {
    if history.is_empty() {
        return ContextTrimOutcome::default();
    }

    let tokens_per_message: Vec<usize> = history
        .iter()
        .map(approximate_unified_message_tokens)
        .collect();
    let mut total_tokens: usize = tokens_per_message.iter().sum();

    if total_tokens <= config.max_tokens {
        return ContextTrimOutcome::default();
    }

    let target_tokens = config.target_tokens();
    let mut remove_count = 0usize;
    let mut preserve_boundary = history.len().saturating_sub(config.preserve_recent_turns);
    if preserve_boundary > history.len().saturating_sub(1) {
        preserve_boundary = history.len().saturating_sub(1);
    }

    while remove_count < preserve_boundary && total_tokens > config.max_tokens {
        total_tokens = total_tokens.saturating_sub(tokens_per_message[remove_count]);
        remove_count += 1;
        if total_tokens <= target_tokens {
            break;
        }
    }

    while remove_count < history.len().saturating_sub(1) && total_tokens > config.max_tokens {
        total_tokens = total_tokens.saturating_sub(tokens_per_message[remove_count]);
        remove_count += 1;
    }

    if remove_count == 0 {
        return ContextTrimOutcome::default();
    }

    history.drain(0..remove_count);
    ContextTrimOutcome {
        removed_messages: remove_count,
    }
}

pub(crate) fn load_context_trim_config(vt_cfg: Option<&VTCodeConfig>) -> ContextTrimConfig {
    let context_cfg = vt_cfg.map(|cfg| &cfg.context);
    let max_tokens = std::env::var("VTCODE_CONTEXT_TOKEN_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .or_else(|| {
            context_cfg
                .map(|cfg| cfg.max_context_tokens)
                .filter(|value| *value > 0)
        })
        .unwrap_or(context_defaults::DEFAULT_MAX_TOKENS);

    let trim_to_percent = context_cfg
        .map(|cfg| cfg.trim_to_percent)
        .unwrap_or(context_defaults::DEFAULT_TRIM_TO_PERCENT)
        .clamp(
            context_defaults::MIN_TRIM_RATIO_PERCENT,
            context_defaults::MAX_TRIM_RATIO_PERCENT,
        );

    let preserve_recent_turns = context_cfg
        .map(|cfg| cfg.preserve_recent_turns)
        .unwrap_or(context_defaults::DEFAULT_PRESERVE_RECENT_TURNS)
        .max(context_defaults::MIN_PRESERVE_RECENT_TURNS);

    ContextTrimConfig {
        max_tokens,
        trim_to_percent,
        preserve_recent_turns,
    }
}

fn approximate_unified_message_tokens(message: &uni::Message) -> usize {
    let mut total_chars = message.content.as_text().len();
    total_chars += message.role.as_generic_str().len();

    if let Some(tool_calls) = &message.tool_calls {
        for call in tool_calls {
            total_chars += call.id.len();
            total_chars += call.call_type.len();
            total_chars += call.function.name.len();
            total_chars += call.function.arguments.len();
        }
    }

    if let Some(tool_call_id) = &message.tool_call_id {
        total_chars += tool_call_id.len();
    }

    total_chars.div_ceil(context_defaults::CHAR_PER_TOKEN_APPROX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enforce_unified_context_window_trims_and_preserves_latest() {
        let mut history: Vec<uni::Message> = (0..12)
            .map(|i| uni::Message::assistant(format!("assistant step {}", i)))
            .collect();
        let original_len = history.len();
        let config = ContextTrimConfig {
            max_tokens: 18,
            trim_to_percent: 70,
            preserve_recent_turns: 3,
        };

        let outcome = enforce_unified_context_window(&mut history, config);

        assert!(outcome.is_trimmed());
        assert_eq!(original_len - history.len(), outcome.removed_messages);

        let remaining_tokens: usize = history.iter().map(approximate_unified_message_tokens).sum();
        assert!(remaining_tokens <= config.max_tokens);

        let last_content = history
            .last()
            .map(|msg| msg.content.as_text())
            .unwrap_or_default();
        assert!(last_content.contains("assistant step 11"));
    }

    #[test]
    fn test_prune_unified_tool_responses_respects_recent_history() {
        let mut history: Vec<uni::Message> = vec![
            uni::Message::user("keep".to_string()),
            uni::Message::tool_response("call_1".to_string(), "{\"result\":1}".to_string()),
            uni::Message::assistant("assistant0".to_string()),
            uni::Message::user("keep2".to_string()),
            {
                let mut msg = uni::Message::assistant("assistant_with_tool".to_string());
                msg.tool_calls = Some(vec![uni::ToolCall::function(
                    "call_2".to_string(),
                    "tool_b".to_string(),
                    "{}".to_string(),
                )]);
                msg
            },
            uni::Message::tool_response("call_2".to_string(), "{\"result\":2}".to_string()),
        ];

        let removed = prune_unified_tool_responses(&mut history, 4);

        assert_eq!(removed, 1);
        assert!(history.len() >= 4);
        assert_eq!(
            history.first().unwrap().content.as_text(),
            "keep".to_string()
        );
        assert!(history.iter().any(|msg| msg.is_tool_response()));
    }

    #[test]
    fn test_apply_aggressive_trim_unified_limits_history() {
        let mut history: Vec<uni::Message> = (0..15)
            .map(|i| uni::Message::assistant(format!("assistant step {i}")))
            .collect();
        let config = ContextTrimConfig {
            max_tokens: 140,
            trim_to_percent: 80,
            preserve_recent_turns: 10,
        };

        let removed = apply_aggressive_trim_unified(&mut history, config);

        let expected_len = context_defaults::AGGRESSIVE_PRESERVE_RECENT_TURNS;
        assert_eq!(removed, 15 - expected_len);
        assert_eq!(history.len(), expected_len);
        let expected_first = format!(
            "assistant step {}",
            15 - context_defaults::AGGRESSIVE_PRESERVE_RECENT_TURNS
        );
        assert!(
            history
                .first()
                .map(|msg| msg.content.as_text())
                .unwrap_or_default()
                .contains(&expected_first)
        );
    }
}
