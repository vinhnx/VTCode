pub const DEFAULT_MODEL: &str = "gpt-5.4";

pub const SUPPORTED_MODELS: &[&str] = &[
    // OpenAI GPT-5 Series (Latest flagship models)
    "gpt",
    "gpt-5",
    "gpt-5.4",
    "gpt-5.4-pro",
    "gpt-5-mini",
    "gpt-5-nano",
    "gpt-5.3-codex",
    "o3",
    "o4-mini",
];

// Convenience constants for commonly used models

// OpenAI GPT-5 Series
pub const GPT: &str = "gpt";
pub const GPT_5: &str = "gpt-5";
pub const GPT_5_4: &str = "gpt-5.4";
pub const GPT_5_4_PRO: &str = "gpt-5.4-pro";
pub const GPT_5_MINI: &str = "gpt-5-mini";
pub const GPT_5_NANO: &str = "gpt-5-nano";
pub const GPT_5_3_CODEX: &str = "gpt-5.3-codex";
pub const O3: &str = "o3";
pub const O4_MINI: &str = "o4-mini";
