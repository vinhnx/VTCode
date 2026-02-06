// Standard model for straightforward tools - Sonnet 4.5 preferred for most use cases
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-5";
pub const SUPPORTED_MODELS: &[&str] = &[
    // Claude 4.5 series
    "claude-sonnet-4-5-20250929", // Latest flagship model for complex agents and coding
    "claude-haiku-4-5-20251001",  // Fastest model with near-frontier intelligence
    "claude-opus-4-5-20251101",   // Premium flagship model with exceptional intelligence
    "claude-opus-4-1-20250805",   // Specialized reasoning model
    "claude-sonnet-4-5",          // Alias for latest Claude Sonnet 4.5
    "claude-haiku-4-5",           // Alias for latest Claude Haiku 4.5
    "claude-opus-4-5",            // Alias for latest Claude Opus 4.5
    "claude-opus-4-6",            // Alias for Claude Opus 4.6
    "claude-opus-4-1",            // Alias for latest Claude Opus 4.1
    // Claude 4 series
    "claude-sonnet-4-20250514", // Claude 4 Sonnet
    "claude-opus-4-20250514",   // Claude 4 Opus
    "claude-sonnet-4-0",        // Alias for Claude 4 Sonnet
    "claude-opus-4-0",          // Alias for Claude 4 Opus
    // Claude 3.x series
    "claude-3-7-sonnet-20250219", // Latest Claude 3.7 Sonnet
    "claude-3-7-sonnet-latest",   // Alias for Claude 3.7 Sonnet
    "claude-haiku-4-5",           // Latest Claude 3.5 Sonnet
    "claude-haiku-4-5",           // Alias for Claude 3.5 Sonnet
    "claude-3-5-haiku-20241022",  // Latest Claude 3.5 Haiku
    "claude-3-5-haiku-latest",    // Alias for latest Claude 3.5 Haiku
    "claude-3-opus-20240229",     // Legacy Claude 3 Opus
    "claude-haiku-4-5-20240229",  // Legacy Claude 3 Sonnet
    "claude-3-haiku-20240307",    // Legacy Claude 3 Haiku
];

// Convenience constants for versioned models
pub const CLAUDE_SONNET_4_5_20250929: &str = "claude-sonnet-4-5-20250929";
pub const CLAUDE_HAIKU_4_5_20251001: &str = "claude-haiku-4-5-20251001";
pub const CLAUDE_OPUS_4_5_20251101: &str = "claude-opus-4-5-20251101";
pub const CLAUDE_OPUS_4_1_20250805: &str = "claude-opus-4-1-20250805";
pub const CLAUDE_SONNET_4_20250514: &str = "claude-sonnet-4-20250514";
pub const CLAUDE_OPUS_4_20250514: &str = "claude-opus-4-20250514";
pub const CLAUDE_3_7_SONNET_20250219: &str = "claude-3-7-sonnet-20250219";
pub const CLAUDE_3_5_SONNET_20241022: &str = "claude-haiku-4-5";
pub const CLAUDE_3_5_HAIKU_20241022: &str = "claude-3-5-haiku-20241022";

// Convenience constants for alias models
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
pub const CLAUDE_SONNET_4_5: &str = "claude-sonnet-4-5";
pub const CLAUDE_OPUS_4_5: &str = "claude-opus-4-5";
pub const CLAUDE_OPUS_4_6: &str = "claude-opus-4-6";
pub const CLAUDE_OPUS_4_1: &str = "claude-opus-4-1";
pub const CLAUDE_SONNET_4_0: &str = "claude-sonnet-4-0";
pub const CLAUDE_OPUS_4_0: &str = "claude-opus-4-0";
pub const CLAUDE_3_7_SONNET_LATEST: &str = "claude-3-7-sonnet-latest";
pub const CLAUDE_3_5_SONNET_LATEST: &str = "claude-haiku-4-5";
pub const CLAUDE_3_5_HAIKU_LATEST: &str = "claude-3-5-haiku-latest";

// Legacy aliases for backwards compatibility
pub const CLAUDE_OPUS_4_1_20250805_LEGACY: &str = "claude-opus-4-1-20250805";

/// Models that accept the reasoning effort parameter or extended thinking
pub const REASONING_MODELS: &[&str] = &[
    CLAUDE_SONNET_4_5_20250929,
    CLAUDE_HAIKU_4_5_20251001,
    CLAUDE_OPUS_4_5_20251101,
    CLAUDE_OPUS_4_6,
    CLAUDE_OPUS_4_1_20250805,
    CLAUDE_SONNET_4_5,
    CLAUDE_HAIKU_4_5,
    CLAUDE_OPUS_4_5,
    CLAUDE_OPUS_4_6,
    CLAUDE_OPUS_4_1,
    "claude-sonnet-4-20250514",
    "claude-opus-4-20250514",
    "claude-sonnet-4-0",
    "claude-opus-4-0",
    "claude-3-7-sonnet-20250219",
    "claude-3-7-sonnet-latest",
];

/// Interleaved thinking configuration for Anthropic models
pub const INTERLEAVED_THINKING_BETA: &str = "interleaved-thinking-2025-05-14";
pub const INTERLEAVED_THINKING_TYPE_ENABLED: &str = "enabled";
