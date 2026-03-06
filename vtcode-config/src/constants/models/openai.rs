pub const DEFAULT_MODEL: &str = "gpt-5.4";
pub const SUPPORTED_MODELS: &[&str] = &[
    "gpt",
    "gpt-5",
    "gpt-5.2",
    "gpt-5.4",
    "gpt-5.4-pro",
    "gpt-5-mini",
    "gpt-5-nano",
    "gpt-5.3-codex", // GPT-5.3 Codex optimized for agentic coding with reasoning effort support
    "o3",
    "o4-mini",
    "gpt-oss-20b",
    "gpt-oss-120b",
];

/// Models that require the OpenAI Responses API
pub const RESPONSES_API_MODELS: &[&str] = &[
    GPT,
    GPT_5,
    GPT_5_2,
    GPT_5_4,
    GPT_5_4_PRO,
    GPT_5_MINI,
    GPT_5_NANO,
    GPT_5_3_CODEX,
    O3,
    O4_MINI,
];

/// Models that support the OpenAI reasoning parameter payload
pub const REASONING_MODELS: &[&str] = &[
    GPT,
    GPT_5,
    GPT_5_2,
    GPT_5_4,
    GPT_5_4_PRO,
    GPT_5_MINI,
    GPT_5_NANO,
    GPT_5_3_CODEX,
    O3,
    O4_MINI,
];

/// Models that do not expose structured tool calling on the OpenAI platform
pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

/// GPT-OSS models that use harmony tokenization
pub const HARMONY_MODELS: &[&str] = &[GPT_OSS_20B, GPT_OSS_120B];

// Convenience constants for commonly used models
pub const GPT: &str = "gpt";
pub const GPT_5: &str = "gpt-5";
pub const GPT_5_2: &str = "gpt-5.2";
pub const GPT_5_4: &str = "gpt-5.4";
pub const GPT_5_4_PRO: &str = "gpt-5.4-pro";
pub const GPT_5_MINI: &str = "gpt-5-mini";
pub const GPT_5_NANO: &str = "gpt-5-nano";
pub const GPT_5_3_CODEX: &str = "gpt-5.3-codex"; // GPT-5.3 Codex optimized for agentic coding
pub const O3: &str = "o3";
pub const O4_MINI: &str = "o4-mini";
pub const GPT_OSS_20B: &str = "gpt-oss-20b";
pub const GPT_OSS_120B: &str = "gpt-oss-120b";
