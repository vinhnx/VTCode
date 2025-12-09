//! Unified token budget constants
//!
//! Single source of truth for all token budget-related constants used throughout
//! the system for context management, budgeting, and execution control.
//!
//! All values are centralized here to ensure consistency across:
//! - System prompts and instructions
//! - Token budget manager
//! - Context optimization
//! - Agent execution loops
//! - Tool response handling
//!
//! ## Threshold Hierarchy
//!
//! Token usage progresses through these stages (as % of max context):
//! 1. **Normal** (0-75%): Full operation, no restrictions
//! 2. **Warning** (75-85%): Start consolidating outputs
//! 3. **Compact** (85-90%): Begin progressive compaction
//! 4. **Alert/Critical** (90-94%): Aggressive pruning and compaction
//! 5. **Checkpoint** (>95%): Create progress checkpoint, prepare context reset
//!
//! ## Model Defaults
//!
//! - **Max Context**: 128K tokens (standard for most models)
//! - **Max Tool Response**: 25K tokens per tool call
//! - **Timeout**: 12K tokens for PTY output

use crate::core::token_budget_constants as budget;

/// ============================================================================
/// PERCENTAGE THRESHOLDS (Normalized 0.0-1.0)
/// ============================================================================
/// Normal operation threshold: < 75% usage
/// At this point, operation is unrestricted and no token management is needed.
pub const THRESHOLD_NORMAL: f64 = budget::THRESHOLD_NORMAL;

/// Warning threshold: 75% usage
/// Start consolidating outputs and removing verbose logging.
pub const THRESHOLD_WARNING: f64 = budget::THRESHOLD_WARNING;

/// Conservative threshold: 70% usage (safety-first checks)
pub const THRESHOLD_CONSERVATIVE: f64 = budget::THRESHOLD_CONSERVATIVE;

/// Compact threshold: 90% usage
/// Begin progressive compaction of context history.
pub const THRESHOLD_COMPACT: f64 = budget::THRESHOLD_COMPACT;

/// Alert threshold: 85% usage
/// Aggressive pruning of context. Used by TokenBudgetManager as default alert.
pub const THRESHOLD_ALERT: f64 = budget::THRESHOLD_ALERT;

/// Checkpoint threshold: 95% usage
/// Create .progress.md checkpoint and prepare for context reset.
pub const THRESHOLD_CHECKPOINT: f64 = budget::THRESHOLD_CHECKPOINT;

/// Critical threshold for aggressive pruning
pub const THRESHOLD_CRITICAL: f64 = budget::THRESHOLD_CRITICAL;

/// Emergency threshold prior to checkpoint
pub const THRESHOLD_EMERGENCY: f64 = budget::THRESHOLD_EMERGENCY;

/// ============================================================================
/// TOKEN APPROXIMATION CONSTANTS (For fallback token counting)
/// ============================================================================
/// Approximate tokens per character for English prose
pub const TOKENS_PER_CHARACTER: f64 = budget::TOKENS_PER_CHARACTER;

/// Approximate tokens per line for structured output (logs, diffs)
pub const TOKENS_PER_LINE: usize = budget::TOKENS_PER_LINE;

/// Minimum token count to return (avoid 0 estimates)
pub const MIN_TOKEN_COUNT: usize = budget::MIN_TOKEN_COUNT;

/// Characters that indicate code (brackets, operators, etc.)
pub const CODE_INDICATOR_CHARS: &[char] = budget::CODE_INDICATOR_CHARS;

/// Threshold for code detection (% of text containing code indicators)
pub const CODE_DETECTION_THRESHOLD: usize = budget::CODE_DETECTION_THRESHOLD;

/// Head ratio percentage for code truncation
pub const CODE_HEAD_RATIO_PERCENT: usize = budget::CODE_HEAD_RATIO_PERCENT;

/// Head ratio percentage for log/text truncation
pub const LOG_HEAD_RATIO_PERCENT: usize = budget::LOG_HEAD_RATIO_PERCENT;

/// Multiplier for code token counts (code is denser than prose)
pub const CODE_TOKEN_MULTIPLIER: f64 = budget::CODE_TOKEN_MULTIPLIER;

/// Long word threshold for token adjustment
pub const LONG_WORD_THRESHOLD: usize = budget::LONG_WORD_THRESHOLD;

/// Reduction factor for long word analysis
pub const LONG_WORD_CHAR_REDUCTION: usize = budget::LONG_WORD_CHAR_REDUCTION;

/// Scale factor for long word calculations
pub const LONG_WORD_SCALE_FACTOR: usize = budget::LONG_WORD_SCALE_FACTOR;

/// ============================================================================
/// TOKEN LIMITS (Absolute Values)
/// ============================================================================
/// Maximum context tokens per model (configurable)
/// Standard default: 128,000 tokens
/// Used by TokenBudgetConfig::default() and most LLM providers
pub const DEFAULT_MAX_CONTEXT_TOKENS: usize = budget::DEFAULT_MAX_CONTEXT_TOKENS;

/// Maximum tokens allowed per tool response
/// All tool outputs are truncated at this limit to prevent context explosion
/// Must be significantly less than max_context to leave room for conversation
pub const MAX_TOOL_RESPONSE_TOKENS: usize = budget::MAX_TOOL_RESPONSE_TOKENS;

/// Maximum tokens for PTY output (terminal commands)
/// Shell/command outputs are truncated at this limit
pub const MAX_PTY_OUTPUT_TOKENS: usize = budget::MAX_PTY_OUTPUT_TOKENS;

/// Model output token limit (for model-generated content)
/// Standard maximum tokens for model responses
pub const MODEL_OUTPUT_TOKEN_LIMIT: usize = budget::MODEL_OUTPUT_TOKEN_LIMIT;

/// ============================================================================
/// ALTERNATIVE THRESHOLDS (For comparison/reference)
/// ============================================================================
/// Minimum confidence threshold for fallback chains
pub const MIN_CONFIDENCE_THRESHOLD: f64 = budget::MIN_CONFIDENCE_THRESHOLD;

/// Relevance score threshold
pub const RELEVANCE_THRESHOLD: f64 = budget::RELEVANCE_THRESHOLD;

/// Pattern similarity threshold for loop detection
pub const PATTERN_SIMILARITY_THRESHOLD: f64 = budget::PATTERN_SIMILARITY_THRESHOLD;

/// Quality score threshold
pub const QUALITY_THRESHOLD: f64 = budget::QUALITY_THRESHOLD;

/// High quality threshold
pub const HIGH_QUALITY_THRESHOLD: f64 = budget::HIGH_QUALITY_THRESHOLD;

/// Conservative threshold (50% usage)
pub const CONSERVATIVE_THRESHOLD: f64 = budget::THRESHOLD_CONSERVATIVE;

/// ============================================================================
/// CONVENIENCE FUNCTIONS
/// ============================================================================
/// Check if usage is in normal state (< 75%)
pub fn is_normal_usage(ratio: f64) -> bool {
    ratio < THRESHOLD_NORMAL
}

/// Check if usage is in warning state (75-85%)
pub fn is_warning_usage(ratio: f64) -> bool {
    (THRESHOLD_WARNING..THRESHOLD_ALERT).contains(&ratio)
}

/// Check if usage is in compact state (85-90%)
pub fn is_compact_usage(ratio: f64) -> bool {
    (THRESHOLD_ALERT..THRESHOLD_COMPACT).contains(&ratio)
}

/// Check if usage is in alert state (90-95%)
pub fn is_alert_usage(ratio: f64) -> bool {
    (THRESHOLD_COMPACT..THRESHOLD_CHECKPOINT).contains(&ratio)
}

/// Check if usage requires checkpoint (> 95%)
pub fn requires_checkpoint(ratio: f64) -> bool {
    ratio >= THRESHOLD_CHECKPOINT
}

/// Get human-readable status for usage ratio
pub fn status_for_ratio(ratio: f64) -> &'static str {
    match () {
        _ if requires_checkpoint(ratio) => "CHECKPOINT",
        _ if is_alert_usage(ratio) => "ALERT",
        _ if is_compact_usage(ratio) => "COMPACT",
        _ if is_warning_usage(ratio) => "WARNING",
        _ => "NORMAL",
    }
}

/// Calculate tokens used given a ratio and max tokens
pub fn tokens_at_ratio(ratio: f64, max_tokens: usize) -> usize {
    ((ratio * max_tokens as f64).ceil()) as usize
}

/// Calculate ratio given tokens used and max tokens
pub fn ratio_from_tokens(used: usize, max_tokens: usize) -> f64 {
    if max_tokens == 0 {
        0.0
    } else {
        used as f64 / max_tokens as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold_ordering() {
        assert!(THRESHOLD_NORMAL < THRESHOLD_ALERT);
        assert!(THRESHOLD_ALERT < THRESHOLD_COMPACT);
        assert!(THRESHOLD_COMPACT < THRESHOLD_CHECKPOINT);
    }

    #[test]
    fn test_status_functions() {
        assert!(is_normal_usage(0.5));
        assert!(is_warning_usage(0.80));
        assert!(is_compact_usage(0.88));
        assert!(is_alert_usage(0.92));
        assert!(requires_checkpoint(0.96));
    }

    #[test]
    fn test_status_labels() {
        assert_eq!(status_for_ratio(0.5), "NORMAL");
        assert_eq!(status_for_ratio(0.80), "WARNING");
        assert_eq!(status_for_ratio(0.88), "COMPACT");
        assert_eq!(status_for_ratio(0.92), "ALERT");
        assert_eq!(status_for_ratio(0.96), "CHECKPOINT");
    }

    #[test]
    fn test_token_calculations() {
        // At 75%, 128K tokens = 96K
        assert_eq!(tokens_at_ratio(0.75, 128_000), 96_000);

        // At 85%, 128K tokens = 108.8K
        assert_eq!(tokens_at_ratio(0.85, 128_000), 108_800);

        // Reverse calculation
        assert!((ratio_from_tokens(96_000, 128_000) - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_token_limits() {
        // Verify relationships
        assert!(MAX_TOOL_RESPONSE_TOKENS < DEFAULT_MAX_CONTEXT_TOKENS);
        assert!(MAX_PTY_OUTPUT_TOKENS < MAX_TOOL_RESPONSE_TOKENS);
        assert!(MODEL_OUTPUT_TOKEN_LIMIT < MAX_PTY_OUTPUT_TOKENS);
    }
}
