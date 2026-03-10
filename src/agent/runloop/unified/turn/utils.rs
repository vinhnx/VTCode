use anyhow::Result;
use vtcode_core::config::constants::output_limits;
use vtcode_core::hooks::{HookMessage, HookMessageLevel};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(crate) fn render_hook_messages(
    renderer: &mut AnsiRenderer,
    messages: &[HookMessage],
) -> Result<()> {
    for message in messages {
        let text = message.text.trim();
        if text.is_empty() {
            continue;
        }

        let style = match message.level {
            HookMessageLevel::Info => MessageStyle::Info,
            HookMessageLevel::Warning => MessageStyle::Info,
            HookMessageLevel::Error => MessageStyle::Error,
        };

        renderer.line(style, text)?;
    }

    Ok(())
}

pub(crate) fn truncate_message_content(content: &str) -> String {
    let mut result =
        String::with_capacity(content.len().min(output_limits::MAX_AGENT_MESSAGES_SIZE));
    let mut truncated = false;

    for line in content.lines() {
        let mut line_bytes = 0;
        let mut end = 0;
        for (idx, ch) in line.char_indices() {
            let ch_len = ch.len_utf8();
            if line_bytes + ch_len > output_limits::MAX_LINE_LENGTH {
                truncated = true;
                break;
            }
            line_bytes += ch_len;
            end = idx + ch_len;
        }
        let trimmed_line = &line[..end];
        if result.len() + trimmed_line.len() + 1 > output_limits::MAX_AGENT_MESSAGES_SIZE {
            truncated = true;
            break;
        }
        result.push_str(trimmed_line);
        result.push('\n');
    }

    if truncated {
        result.push_str("[... content truncated due to size limit ...]");
    }

    result
}

pub(crate) fn enforce_history_limits(history: &mut Vec<uni::Message>) {
    let max_messages = output_limits::DEFAULT_MESSAGE_LIMIT.min(output_limits::MAX_MESSAGE_LIMIT);
    while history.len() > max_messages {
        if !remove_oldest_non_system(history) {
            break;
        }
    }

    loop {
        let total_bytes: usize = history.iter().map(|msg| msg.content.as_text().len()).sum();
        if total_bytes <= output_limits::MAX_ALL_MESSAGES_SIZE {
            break;
        }
        if !remove_oldest_non_system(history) {
            break;
        }
    }
}

fn remove_oldest_non_system(history: &mut Vec<uni::Message>) -> bool {
    if history.is_empty() {
        return false;
    }
    if history[0].role != uni::MessageRole::System {
        history.remove(0);
        return true;
    }
    if history.len() > 1 {
        history.remove(1);
        return true;
    }
    false
}
const UNLIMITED_TOOL_LOOP_BALANCER_WINDOW: usize = 20;

pub(crate) fn should_trigger_turn_balancer(
    step_count: usize,
    max_tool_loops: usize,
    repeated: usize,
    repeat_limit: usize,
) -> bool {
    let loop_window = if max_tool_loops == usize::MAX {
        UNLIMITED_TOOL_LOOP_BALANCER_WINDOW
    } else {
        max_tool_loops
    };
    let step_threshold = loop_window.saturating_mul(3) / 4;
    let effective_repeat_limit = repeat_limit.max(3);
    step_count > step_threshold && repeated >= effective_repeat_limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balancer_triggers_after_three_quarters_and_effective_repeat_limit() {
        assert!(should_trigger_turn_balancer(16, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(15, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(16, 20, 2, 3));
        assert!(!should_trigger_turn_balancer(16, 20, 2, 2));
    }

    #[test]
    fn balancer_uses_fallback_window_for_unlimited_loop_limit() {
        assert!(should_trigger_turn_balancer(16, usize::MAX, 3, 3));
        assert!(!should_trigger_turn_balancer(15, usize::MAX, 3, 3));
    }

    #[test]
    fn truncate_message_content_limits_lines_and_size() {
        let long_line = "a".repeat(output_limits::MAX_LINE_LENGTH + 16);
        let truncated = truncate_message_content(&long_line);

        assert!(truncated.contains("content truncated"));
        assert!(truncated.len() <= output_limits::MAX_AGENT_MESSAGES_SIZE);
    }

    #[test]
    fn enforce_history_limits_caps_message_count_and_keeps_system() {
        let mut history = Vec::new();
        history.push(uni::Message::system("system".to_string()));
        for idx in 0..(output_limits::DEFAULT_MESSAGE_LIMIT + 1) {
            history.push(uni::Message::assistant(format!("msg {}", idx)));
        }

        enforce_history_limits(&mut history);

        assert!(history.len() <= output_limits::DEFAULT_MESSAGE_LIMIT);
        assert_eq!(
            history.first().map(|m| m.role.clone()),
            Some(uni::MessageRole::System)
        );
    }
}
