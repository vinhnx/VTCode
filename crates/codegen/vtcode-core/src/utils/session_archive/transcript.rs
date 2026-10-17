use std::collections::HashMap;

use super::message::SessionMessage;
use crate::llm::provider::MessageRole;
use crate::utils::session_transcript_norm::{
    format_repeated_summary, normalize_recovery_line, normalized_transcript_key, push_clean_transcript_line,
    should_drop_transcript_line, summarize_tool_block,
};

pub(crate) fn progress_transcript_from_recent_messages(recent_messages: &[SessionMessage]) -> Vec<String> {
    let mut transcript = Vec::with_capacity(recent_messages.len());

    for message in recent_messages {
        if !matches!(message.role, MessageRole::User | MessageRole::Assistant) {
            continue;
        }

        let content = message.content.trim();
        let content: &str = content.as_ref();
        if !content.is_empty() && transcript.last().is_none_or(|last: &String| last.as_str() != content) {
            transcript.push(content.to_string());
        }
    }

    clean_transcript_lines(&transcript)
}

pub(crate) fn clean_transcript_lines(lines: &[String]) -> Vec<String> {
    let mut cleaned = Vec::new();
    let mut seen_tool_blocks: HashMap<String, (usize, usize, String)> = HashMap::new();
    let mut index = 0usize;

    while index < lines.len() {
        let line = lines[index].trim_end();

        if should_reset_tool_dedupe_scope(line) {
            seen_tool_blocks.clear();
        }

        if let Some(replacement) = normalize_recovery_line(line) {
            push_clean_transcript_line(&mut cleaned, replacement);
            index += 1;
            continue;
        }

        if should_drop_transcript_line(line) {
            index += 1;
            continue;
        }

        if line.trim_start().starts_with("• ") {
            let (summary, next_index) = summarize_tool_block(lines, index);
            let signature = normalized_transcript_key(&summary);

            if let Some((first_index, repeats, original_line)) = seen_tool_blocks.get_mut(&signature) {
                *repeats += 1;
                if let Some(existing) = cleaned.get_mut(*first_index) {
                    *existing = format_repeated_summary(original_line, *repeats);
                }
            } else {
                let insertion_index = cleaned.len();
                push_clean_transcript_line(&mut cleaned, summary);
                if cleaned.len() > insertion_index {
                    seen_tool_blocks.insert(signature, (insertion_index, 1, cleaned[insertion_index].clone()));
                }
            }
            index = next_index;
            continue;
        }

        push_clean_transcript_line(&mut cleaned, line.to_string());
        index += 1;
    }

    while cleaned.last().is_some_and(|line: &String| line.is_empty()) {
        cleaned.pop();
    }

    cleaned
}

fn should_reset_tool_dedupe_scope(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !line.starts_with(' ') && !trimmed.starts_with("• ") && !trimmed.starts_with("[!]")
}
