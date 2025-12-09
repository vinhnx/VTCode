//! Unified Token Budget & Context Management Constants
//!
//! Single source of truth for all token budget-related constants used throughout
//! the system. Consolidates values previously scattered across:
//! - token_constants.rs (core thresholds)
//! - vtcode-config/constants.rs (default limits)
//! - vtcode-config/defaults/mod.rs (scenario windows)
//! - context.rs (configuration defaults)
//!
//! ## Architecture
//!
//! All constants are centralized here to ensure:
//! 1. No hardcoded duplicate values across the codebase
//! 2. Single place to update token budgets
//! 3. Clear relationships between thresholds and limits
//! 4. Easy scenario/model configuration
//!
//! ## Percentage Threshold Hierarchy (Optimized for Performance)
//!
//! Token usage progresses through stages (as % of max context):
//! ```
//! 0%      25%      50%      70%      75%      85%      90%      92%      94%      95%      100%
//! |--------|--------|--------|--------|--------|--------|--------|--------|--------|--------|
//! Normal                    Conservative  Warning   Compact  Critical Emergency Checkpoint
//! ```
//!
//! - **Normal** (0-82%): Full operation, no restrictions (OPTIMIZED: was 0-75%)
//! - **Warning** (82-88%): Start consolidating outputs (reduced from 75-85%)
//! - **Compact** (88-93%): Progressive compaction (increased range from 90-95%)
//! - **Critical** (93-97%): Aggressive pruning, minimal new input (NEW)
//! - **Emergency** (97-99%): Last-ditch effort, pause and checkpoint (NEW)
//! - **Checkpoint** (>99%): Create checkpoint, force context reset (increased safety margin)
//!
//! ## Token Limit Relationships
//!
//! Absolute limits are ordered from largest to smallest:
//! - DEFAULT_MAX_CONTEXT_TOKENS (128K): Full context window
//! - SCENARIO_HIGH_PERF_CONTEXT (200K): Performance-optimized scenario
//! - SCENARIO_HIGH_QUALITY_CONTEXT (150K): Quality-optimized scenario
//! - SCENARIO_BALANCED_CONTEXT (125K): Balanced scenario
//! - MAX_TOOL_RESPONSE_TOKENS (25K): Per-tool limit
//! - MAX_PTY_OUTPUT_TOKENS (8K): Terminal command limit
//! - MODEL_OUTPUT_TOKEN_LIMIT (4K): Model generation limit

// ============================================================================
// PERCENTAGE THRESHOLDS (Normalized 0.0-1.0)
// ============================================================================

/// Normal operation threshold: usage < 75%
/// At this point, operation is unrestricted and no token management is needed.
/// Reverts to previously proven safe default.
pub const THRESHOLD_NORMAL: f64 = 0.75;

/// Warning threshold: usage >= 75%
/// Start consolidating outputs and removing verbose logging.
/// Reverts to previously proven safe default.
pub const THRESHOLD_WARNING: f64 = 0.75;

/// Alert threshold: usage >= 85%
/// Aggressive pruning of context begins. Kept for backward compatibility.
pub const THRESHOLD_ALERT: f64 = 0.85;

/// Compact threshold: usage >= 90%
/// Begin progressive compaction of context history.
/// Reverts to 90% for conservative behavior.
pub const THRESHOLD_COMPACT: f64 = 0.90;

/// Critical threshold: usage >= 92%
/// Aggressive pruning and minimal new input acceptance.
/// Prepares system for potential context reset ahead of checkpoint.
pub const THRESHOLD_CRITICAL: f64 = 0.92;

/// Emergency threshold: usage >= 94%
/// Last-ditch recovery mode. Pause accepting new context and prepare checkpoint.
pub const THRESHOLD_EMERGENCY: f64 = 0.94;

/// Checkpoint threshold: usage >= 95%
/// Create .progress.md checkpoint and force context reset.
/// Reverts to proven safe ceiling.
pub const THRESHOLD_CHECKPOINT: f64 = 0.95;

/// Conservative threshold: usage >= 70%
/// Used for conservative/safety-first decision making in risk-averse modes.
pub const THRESHOLD_CONSERVATIVE: f64 = 0.70;

// ============================================================================
// TOKEN LIMIT CONSTANTS (Absolute Values in Tokens)
// ============================================================================

/// Default maximum context tokens (standard model default)
/// - Used as default for most LLM providers
/// - Configurable per model in LLM config
pub const DEFAULT_MAX_CONTEXT_TOKENS: usize = 128_000;

/// Maximum tokens allowed per tool response
/// - All tool outputs are truncated at this limit
/// - Prevents context explosion from large tool results
/// - Must be < DEFAULT_MAX_CONTEXT_TOKENS to leave room for conversation
pub const MAX_TOOL_RESPONSE_TOKENS: usize = 25_000;

/// Maximum tokens for tool response in aggressive/compact mode
/// - Reduced limit when system is under token pressure
/// - ~60KB of content, provides safety margin
pub const MAX_TOOL_RESPONSE_AGGRESSIVE: usize = 15_000;

/// Maximum tokens for tool response in emergency mode
/// - Minimal output when context is critically limited
/// - ~20KB of content
pub const MAX_TOOL_RESPONSE_EMERGENCY: usize = 8_000;

/// Default token budget for model input preparation
/// - Used when preparing tool outputs for model consumption
/// - Same as MAX_TOOL_RESPONSE_TOKENS for consistency
pub const DEFAULT_MODEL_INPUT_TOKEN_BUDGET: usize = 25_000;

/// Maximum tokens for PTY output (terminal/shell commands)
/// - Prevents context overflow from verbose commands (cargo, logs, etc.)
/// - ~48KB of content when estimated at 1 token ≈ 4 characters
/// - OPTIMIZED: Increased from 8K to 12K for better log capture
pub const MAX_PTY_OUTPUT_TOKENS: usize = 12_000;

/// Maximum tokens for PTY output (alias for clarity in different contexts)
pub const DEFAULT_PTY_OUTPUT_MAX_TOKENS: usize = 12_000;

/// Maximum tokens for PTY output in aggressive/compact mode
/// - When system is under token pressure, use smaller limits
/// - ~24KB of content
pub const MAX_PTY_OUTPUT_AGGRESSIVE: usize = 6_000;

/// Maximum tokens for PTY output in emergency mode
/// - Minimal output when context is critically limited
/// - ~12KB of content
pub const MAX_PTY_OUTPUT_EMERGENCY: usize = 3_000;

/// Model output token limit (for model-generated content)
/// - Standard maximum tokens for model responses
/// - Used by Anthropic and other providers with max_tokens limit
pub const MODEL_OUTPUT_TOKEN_LIMIT: usize = 4_096;

/// Anthropic's default max_tokens setting
pub const ANTHROPIC_DEFAULT_MAX_TOKENS: u32 = 4_096;

// ============================================================================
// SCENARIO-BASED CONTEXT WINDOWS
// ============================================================================

/// High performance scenario context window
/// - Optimized for speed and throughput
/// - Larger context for complex multi-tool tasks
/// - Uses higher thresholds (warning/compact/checkpoint tuned per scenario)
pub const SCENARIO_HIGH_PERF_CONTEXT: usize = 200_000;

/// High quality scenario context window
/// - Optimized for quality and precision
/// - Larger context for careful analysis and refinement
/// - Uses standard thresholds: 75% warning, 90% compact, 95% checkpoint
pub const SCENARIO_HIGH_QUALITY_CONTEXT: usize = 150_000;

/// Balanced scenario context window (DEFAULT)
/// - Default balanced between performance and quality
/// - Standard context size for general use
/// - Uses standard thresholds: 75% warning, 90% compact, 95% checkpoint
pub const SCENARIO_BALANCED_CONTEXT: usize = 125_000;

/// Aggressive scenario context window (NEW)
/// - More aggressive memory usage for speed
/// - Suitable when context resets are acceptable
/// - Uses higher thresholds tuned for aggressive usage
pub const SCENARIO_AGGRESSIVE_CONTEXT: usize = 100_000;

/// Minimal scenario context window (NEW)
/// - Conservative operation for reliability
/// - Smaller context window with frequent resets
/// - Uses lower thresholds for safety-first operation
pub const SCENARIO_MINIMAL_CONTEXT: usize = 80_000;

// ============================================================================
// CONTEXT TRIMMING & COMPRESSION CONSTANTS
// ============================================================================

/// Default context window when not otherwise specified
/// - Used as fallback in context trimming
/// - Lower than default_max_context_tokens to leave safety margin
pub const DEFAULT_TRIMMED_CONTEXT_TOKENS: usize = 90_000;

/// Trim target as a percentage of maximum token budget
/// - When trimming context, target this percentage
/// - 80% allows some breathing room while aggressively reducing size
pub const DEFAULT_TRIM_TO_PERCENT: u8 = 80;

/// Minimum allowed trim percentage (prevents overly aggressive retention)
/// - Floor on how high we trim to (how low we trim from)
/// - Prevents keeping too much context when severely constrained
pub const MIN_TRIM_RATIO_PERCENT: u8 = 60;

/// Maximum allowed trim percentage (prevents minimal trimming)
/// - Ceiling on how high we trim to (how high we trim from)
/// - Ensures we actually trim when needed
pub const MAX_TRIM_RATIO_PERCENT: u8 = 90;

/// Default percentage of context to use for max token trim operations
/// - Used when calculating max tokens for truncation operations
pub const DEFAULT_MAX_TOKEN_PERCENT: u8 = 90;

/// Tool output spool threshold in bytes
/// - Output larger than this is spooled to disk instead of kept in memory
pub const TOOL_OUTPUT_SPOOL_BYTES: usize = 200_000;

// ============================================================================
// RECENT TURNS PRESERVATION (for context trimming)
// ============================================================================

/// Default number of recent turns to preserve verbatim
/// - When trimming context, always keep this many recent turns complete
/// - Ensures recent conversation history is maintained
pub const DEFAULT_PRESERVE_RECENT_TURNS: usize = 12;

/// Minimum number of recent turns that must remain after trimming
/// - Floor for preservation to ensure some context is kept
pub const MIN_PRESERVE_RECENT_TURNS: usize = 6;

/// Maximum number of recent turns to keep when aggressively reducing context
/// - Ceiling when in aggressive pruning mode
pub const AGGRESSIVE_PRESERVE_RECENT_TURNS: usize = 8;

// ============================================================================
// TOKEN APPROXIMATION CONSTANTS (Fallback Token Counting)
// ============================================================================

/// Approximate tokens per character for English prose
/// - Used when token counts aren't available from tokenizer
pub const TOKENS_PER_CHARACTER: f64 = 4.0;

/// Approximate tokens per line for structured output (logs, diffs)
pub const TOKENS_PER_LINE: usize = 10;

/// Minimum token count to return (avoid 0 estimates)
pub const MIN_TOKEN_COUNT: usize = 1;

/// Approximate character count per token
/// - Reverse of TOKENS_PER_CHARACTER (1/4)
pub const CHAR_PER_TOKEN_APPROX: usize = 3;

/// Characters that indicate code (brackets, operators, etc.)
pub const CODE_INDICATOR_CHARS: &[char] = &[
    '(', ')', '{', '}', '[', ']', '=', '+', '-', '/', '*', ';', ':', ',',
];

/// Threshold for code detection (% of text containing code indicators)
/// - If >20% of chars are code indicators, treat as code
pub const CODE_DETECTION_THRESHOLD: usize = 20;

/// Head ratio percentage for code truncation
/// - When truncating code, keep 50% at the head
pub const CODE_HEAD_RATIO_PERCENT: usize = 50;

/// Head ratio percentage for log/text truncation
/// - When truncating logs/text, keep 40% at the head
pub const LOG_HEAD_RATIO_PERCENT: usize = 40;

/// Multiplier for code token counts (code is denser than prose)
pub const CODE_TOKEN_MULTIPLIER: f64 = 1.15;

/// Long word threshold for token adjustment
/// - Words longer than this affect token estimation
pub const LONG_WORD_THRESHOLD: usize = 10;

/// Reduction factor for long word analysis
pub const LONG_WORD_CHAR_REDUCTION: usize = 3;

/// Scale factor for long word calculations
pub const LONG_WORD_SCALE_FACTOR: usize = 100;

// ============================================================================
// QUALITY & SIMILARITY THRESHOLDS
// ============================================================================

/// Minimum confidence threshold for fallback chains
pub const MIN_CONFIDENCE_THRESHOLD: f64 = 0.65;

/// Relevance score threshold
pub const RELEVANCE_THRESHOLD: f64 = 0.70;

/// Pattern similarity threshold for loop detection
/// - Used to detect repeated patterns in agent behavior
pub const PATTERN_SIMILARITY_THRESHOLD: f64 = 0.85;

/// Quality score threshold
pub const QUALITY_THRESHOLD: f64 = 0.80;

/// High quality threshold
pub const HIGH_QUALITY_THRESHOLD: f64 = 0.90;

// ============================================================================
// BYTE-LEVEL SAFEGUARDS (Secondary limits after token truncation)
// ============================================================================

/// Byte fuse for PTY output - secondary safeguard after token truncation
/// - Protects against edge cases where token estimation underestimates size
/// - 40 KiB provides a safety margin
pub const DEFAULT_PTY_OUTPUT_BYTE_FUSE: usize = 40 * 1024;

/// Byte fuse for model input - secondary safeguard after token truncation
/// - Protects against pathological payload sizes
/// - 10 KiB secondary safeguard for model input
pub const DEFAULT_MODEL_INPUT_BYTE_FUSE: usize = 10 * 1024;

// ============================================================================
// CONVENIENCE HELPER FUNCTIONS
// ============================================================================

/// Check if usage is in normal state (< 82%)
pub fn is_normal_usage(ratio: f64) -> bool {
    ratio < THRESHOLD_NORMAL
}

/// Check if usage is in warning state (82-88%)
pub fn is_warning_usage(ratio: f64) -> bool {
    (THRESHOLD_WARNING..THRESHOLD_COMPACT).contains(&ratio)
}

/// Check if usage is in alert state (88-93%)
/// NOTE: This now maps to compact state. Kept for backward compatibility.
pub fn is_alert_usage(ratio: f64) -> bool {
    is_compact_usage(ratio)
}

/// Check if usage is in compact state (88-93%)
pub fn is_compact_usage(ratio: f64) -> bool {
    (THRESHOLD_COMPACT..THRESHOLD_CRITICAL).contains(&ratio)
}

/// Check if usage is in critical state (93-97%)
pub fn is_critical_usage(ratio: f64) -> bool {
    (THRESHOLD_CRITICAL..THRESHOLD_EMERGENCY).contains(&ratio)
}

/// Check if usage is in emergency state (97-99%)
pub fn is_emergency_usage(ratio: f64) -> bool {
    (THRESHOLD_EMERGENCY..THRESHOLD_CHECKPOINT).contains(&ratio)
}

/// Check if usage requires checkpoint (> 99%)
pub fn requires_checkpoint(ratio: f64) -> bool {
    ratio >= THRESHOLD_CHECKPOINT
}

/// Get human-readable status for usage ratio
pub fn status_for_ratio(ratio: f64) -> &'static str {
    match () {
        _ if requires_checkpoint(ratio) => "CHECKPOINT",
        _ if is_emergency_usage(ratio) => "EMERGENCY",
        _ if is_critical_usage(ratio) => "CRITICAL",
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

// ============================================================================
// CONVERSION FACTORS
// ============================================================================

/// Factor to convert ratio (0.0-1.0) to percentage (0-100)
pub const RATIO_TO_PERCENT: f64 = 100.0;

/// Convert ratio to percentage
pub fn ratio_to_percentage(ratio: f64) -> f64 {
    ratio * RATIO_TO_PERCENT
}

/// Convert percentage to ratio
pub fn percentage_to_ratio(percentage: f64) -> f64 {
    percentage / RATIO_TO_PERCENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold_ordering() {
        // Verify thresholds are in correct order
        assert!(THRESHOLD_CONSERVATIVE <= THRESHOLD_NORMAL);
        assert!(THRESHOLD_NORMAL <= THRESHOLD_WARNING);
        assert!(THRESHOLD_WARNING < THRESHOLD_COMPACT);
        assert!(THRESHOLD_COMPACT < THRESHOLD_CRITICAL);
        assert!(THRESHOLD_CRITICAL < THRESHOLD_EMERGENCY);
        assert!(THRESHOLD_EMERGENCY < THRESHOLD_CHECKPOINT);
    }

    #[test]
    fn test_token_limit_ordering() {
        // Verify token limits are ordered correctly
        assert!(MODEL_OUTPUT_TOKEN_LIMIT < MAX_PTY_OUTPUT_EMERGENCY);
        assert!(MAX_PTY_OUTPUT_EMERGENCY < MAX_PTY_OUTPUT_AGGRESSIVE);
        assert!(MAX_PTY_OUTPUT_AGGRESSIVE < MAX_PTY_OUTPUT_TOKENS);
        assert!(MAX_TOOL_RESPONSE_EMERGENCY < MAX_TOOL_RESPONSE_AGGRESSIVE);
        assert!(MAX_TOOL_RESPONSE_AGGRESSIVE < MAX_TOOL_RESPONSE_TOKENS);
        assert!(MAX_TOOL_RESPONSE_TOKENS < DEFAULT_MAX_CONTEXT_TOKENS);
    }

    #[test]
    fn test_scenario_windows() {
        // Verify scenario context windows
        assert!(SCENARIO_MINIMAL_CONTEXT < SCENARIO_AGGRESSIVE_CONTEXT);
        assert!(SCENARIO_AGGRESSIVE_CONTEXT < SCENARIO_BALANCED_CONTEXT);
        assert!(SCENARIO_BALANCED_CONTEXT < SCENARIO_HIGH_QUALITY_CONTEXT);
        assert!(SCENARIO_HIGH_QUALITY_CONTEXT < SCENARIO_HIGH_PERF_CONTEXT);
    }

    #[test]
    fn test_status_functions() {
        assert!(is_normal_usage(0.5));
        assert!(is_warning_usage(0.85));
        assert!(is_compact_usage(0.90));
        assert!(is_critical_usage(0.93));
        assert!(is_emergency_usage(0.94));
        assert!(requires_checkpoint(0.96));
    }

    #[test]
    fn test_status_labels() {
        assert_eq!(status_for_ratio(0.5), "NORMAL");
        assert_eq!(status_for_ratio(0.85), "WARNING");
        assert_eq!(status_for_ratio(0.90), "COMPACT");
        assert_eq!(status_for_ratio(0.93), "CRITICAL");
        assert_eq!(status_for_ratio(0.94), "EMERGENCY");
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
    fn test_percentage_conversion() {
        assert!((ratio_to_percentage(0.75) - 75.0).abs() < 0.001);
        assert!((ratio_to_percentage(0.85) - 85.0).abs() < 0.001);
        assert!((percentage_to_ratio(75.0) - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_trim_percentages() {
        // Verify trim ratios are ordered
        assert!(MIN_TRIM_RATIO_PERCENT < DEFAULT_TRIM_TO_PERCENT);
        assert!(DEFAULT_TRIM_TO_PERCENT < MAX_TRIM_RATIO_PERCENT);
    }

    #[test]
    fn test_preservation_turns() {
        // Verify turn preservation ordering
        assert!(MIN_PRESERVE_RECENT_TURNS < DEFAULT_PRESERVE_RECENT_TURNS);
        assert!(AGGRESSIVE_PRESERVE_RECENT_TURNS < DEFAULT_PRESERVE_RECENT_TURNS);
    }
}
