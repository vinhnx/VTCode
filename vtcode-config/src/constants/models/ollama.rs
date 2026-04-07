pub const DEFAULT_LOCAL_MODEL: &str = "gpt-oss:20b";
pub const DEFAULT_CLOUD_MODEL: &str = "gpt-oss:120b-cloud";
pub const DEFAULT_MODEL: &str = DEFAULT_LOCAL_MODEL;
pub const SUPPORTED_MODELS: &[&str] = &[
    DEFAULT_LOCAL_MODEL,
    QWEN3_1_7B,
    QWEN3_CODER_NEXT,
    DEFAULT_CLOUD_MODEL,
    GPT_OSS_20B_CLOUD,
    DEEPSEEK_V32_CLOUD,
    QWEN3_NEXT_80B_CLOUD,
    GLM_5_CLOUD,
    GLM_5_1_CLOUD,
    GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD,
    GEMINI_3_FLASH_PREVIEW_CLOUD,
    MINIMAX_M2_CLOUD,
    MINIMAX_M27_CLOUD,
    MINIMAX_M25_CLOUD,
    NEMOTRON_3_SUPER_CLOUD,
];

/// Models that emit structured reasoning traces when `think` is enabled
pub const REASONING_MODELS: &[&str] = &[
    GPT_OSS_20B,
    GPT_OSS_20B_CLOUD,
    GPT_OSS_120B_CLOUD,
    QWEN3_1_7B,
    DEEPSEEK_V32_CLOUD,
    QWEN3_NEXT_80B_CLOUD,
    GLM_5_CLOUD,
    GLM_5_1_CLOUD,
    GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD,
    GEMINI_3_FLASH_PREVIEW_CLOUD,
    MINIMAX_M2_CLOUD,
    MINIMAX_M27_CLOUD,
    MINIMAX_M25_CLOUD,
    NEMOTRON_3_SUPER_CLOUD,
];

/// Models that require an explicit reasoning effort level instead of boolean toggle
pub const REASONING_LEVEL_MODELS: &[&str] = &[
    GPT_OSS_20B,
    GPT_OSS_20B_CLOUD,
    GPT_OSS_120B_CLOUD,
    GLM_5_CLOUD,
    GLM_5_1_CLOUD,
    MINIMAX_M2_CLOUD,
    MINIMAX_M25_CLOUD,
    GEMINI_3_FLASH_PREVIEW_CLOUD,
    NEMOTRON_3_SUPER_CLOUD,
];

pub const GPT_OSS_20B: &str = DEFAULT_LOCAL_MODEL;
pub const GPT_OSS_20B_CLOUD: &str = "gpt-oss:20b-cloud";
pub const GPT_OSS_120B_CLOUD: &str = DEFAULT_CLOUD_MODEL;
pub const QWEN3_1_7B: &str = "qwen3:1.7b";
pub const QWEN3_CODER_NEXT: &str = "qwen3-coder-next:cloud";
pub const DEEPSEEK_V32_CLOUD: &str = "deepseek-v3.2:cloud";
pub const QWEN3_NEXT_80B_CLOUD: &str = "qwen3-next:80b-cloud";
pub const GLM_5_CLOUD: &str = "glm-5:cloud";
pub const GLM_5_1_CLOUD: &str = "glm-5.1:cloud";
pub const GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD: &str = "gemini-3.1-pro-preview:latest";
pub const GEMINI_3_FLASH_PREVIEW_CLOUD: &str = "gemini-3-flash-preview:cloud";
pub const MINIMAX_M2_CLOUD: &str = "minimax-m2:cloud";
pub const MINIMAX_M27_CLOUD: &str = "minimax-m2.7:cloud";
pub const MINIMAX_M25_CLOUD: &str = "minimax-m2.5:cloud";
pub const NEMOTRON_3_SUPER_CLOUD: &str = "nemotron-3-super:cloud";
