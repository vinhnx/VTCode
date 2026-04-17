// Claude 4.x series - Latest Anthropic models
pub const DEFAULT_MODEL: &str = "claude-opus-4-7";
pub const SUPPORTED_MODELS: &[&str] = &[
    "claude-opus-4-7",           // Premium flagship with adaptive thinking
    "claude-opus-4-6",           // Previous flagship retained for compatibility
    "claude-sonnet-4-6",         // Latest balanced flagship for complex agents and coding
    "claude-haiku-4-5",          // Fastest model with near-frontier intelligence
    "claude-haiku-4-5-20251001", // Haiku 4.5 versioned
    "claude-mythos-preview", // Invitation-only research preview for defensive security workloads
];

// Convenience constants for versioned models
pub const CLAUDE_HAIKU_4_5_20251001: &str = "claude-haiku-4-5-20251001";

// Convenience constants for alias models
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
pub const CLAUDE_SONNET_4_6: &str = "claude-sonnet-4-6";
pub const CLAUDE_OPUS_4_6: &str = "claude-opus-4-6";
pub const CLAUDE_OPUS_4_7: &str = "claude-opus-4-7";
pub const CLAUDE_MYTHOS_PREVIEW: &str = "claude-mythos-preview";

/// Models that accept the reasoning effort parameter or extended thinking
pub const REASONING_MODELS: &[&str] = &[
    CLAUDE_OPUS_4_6,
    CLAUDE_SONNET_4_6,
    CLAUDE_OPUS_4_7,
    CLAUDE_HAIKU_4_5,
    CLAUDE_HAIKU_4_5_20251001,
    CLAUDE_MYTHOS_PREVIEW,
];
