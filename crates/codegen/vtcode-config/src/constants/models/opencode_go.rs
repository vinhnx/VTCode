// OpenCode Go models (low-cost subscription)
// https://opencode.ai/docs/go/
pub const DEFAULT_MODEL: &str = GLM_5_1;

pub const GLM_5_2: &str = "glm-5.2";
pub const GLM_5_1: &str = "glm-5.1";
pub const KIMI_K2_7_CODE: &str = "kimi-k2.7-code";
pub const KIMI_K2_6: &str = "kimi-k2.6";
pub const MIMO_V2_5: &str = "mimo-v2.5";
pub const MIMO_V2_5_PRO: &str = "mimo-v2.5-pro";
pub const MINIMAX_M3: &str = "minimax-m3";
pub const MINIMAX_M2_7: &str = "minimax-m2.7";
pub const QWEN_3_7_MAX: &str = "qwen3.7-max";
pub const QWEN_3_7_PLUS: &str = "qwen3.7-plus";
pub const QWEN_3_6_PLUS: &str = "qwen3.6-plus";
pub const DEEPSEEK_V4_PRO: &str = "deepseek-v4-pro";
pub const DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";

pub const MESSAGES_API_MODELS: &[&str] = &[
    MINIMAX_M3,
    MINIMAX_M2_7,
    QWEN_3_7_MAX,
    QWEN_3_7_PLUS,
    QWEN_3_6_PLUS,
];
pub const CHAT_COMPLETIONS_MODELS: &[&str] = &[
    GLM_5_2,
    GLM_5_1,
    KIMI_K2_7_CODE,
    KIMI_K2_6,
    MIMO_V2_5,
    MIMO_V2_5_PRO,
    DEEPSEEK_V4_PRO,
    DEEPSEEK_V4_FLASH,
];

// Curated models VT Code currently exposes in config flows and ModelId metadata.
pub const CONFIGURED_MODELS: &[&str] = &[
    GLM_5_2,
    GLM_5_1,
    KIMI_K2_7_CODE,
    KIMI_K2_6,
    MIMO_V2_5,
    MIMO_V2_5_PRO,
    MINIMAX_M3,
    MINIMAX_M2_7,
    QWEN_3_7_MAX,
    QWEN_3_7_PLUS,
    QWEN_3_6_PLUS,
    DEEPSEEK_V4_PRO,
    DEEPSEEK_V4_FLASH,
];

pub const SUPPORTED_MODELS: &[&str] = CONFIGURED_MODELS;
pub const REASONING_MODELS: &[&str] = &[];
