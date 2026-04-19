// OpenCode Zen models (pay-as-you-go gateway)
// https://opencode.ai/docs/zen/
pub const DEFAULT_MODEL: &str = GPT_5_4;

pub const GPT_5_4: &str = "gpt-5.4";
pub const GPT_5_4_PRO: &str = "gpt-5.4-pro";
pub const GPT_5_4_MINI: &str = "gpt-5.4-mini";
pub const GPT_5_4_NANO: &str = "gpt-5.4-nano";
pub const GPT_5_3_CODEX: &str = "gpt-5.3-codex";
pub const GPT_5_2: &str = "gpt-5.2";
pub const GPT_5_2_CODEX: &str = "gpt-5.2-codex";
pub const GPT_5_1: &str = "gpt-5.1";
pub const GPT_5_1_CODEX: &str = "gpt-5.1-codex";
pub const GPT_5_1_CODEX_MAX: &str = "gpt-5.1-codex-max";
pub const GPT_5: &str = "gpt-5";
pub const GPT_5_CODEX: &str = "gpt-5-codex";
pub const GPT_5_NANO: &str = "gpt-5-nano";

pub const CLAUDE_OPUS_4_7: &str = "claude-opus-4-7";
pub const CLAUDE_OPUS_4_6: &str = "claude-opus-4-6";
pub const CLAUDE_OPUS_4_5: &str = "claude-opus-4-5";
pub const CLAUDE_OPUS_4_1: &str = "claude-opus-4-1";
pub const CLAUDE_SONNET_4_6: &str = "claude-sonnet-4-6";
pub const CLAUDE_SONNET_4_5: &str = "claude-sonnet-4-5";
pub const CLAUDE_SONNET_4: &str = "claude-sonnet-4";
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
pub const CLAUDE_3_5_HAIKU: &str = "claude-3-5-haiku";

pub const QWEN3_6_PLUS: &str = "qwen3.6-plus";
pub const QWEN3_5_PLUS: &str = "qwen3.5-plus";
pub const MINIMAX_M2_5: &str = "minimax-m2.5";
pub const MINIMAX_M2_5_FREE: &str = "minimax-m2.5-free";
pub const GLM_5_1: &str = "glm-5.1";
pub const GLM_5: &str = "glm-5";
pub const KIMI_K2_5: &str = "kimi-k2.5";
pub const BIG_PICKLE: &str = "big-pickle";
pub const NEMOTRON_3_SUPER_FREE: &str = "nemotron-3-super-free";

pub const OPENAI_MODELS: &[&str] = &[
    GPT_5_4,
    GPT_5_4_PRO,
    GPT_5_4_MINI,
    GPT_5_4_NANO,
    GPT_5_3_CODEX,
    GPT_5_2,
    GPT_5_2_CODEX,
    GPT_5_1,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_MAX,
    GPT_5,
    GPT_5_CODEX,
    GPT_5_NANO,
];

pub const ANTHROPIC_MODELS: &[&str] = &[
    CLAUDE_OPUS_4_7,
    CLAUDE_OPUS_4_6,
    CLAUDE_OPUS_4_5,
    CLAUDE_OPUS_4_1,
    CLAUDE_SONNET_4_6,
    CLAUDE_SONNET_4_5,
    CLAUDE_SONNET_4,
    CLAUDE_HAIKU_4_5,
    CLAUDE_3_5_HAIKU,
];

pub const OPENAI_COMPATIBLE_MODELS: &[&str] = &[
    QWEN3_6_PLUS,
    QWEN3_5_PLUS,
    MINIMAX_M2_5,
    MINIMAX_M2_5_FREE,
    GLM_5_1,
    GLM_5,
    KIMI_K2_5,
    BIG_PICKLE,
    NEMOTRON_3_SUPER_FREE,
];

// Curated models VT Code currently exposes in config flows and ModelId metadata.
pub const CONFIGURED_MODELS: &[&str] =
    &[GPT_5_4, GPT_5_4_MINI, CLAUDE_SONNET_4_6, GLM_5_1, KIMI_K2_5];

pub const SUPPORTED_MODELS: &[&str] = &[
    GPT_5_4,
    GPT_5_4_PRO,
    GPT_5_4_MINI,
    GPT_5_4_NANO,
    GPT_5_3_CODEX,
    GPT_5_2,
    GPT_5_2_CODEX,
    GPT_5_1,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_MAX,
    GPT_5,
    GPT_5_CODEX,
    GPT_5_NANO,
    CLAUDE_OPUS_4_7,
    CLAUDE_OPUS_4_6,
    CLAUDE_OPUS_4_5,
    CLAUDE_OPUS_4_1,
    CLAUDE_SONNET_4_6,
    CLAUDE_SONNET_4_5,
    CLAUDE_SONNET_4,
    CLAUDE_HAIKU_4_5,
    CLAUDE_3_5_HAIKU,
    QWEN3_6_PLUS,
    QWEN3_5_PLUS,
    MINIMAX_M2_5,
    MINIMAX_M2_5_FREE,
    GLM_5_1,
    GLM_5,
    KIMI_K2_5,
    BIG_PICKLE,
    NEMOTRON_3_SUPER_FREE,
];
pub const REASONING_MODELS: &[&str] = &[];
