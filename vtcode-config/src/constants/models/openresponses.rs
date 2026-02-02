pub const DEFAULT_MODEL: &str = "gpt-5";

pub const SUPPORTED_MODELS: &[&str] = &[
    // OpenAI GPT-5 Series (Latest flagship models)
    "gpt-5",
    "gpt-5.1",
    "gpt-5.2",
    "gpt-5-mini",
    "gpt-5-nano",
    "gpt-5-codex",
    "gpt-5.1-codex",
    "gpt-5.1-codex-max",
    "gpt-5.2-codex",
    // OpenAI o-series (Reasoning models)
    "o3",
    "o4-mini",
    // Legacy GPT-4 series (still supported)
    "gpt-4o",
    "gpt-4o-mini",
];

// Convenience constants for commonly used models

// OpenAI GPT-5 Series
pub const GPT_5: &str = "gpt-5";
pub const GPT_5_1: &str = "gpt-5.1";
pub const GPT_5_2: &str = "gpt-5.2";
pub const GPT_5_MINI: &str = "gpt-5-mini";
pub const GPT_5_NANO: &str = "gpt-5-nano";
pub const GPT_5_CODEX: &str = "gpt-5-codex";
pub const GPT_5_1_CODEX: &str = "gpt-5.1-codex";
pub const GPT_5_1_CODEX_MAX: &str = "gpt-5.1-codex-max";
pub const GPT_5_2_CODEX: &str = "gpt-5.2-codex";

// OpenAI o-series
pub const O3: &str = "o3";
pub const O4_MINI: &str = "o4-mini";

// Legacy GPT-4 series
pub const GPT_4O: &str = "gpt-4o";
pub const GPT_4O_MINI: &str = "gpt-4o-mini";

