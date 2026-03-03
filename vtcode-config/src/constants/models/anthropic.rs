// Claude 4.6 series - Latest Anthropic models with extended thinking
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";
pub const SUPPORTED_MODELS: &[&str] = &[
    "claude-sonnet-4-6",          // Latest balanced flagship for complex agents and coding
    "claude-opus-4-6",            // Premium flagship with exceptional intelligence
    "claude-haiku-4-5",           // Fastest model with near-frontier intelligence
    "claude-haiku-4-5-20251001",  // Haiku 4.5 versioned
];

// Convenience constants for versioned models
pub const CLAUDE_HAIKU_4_5_20251001: &str = "claude-haiku-4-5-20251001";

// Convenience constants for alias models
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
pub const CLAUDE_SONNET_4_6: &str = "claude-sonnet-4-6";
pub const CLAUDE_OPUS_4_6: &str = "claude-opus-4-6";

/// Models that accept the reasoning effort parameter or extended thinking
pub const REASONING_MODELS: &[&str] = &[
    CLAUDE_SONNET_4_6,
    CLAUDE_OPUS_4_6,
    CLAUDE_HAIKU_4_5,
    CLAUDE_HAIKU_4_5_20251001,
];
