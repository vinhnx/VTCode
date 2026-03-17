pub const DEFAULT_MODEL: &str = "gpt-5.4";
pub const SUPPORTED_MODELS: &[&str] = &[
    GPT,
    "gpt-5.4",
    "gpt-5.4-pro",
    "gpt-5.4-nano",
    "gpt-5.4-mini",
    "gpt-5.3-codex", // GPT-5.3 Codex optimized for agentic coding with xhigh reasoning support
    "gpt-5.2-codex", // GPT-5.2 Codex optimized for agentic coding with xhigh reasoning support
    "gpt-5.1-codex", // GPT-5.1 Codex optimized for agentic coding
    "gpt-5.1-codex-max", // GPT-5.1 Codex Max higher-compute coding variant
    "gpt-5-codex",   // GPT-5 Codex optimized for agentic coding
    "gpt-5.2",
    "gpt-5",
    "gpt-5-mini",
    "gpt-5-nano",
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
    GPT_5_3_CODEX,
    GPT_5_2_CODEX,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_MAX,
    GPT_5_CODEX,
    GPT_5_MINI,
    GPT_5_NANO,
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
    GPT_5_3_CODEX,
    GPT_5_2_CODEX,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_MAX,
    GPT_5_CODEX,
    GPT_5_MINI,
    GPT_5_NANO,
    O3,
    O4_MINI,
];

/// Models that support the native OpenAI `service_tier` request parameter.
pub const SERVICE_TIER_MODELS: &[&str] = RESPONSES_API_MODELS;

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
pub const GPT_5_4_NANO: &str = "gpt-5.4-nano";
pub const GPT_5_4_MINI: &str = "gpt-5.4-mini";
pub const GPT_5_3_CODEX: &str = "gpt-5.3-codex"; // GPT-5.3 Codex optimized for agentic coding
pub const GPT_5_2_CODEX: &str = "gpt-5.2-codex"; // GPT-5.2 Codex optimized for agentic coding
pub const GPT_5_1_CODEX: &str = "gpt-5.1-codex"; // GPT-5.1 Codex optimized for agentic coding
pub const GPT_5_1_CODEX_MAX: &str = "gpt-5.1-codex-max"; // GPT-5.1 Codex Max optimized for longer-running coding tasks
pub const GPT_5_CODEX: &str = "gpt-5-codex"; // GPT-5 Codex optimized for agentic coding
pub const GPT_5_MINI: &str = "gpt-5-mini";
pub const GPT_5_NANO: &str = "gpt-5-nano";
pub const O3: &str = "o3";
pub const O4_MINI: &str = "o4-mini";
pub const GPT_OSS_20B: &str = "gpt-oss-20b";
pub const GPT_OSS_120B: &str = "gpt-oss-120b";
