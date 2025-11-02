use std::collections::HashSet;
use vtcode_core::core::conversation_summarizer::ConversationTurn;

/// Configuration for conversation compression
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CompressionConfig {
    /// Maximum number of turns to keep in the conversation
    pub max_turns: usize,
    /// Maximum tokens per message
    pub max_tokens_per_message: usize,
    /// Minimum message length to keep (in chars)
    pub min_message_length: usize,
    /// Whether to merge consecutive messages from the same role
    pub merge_consecutive: bool,
    /// Whether to remove redundant system messages
    pub remove_redundant_system_messages: bool,
    /// Whether to truncate long messages
    pub truncate_long_messages: bool,
    /// Maximum number of messages to keep from the beginning
    pub keep_first_messages: usize,
    /// Maximum number of messages to keep from the end
    pub keep_last_messages: usize,
    /// System messages to always keep (even if short)
    pub keep_system_messages: HashSet<&'static str>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        let mut keep_system_messages = HashSet::new();
        keep_system_messages.insert("system_important");
        keep_system_messages.insert("system_error");
        keep_system_messages.insert("system_warning");

        Self {
            max_turns: 50,
            max_tokens_per_message: 1000,
            min_message_length: 10,
            merge_consecutive: true,
            remove_redundant_system_messages: true,
            truncate_long_messages: true,
            keep_first_messages: 5,
            keep_last_messages: 5,
            keep_system_messages,
        }
    }
}

/// Compresses conversation turns using rule-based methods
#[allow(dead_code)]
pub fn compress_conversation(turns: &[ConversationTurn]) -> Vec<ConversationTurn> {
    let config = CompressionConfig::default();
    compress_conversation_with_config(turns, &config)
}

/// Compresses conversation turns with custom configuration
#[allow(dead_code)]
pub fn compress_conversation_with_config(
    turns: &[ConversationTurn],
    config: &CompressionConfig,
) -> Vec<ConversationTurn> {
    // Short-circuit only if no compression is needed
    if turns.is_empty() {
        return Vec::new();
    }

    let mut compressed = Vec::with_capacity(config.max_turns);
    let mut last_role: Option<String> = None;
    let mut buffer = String::new();
    let mut buffer_turn_number = 0;

    // Helper to flush the buffer
    let flush_buffer = |compressed: &mut Vec<ConversationTurn>,
                        role: &str,
                        content: String,
                        turn_number: usize| {
        if !content.is_empty() {
            compressed.push(ConversationTurn {
                role: role.to_string(),
                content,
                turn_number,
                task_info: None, // Preserve task_info if needed
            });
        }
    };

    // Process each turn with the configured rules
    for (_i, turn) in turns.iter().enumerate() {
        // Skip empty or very short messages unless they're important
        let trimmed_len = turn.content.trim().len();
        if trimmed_len == 0 {
            continue;
        }
        let is_system_like = turn.role.starts_with("system");
        if trimmed_len < config.min_message_length
            && !config.keep_system_messages.contains(turn.role.as_str())
            && is_system_like
        {
            continue;
        }

        // Truncate long messages if enabled
        let content = if config.truncate_long_messages {
            truncate_message(&turn.content, config.max_tokens_per_message)
        } else {
            turn.content.clone()
        };

        // If we're merging consecutive messages from the same role
        if config.merge_consecutive && last_role.as_ref() == Some(&turn.role) {
            if !buffer.is_empty() {
                buffer.push_str("\n\n");
            }
            buffer.push_str(&content);
        } else {
            // Flush the previous buffer if any
            if let Some(role) = last_role.take() {
                flush_buffer(
                    &mut compressed,
                    &role,
                    std::mem::take(&mut buffer),
                    buffer_turn_number,
                );
            }

            // Start a new buffer
            buffer = content;
            buffer_turn_number = turn.turn_number;
            last_role = Some(turn.role.clone());
        }
    }

    // Flush any remaining content
    if let Some(role) = last_role {
        flush_buffer(&mut compressed, &role, buffer, buffer_turn_number);
    }

    // Ensure we don't exceed max_turns
    if compressed.len() > config.max_turns {
        // First pass: separate important and non-important messages
        let mut important = Vec::new();
        let mut non_important = Vec::new();

        for turn in compressed.iter() {
            if is_important_message(turn, config) {
                important.push(turn.clone());
            } else {
                non_important.push(turn.clone());
            }
        }

        // Build result: keep important messages, then add most recent non-important ones up to max_turns
        compressed = important;
        let remaining_capacity = config.max_turns.saturating_sub(compressed.len());
        if remaining_capacity > 0 && !non_important.is_empty() {
            // Take from the end to keep recent messages
            let skip = non_important.len().saturating_sub(remaining_capacity);
            compressed.extend(non_important.into_iter().skip(skip));
        }
    }

    compressed
}

/// Checks if a message should be considered important
#[allow(dead_code)]
fn is_important_message(turn: &ConversationTurn, config: &CompressionConfig) -> bool {
    // System messages marked as important
    if config.keep_system_messages.contains(turn.role.as_str()) {
        return true;
    }

    // Messages with error or warning in content
    let lower_content = turn.content.to_lowercase();
    lower_content.contains("error") || lower_content.contains("warning")
}

/// Finds the least important message that can be removed
#[allow(dead_code)]
fn find_least_important_message(
    turns: &[ConversationTurn],
    config: &CompressionConfig,
) -> Option<usize> {
    // Skip the first and last few messages as they're usually important
    let start = config.keep_first_messages.min(turns.len());
    let end = turns.len().saturating_sub(config.keep_last_messages);

    (start..end).min_by_key(|&i| {
        let turn = &turns[i];
        let importance = if is_important_message(turn, config) {
            usize::MAX
        } else {
            // Prefer to remove shorter messages
            turn.content.len()
        };
        std::cmp::Reverse(importance)
    })
}

/// Estimates the token count of a message (roughly 4 chars per token)
#[allow(dead_code)]
pub fn estimate_token_count(text: &str) -> usize {
    text.chars().count() / 4
}

/// Checks if the conversation needs further compression
#[allow(dead_code)]
pub fn needs_further_compression(turns: &[ConversationTurn], max_tokens: usize) -> bool {
    let total_tokens: usize = turns
        .iter()
        .map(|t| estimate_token_count(&t.content) + 10) // +10 for role and formatting
        .sum();

    total_tokens > max_tokens
}

/// Truncates long messages while preserving important parts
#[allow(dead_code)]
pub fn truncate_message(message: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4; // Rough estimate
    let chars: Vec<char> = message.chars().collect();

    if chars.len() <= max_chars {
        return message.to_string();
    }

    // Try to find a good breaking point (end of sentence) within the limit
    let mut best_end = max_chars.min(chars.len());

    // Search from start to max_chars for the last sentence-ending punctuation
    for (i, &c) in chars.iter().enumerate().take(best_end + 1) {
        if c == '.' || c == '!' || c == '?' || c == '\n' {
            best_end = i + 1;
        }
    }

    // Ensure we actually truncate
    if best_end >= chars.len() {
        best_end = max_chars.min(chars.len());
    }

    chars[..best_end].iter().collect()
}

/// Groups consecutive messages from the same role
#[allow(dead_code)]
pub fn group_consecutive_messages(turns: Vec<ConversationTurn>) -> Vec<ConversationTurn> {
    if turns.is_empty() {
        return Vec::new();
    }

    let config = CompressionConfig::default();
    let mut result = Vec::new();

    // Take ownership of the first turn to avoid borrowing issues
    let mut turns_iter = turns.into_iter();
    let first_turn = turns_iter.next().unwrap();
    let mut current_role = first_turn.role.clone();
    let mut current_content = first_turn.content;
    let mut current_turn_number = first_turn.turn_number;

    for turn in turns_iter {
        if turn.role == current_role {
            if !current_content.is_empty() {
                current_content.push_str("\n\n");
            }
            current_content.push_str(&turn.content);

            // Truncate if needed
            if config.truncate_long_messages
                && estimate_token_count(&current_content) > config.max_tokens_per_message
            {
                current_content = truncate_message(&current_content, config.max_tokens_per_message);
            }
        } else {
            if !current_content.is_empty() {
                result.push(ConversationTurn {
                    role: current_role.clone(),
                    content: std::mem::take(&mut current_content),
                    turn_number: current_turn_number,
                    task_info: None, // Preserve task_info if needed
                });
            }
            current_role = turn.role;
            current_content = turn.content.clone();
            current_turn_number = turn.turn_number;
        }
    }

    // Add the last group
    if !current_content.is_empty() {
        result.push(ConversationTurn {
            role: current_role.clone(),
            content: current_content,
            turn_number: current_turn_number,
            task_info: None, // Preserve task_info if needed
        });
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::core::conversation_summarizer::ConversationTurn;

    fn create_test_turn(role: &str, content: &str, turn_number: usize) -> ConversationTurn {
        ConversationTurn {
            turn_number,
            content: content.to_string(),
            role: role.to_string(),
            task_info: None,
        }
    }

    #[test]
    fn test_compress_conversation_merges_consecutive() {
        let turns = vec![
            create_test_turn("user", "Hello", 1),
            create_test_turn("assistant", "Hi there!", 2),
            create_test_turn("assistant", "How can I help?", 3),
        ];

        let compressed = compress_conversation(&turns);
        assert_eq!(compressed.len(), 2);
        assert_eq!(compressed[0].content, "Hello");
        assert!(compressed[1].content.contains("Hi there!"));
        assert!(compressed[1].content.contains("How can I help?"));
    }

    #[test]
    fn test_compress_conversation_preserves_important() {
        let turns = vec![
            create_test_turn("system_important", "Critical error occurred", 1),
            create_test_turn("system", "Debug info", 2),
            create_test_turn("user", "Hello", 3),
        ];

        let config = CompressionConfig {
            max_turns: 2,
            keep_system_messages: ["system_important"].iter().cloned().collect(),
            ..Default::default()
        };

        let compressed = compress_conversation_with_config(&turns, &config);
        assert_eq!(compressed.len(), 2);
        assert_eq!(compressed[0].role, "system_important");
        assert_eq!(compressed[1].role, "user");
    }

    #[test]
    fn test_truncate_message() {
        let message = "This is a long message that needs to be truncated. It has multiple sentences. This is the end.";
        let truncated = truncate_message(message, 20); // ~5 words
        assert!(truncated.len() < message.len());
        assert!(truncated.ends_with('.'));
    }

    #[test]
    fn test_group_consecutive_messages() {
        let turns = vec![
            create_test_turn("user", "Hello", 1),
            create_test_turn("user", "Are you there?", 2),
            create_test_turn("assistant", "Yes, how can I help?", 3),
        ];

        let grouped = group_consecutive_messages(turns);
        assert_eq!(grouped.len(), 2);
        assert!(grouped[0].content.contains("Hello\n\nAre you there?"));
        assert_eq!(grouped[1].role, "assistant");
    }

    #[test]
    fn test_needs_further_compression() {
        let turns = vec![
            create_test_turn("user", "Hello", 1),
            create_test_turn("assistant", "Hi there!", 2),
        ];

        // Test with a very small token limit
        assert!(needs_further_compression(&turns, 5));

        // Test with a large token limit
        assert!(!needs_further_compression(&turns, 1000));
    }
}
