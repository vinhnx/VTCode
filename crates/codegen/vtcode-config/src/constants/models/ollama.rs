pub const DEFAULT_LOCAL_MODEL: &str = "gpt-oss:20b";
pub const DEFAULT_CLOUD_MODEL: &str = "gpt-oss:120b-cloud";
pub const DEFAULT_MODEL: &str = DEFAULT_LOCAL_MODEL;
pub const SUPPORTED_MODELS: &[&str] = &[
    DEFAULT_LOCAL_MODEL,
    DEFAULT_CLOUD_MODEL,
    GPT_OSS_20B_CLOUD,
    DEEPSEEK_V4_FLASH_CLOUD,
    DEEPSEEK_V4_PRO_CLOUD,
    GLM_5_1_CLOUD,
    GLM_5_2_CLOUD,
    GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD,
    MINIMAX_M27_CLOUD,
    MINIMAX_M3_CLOUD,
    KIMI_K2_6_CLOUD,
    KIMI_K2_7_CODE_CLOUD,
    GEMMA_4,
    LAGUNA_XS_2,
];

/// Models that emit structured reasoning traces when `think` is enabled
pub const REASONING_MODELS: &[&str] = &[
    GPT_OSS_20B,
    GPT_OSS_20B_CLOUD,
    GPT_OSS_120B_CLOUD,
    DEEPSEEK_V4_FLASH_CLOUD,
    DEEPSEEK_V4_PRO_CLOUD,
    GLM_5_1_CLOUD,
    GLM_5_2_CLOUD,
    GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD,
    MINIMAX_M27_CLOUD,
    MINIMAX_M3_CLOUD,
    LAGUNA_XS_2,
    KIMI_K2_7_CODE_CLOUD,
];

/// Models that require an explicit reasoning effort level instead of boolean toggle
pub const REASONING_LEVEL_MODELS: &[&str] = &[
    GPT_OSS_20B,
    GPT_OSS_20B_CLOUD,
    GPT_OSS_120B_CLOUD,
    GLM_5_1_CLOUD,
    GLM_5_2_CLOUD,
];

pub const GPT_OSS_20B: &str = DEFAULT_LOCAL_MODEL;
pub const GPT_OSS_20B_CLOUD: &str = "gpt-oss:20b-cloud";
pub const GPT_OSS_120B_CLOUD: &str = DEFAULT_CLOUD_MODEL;
pub const DEEPSEEK_V4_FLASH_CLOUD: &str = "deepseek-v4-flash:cloud";
pub const DEEPSEEK_V4_PRO_CLOUD: &str = "deepseek-v4-pro:cloud";
pub const GLM_5_1_CLOUD: &str = "glm-5.1:cloud";
pub const GLM_5_2_CLOUD: &str = "glm-5.2:cloud";
pub const GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD: &str = "gemini-3.1-pro-preview:latest";
pub const MINIMAX_M27_CLOUD: &str = "minimax-m2.7:cloud";
pub const MINIMAX_M3_CLOUD: &str = "minimax-m3:cloud";
pub const KIMI_K2_6_CLOUD: &str = "kimi-k2.6:cloud";
pub const KIMI_K2_7_CODE_CLOUD: &str = "kimi-k2.7-code:cloud";
pub const GEMMA_4: &str = "gemma4";
pub const LAGUNA_XS_2: &str = "laguna-xs.2";
