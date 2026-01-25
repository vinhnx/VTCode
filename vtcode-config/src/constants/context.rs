/// Head ratio percentage for code files (legacy, kept for compatibility)
pub const CODE_HEAD_RATIO_PERCENT: usize = 60;

/// Head ratio percentage for log files (legacy, kept for compatibility)
pub const LOG_HEAD_RATIO_PERCENT: usize = 20;

// =========================================================================
// Context Window Sizes
// =========================================================================

/// Standard context window size (200K tokens) - default for most models
pub const STANDARD_CONTEXT_WINDOW: usize = 200_000;

/// Extended context window size (1M tokens) - beta feature
/// Available for Claude Sonnet 4, Sonnet 4.5 in usage tier 4
/// Requires beta header: "context-1m-2025-08-07"
pub const EXTENDED_CONTEXT_WINDOW: usize = 1_000_000;

/// Claude.ai Enterprise context window (500K tokens)
pub const ENTERPRISE_CONTEXT_WINDOW: usize = 500_000;

// =========================================================================
// Token Budget Thresholds (for proactive context management)
// =========================================================================

/// First warning threshold - start preparing for context handoff
/// At 70% usage: Consider updating key artifacts to persist context
pub const TOKEN_BUDGET_WARNING_THRESHOLD: f64 = 0.70;

/// Second warning threshold - active context management needed
/// At 85% usage: Actively summarize and persist state
pub const TOKEN_BUDGET_HIGH_THRESHOLD: f64 = 0.85;

/// Critical threshold - immediate action required
/// At 90% usage: Force context handoff or summary
pub const TOKEN_BUDGET_CRITICAL_THRESHOLD: f64 = 0.90;

// =========================================================================
// Extended Thinking Token Management
// =========================================================================

/// Minimum budget tokens for extended thinking (Anthropic requirement)
pub const MIN_THINKING_BUDGET: u32 = 1_024;

/// Recommended budget tokens for complex reasoning tasks
pub const RECOMMENDED_THINKING_BUDGET: u32 = 10_000;

/// Default thinking budget for production use (64K output models: Opus 4.5, Sonnet 4.5, Haiku 4.5)
/// Extended thinking is now auto-enabled by default as of January 2026
pub const DEFAULT_THINKING_BUDGET: u32 = 31_999;

/// Maximum thinking budget for 64K output models (Opus 4.5, Sonnet 4.5, Haiku 4.5)
/// Use MAX_THINKING_TOKENS=63999 environment variable to enable this
pub const MAX_THINKING_BUDGET_64K: u32 = 63_999;

/// Maximum thinking budget for 32K output models (Opus 4)
pub const MAX_THINKING_BUDGET_32K: u32 = 31_999;

// =========================================================================
// Beta Headers
// =========================================================================

/// Beta header for 1M token context window
/// Include in requests to enable extended context for Sonnet 4/4.5
pub const BETA_CONTEXT_1M: &str = "context-1m-2025-08-07";

// =========================================================================
// Context-Aware Model Detection
// =========================================================================

/// Models that support context awareness (budget tracking in prompts)
/// Context awareness: model tracks remaining token budget throughout conversation
/// Currently: Claude Sonnet 4.5, Claude Haiku 4.5
pub const CONTEXT_AWARE_MODELS: &[&str] = &[
    "claude-sonnet-4-5",
    "claude-sonnet-4-5-20250514",
    "claude-haiku-4-5",
    "claude-haiku-4-5-20250514",
];

/// Check if a model supports context awareness
pub fn supports_context_awareness(model: &str) -> bool {
    CONTEXT_AWARE_MODELS.iter().any(|m| model.contains(m))
}

/// Models eligible for 1M context window (beta)
/// Requires usage tier 4 or custom rate limits
pub const EXTENDED_CONTEXT_ELIGIBLE_MODELS: &[&str] = &[
    "claude-sonnet-4",
    "claude-sonnet-4-5",
    "claude-sonnet-4-20250514",
    "claude-sonnet-4-5-20250514",
];

/// Check if a model is eligible for 1M context window
pub fn supports_extended_context(model: &str) -> bool {
    EXTENDED_CONTEXT_ELIGIBLE_MODELS
        .iter()
        .any(|m| model.contains(m))
}
