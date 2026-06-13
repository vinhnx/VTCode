// Claude 4.x series - Latest Anthropic models
pub const DEFAULT_MODEL: &str = "claude-opus-4-8";
pub const SUPPORTED_MODELS: &[&str] = &[
    "claude-opus-4-8", // Latest premium flagship with adaptive thinking
    "claude-sonnet-4-6", // Latest balanced flagship for complex agents and coding
    "claude-haiku-4-5", // Fastest model with near-frontier intelligence
    "claude-haiku-4-5-20251001", // Haiku 4.5 versioned
];

// Convenience constants for versioned models
pub const CLAUDE_HAIKU_4_5_20251001: &str = "claude-haiku-4-5-20251001";

// Convenience constants for alias models
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
pub const CLAUDE_SONNET_4_6: &str = "claude-sonnet-4-6";
pub const CLAUDE_OPUS_4_8: &str = "claude-opus-4-8";

/// Models that accept the reasoning effort parameter or extended thinking
pub const REASONING_MODELS: &[&str] = &[
    CLAUDE_SONNET_4_6,
    CLAUDE_OPUS_4_8,
    CLAUDE_HAIKU_4_5,
    CLAUDE_HAIKU_4_5_20251001,
];

