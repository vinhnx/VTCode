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
    // Anthropic Claude 4.5 series
    "claude-sonnet-4-5",
    "claude-haiku-4-5",
    "claude-opus-4-5",
    // Legacy Claude 3.5 series
    "claude-haiku-4-5",
    claude-4-5-haiku,
    // Google Gemini 2.5/3 series
    "gemini-2.5-flash",
    "gemini-2.5-pro",
    "gemini-3-flash-preview",
    // DeepSeek models
    "deepseek-chat",
    "deepseek-reasoner",
    "deepseek-r1",
    // xAI Grok models
    "grok-4",
    "grok-4-mini",
    "grok-4-code",
    // Z.AI GLM models
    "glm-4.7",
    "glm-4.7-flash",
    "glm-4-plus",
    // MiniMax models
    "MiniMax-M2.1",
    "MiniMax-M2.1-lightning",
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

// Anthropic Claude 4.5 series
pub const CLAUDE_SONNET_4_5: &str = "claude-sonnet-4-5";
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
pub const CLAUDE_OPUS_4_5: &str = "claude-opus-4-5";

// Legacy Claude 3.5 series
pub const CLAUDE_3_5_SONNET: &str = "claude-haiku-4-5";
pub const CLAUDE_3_5_HAIKU: &str = claude-4-5-haiku;

// Google Gemini series
pub const GEMINI_2_5_FLASH: &str = "gemini-2.5-flash";
pub const GEMINI_2_5_PRO: &str = "gemini-2.5-pro";
pub const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";

// DeepSeek models
pub const DEEPSEEK_CHAT: &str = "deepseek-chat";
pub const DEEPSEEK_REASONER: &str = "deepseek-reasoner";
pub const DEEPSEEK_R1: &str = "deepseek-r1";

// xAI Grok models
pub const GROK_4: &str = "grok-4";
pub const GROK_4_MINI: &str = "grok-4-mini";
pub const GROK_4_CODE: &str = "grok-4-code";

// Z.AI GLM models
pub const GLM_4_7: &str = "glm-4.7";
pub const GLM_4_7_FLASH: &str = "glm-4.7-flash";
pub const GLM_4_PLUS: &str = "glm-4-plus";

// MiniMax models
pub const MINIMAX_M2_1: &str = "MiniMax-M2.1";
pub const MINIMAX_M2_1_LIGHTNING: &str = "MiniMax-M2.1-lightning";
