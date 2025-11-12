//! Token budget approximation constants
//!
//! This module defines all magic numbers used in token counting and truncation logic.
//! Extracted for reusability and clarity.

/// Estimated tokens per character in regular text/code
/// Based on empirical observations: ~1 token per 3.5 characters
/// More conservative than 4.0 to account for punctuation becoming separate tokens
pub const TOKENS_PER_CHARACTER: f64 = 3.5;

/// Threshold for word length (chars) that triggers extra token allocation
/// Words longer than this get extra token consideration
pub const LONG_WORD_THRESHOLD: usize = 8;

/// Character reduction threshold for calculating extra tokens from long words
pub const LONG_WORD_CHAR_REDUCTION: usize = 6;

/// Scaling factor for extra tokens from long words
/// Used as: (word_count * extra_tokens / LONG_WORD_SCALE_FACTOR)
pub const LONG_WORD_SCALE_FACTOR: usize = 10;

/// Estimated tokens per non-empty line in structured output
/// Used for logs, diffs, and other line-oriented output
/// Based on observation: ~15 tokens per line on average
pub const TOKENS_PER_LINE: usize = 15;

/// Character ratio threshold for code detection
/// Content is considered "code" if bracket count > (char_count / CODE_DETECTION_THRESHOLD)
pub const CODE_DETECTION_THRESHOLD: usize = 20;

/// Characters that indicate code (brackets, operators, separators)
pub const CODE_INDICATOR_CHARS: &str = "{}[]<>()=;:,";

/// Percentage increase for token estimation when content is detected as code
/// Code typically has more fragments (brackets, operators) than regular text
/// Used as: (estimate * CODE_TOKEN_MULTIPLIER).ceil()
pub const CODE_TOKEN_MULTIPLIER: f64 = 1.1;

/// Head allocation ratio for code content (%)
/// When truncating code, allocate 50% to head, 50% to tail
/// Rationale: logic distributed throughout file
pub const CODE_HEAD_RATIO_PERCENT: usize = 50;

/// Head allocation ratio for log/output content (%)
/// When truncating logs, allocate 40% to head, 60% to tail
/// Rationale: errors and summaries appear at end
pub const LOG_HEAD_RATIO_PERCENT: usize = 40;

/// Default minimum token result
/// Ensures we never return 0 tokens for non-empty content
pub const MIN_TOKEN_COUNT: usize = 1;

/// Display-level constants (UI safety limits, not semantic)
/// These apply AFTER token-based truncation to prevent TUI lag

/// Maximum line length in characters to prevent TUI hang
/// Long lines are wrapped or truncated at display time
pub const MAX_LINE_LENGTH_FOR_DISPLAY: usize = 150;

/// Maximum number of lines to show in inline mode rendering
/// Full output spooled to .vtcode/tool-output/ for later review
pub const INLINE_STREAM_MAX_LINES_LIMIT: usize = 30;

/// Maximum code lines to show in code fence blocks
/// Semantic content already truncated by token limit upstream
pub const MAX_CODE_LINES_FOR_DISPLAY: usize = 500;

/// Maximum content width for code fence rendering (characters)
pub const CODE_FENCE_MAX_WIDTH: usize = 96;

/// Character reduction for code fence content margins
/// Applied as: MAX_WIDTH.saturating_sub(FENCE_MARGIN)
pub const CODE_FENCE_MARGIN: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_constants_are_positive() {
        assert!(TOKENS_PER_CHARACTER > 0.0);
        assert!(LONG_WORD_THRESHOLD > 0);
        assert!(LONG_WORD_CHAR_REDUCTION > 0);
        assert!(LONG_WORD_SCALE_FACTOR > 0);
        assert!(TOKENS_PER_LINE > 0);
        assert!(CODE_DETECTION_THRESHOLD > 0);
        assert!(CODE_TOKEN_MULTIPLIER > 1.0);
    }

    #[test]
    fn head_ratios_are_valid_percentages() {
        assert!(CODE_HEAD_RATIO_PERCENT <= 100);
        assert!(LOG_HEAD_RATIO_PERCENT <= 100);
        assert!(CODE_HEAD_RATIO_PERCENT > 0);
        assert!(LOG_HEAD_RATIO_PERCENT > 0);
    }

    #[test]
    fn code_detection_chars_not_empty() {
        assert!(!CODE_INDICATOR_CHARS.is_empty());
    }
}
