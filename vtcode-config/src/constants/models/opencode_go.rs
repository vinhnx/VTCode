// OpenCode Go models (low-cost subscription)
// https://opencode.ai/docs/go/
pub const DEFAULT_MODEL: &str = KIMI_K2_5;

pub const GLM_5_1: &str = "glm-5.1";
pub const GLM_5: &str = "glm-5";
pub const KIMI_K2_5: &str = "kimi-k2.5";
pub const MIMO_V2_PRO: &str = "mimo-v2-pro";
pub const MIMO_V2_OMNI: &str = "mimo-v2-omni";
pub const MINIMAX_M2_7: &str = "minimax-m2.7";
pub const MINIMAX_M2_5: &str = "minimax-m2.5";
pub const QWEN3_6_PLUS: &str = "qwen3.6-plus";
pub const QWEN3_5_PLUS: &str = "qwen3.5-plus";

pub const MESSAGES_API_MODELS: &[&str] = &[MINIMAX_M2_7, MINIMAX_M2_5];
pub const CHAT_COMPLETIONS_MODELS: &[&str] = &[
    GLM_5_1,
    GLM_5,
    KIMI_K2_5,
    MIMO_V2_PRO,
    MIMO_V2_OMNI,
    QWEN3_6_PLUS,
    QWEN3_5_PLUS,
];

// Curated models VT Code currently exposes in config flows and ModelId metadata.
pub const CONFIGURED_MODELS: &[&str] = &[GLM_5_1, KIMI_K2_5, MINIMAX_M2_5, MINIMAX_M2_7];

pub const SUPPORTED_MODELS: &[&str] = &[
    GLM_5_1,
    GLM_5,
    KIMI_K2_5,
    MIMO_V2_PRO,
    MIMO_V2_OMNI,
    MINIMAX_M2_7,
    MINIMAX_M2_5,
    QWEN3_6_PLUS,
    QWEN3_5_PLUS,
];
pub const REASONING_MODELS: &[&str] = &[];
