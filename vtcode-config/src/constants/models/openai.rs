pub const DEFAULT_MODEL: &str = "gpt-5";
pub const SUPPORTED_MODELS: &[&str] = &[
    "gpt-5",
    "gpt-5-codex",
    "gpt-5-mini",
    "gpt-5-nano",
    "gpt-5.2",
    "gpt-5.2-2025-12-11",
    "gpt-5.2-codex",
    "gpt-5.1",           // Enhanced version of GPT-5 with temperature support and streaming
    "gpt-5.1-codex",     // Enhanced version of GPT-5 Codex with temperature support and streaming
    "gpt-5.1-codex-max", // Enhanced version of GPT-5 Codex with temperature support and streaming
    "gpt-5.1-mini",      // Enhanced mini version with temperature support and streaming
    "codex-mini-latest",
    "gpt-oss-20b",
    "gpt-oss-120b",
];

/// Models that require the OpenAI Responses API
pub const RESPONSES_API_MODELS: &[&str] = &[
    GPT_5,
    GPT_5_CODEX,
    GPT_5_MINI,
    GPT_5_NANO,
    GPT_5_2,
    GPT_5_2_ALIAS,
    GPT_5_2_CODEX,
    GPT_5_1,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_MAX,
    GPT_5_1_MINI,
];

/// Models that support the OpenAI reasoning parameter payload
pub const REASONING_MODELS: &[&str] = &[
    GPT_5,
    GPT_5_CODEX,
    GPT_5_MINI,
    GPT_5_NANO,
    GPT_5_2,
    GPT_5_2_ALIAS,
    GPT_5_2_CODEX,
    GPT_5_1,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_MAX,
];

/// Models that do not expose structured tool calling on the OpenAI platform
pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

/// GPT-OSS models that use harmony tokenization
pub const HARMONY_MODELS: &[&str] = &[GPT_OSS_20B, GPT_OSS_120B];

// Convenience constants for commonly used models
pub const GPT_5: &str = "gpt-5";
pub const GPT_5_CODEX: &str = "gpt-5-codex";
pub const GPT_5_MINI: &str = "gpt-5-mini";
pub const GPT_5_NANO: &str = "gpt-5-nano";
pub const GPT_5_2: &str = "gpt-5.2";
pub const GPT_5_2_ALIAS: &str = "gpt-5.2-2025-12-11";
pub const GPT_5_2_CODEX: &str = "gpt-5.2-codex";
pub const GPT_5_1: &str = "gpt-5.1"; // Enhanced version with temperature support and streaming
pub const GPT_5_1_CODEX: &str = "gpt-5.1-codex"; // Enhanced version with temperature support and streaming
pub const GPT_5_1_CODEX_MAX: &str = "gpt-5.1-codex-max"; // Enhanced version with temperature support and streaming
pub const GPT_5_1_MINI: &str = "gpt-5.1-mini"; // Enhanced version with temperature support and streaming
pub const CODEX_MINI_LATEST: &str = "codex-mini-latest";
pub const GPT_OSS_20B: &str = "gpt-oss-20b";
pub const GPT_OSS_120B: &str = "gpt-oss-120b";
