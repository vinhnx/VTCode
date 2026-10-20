const DEFAULT_LOCAL_MODEL: &str = "gpt-oss:20b";
pub const DEFAULT_CLOUD_MODEL: &str = "gpt-oss:120b-cloud";
pub const DEFAULT_MODEL: &str = DEFAULT_LOCAL_MODEL;
pub const SUPPORTED_MODELS: &[&str] = &[
    DEFAULT_LOCAL_MODEL,
    DEFAULT_CLOUD_MODEL,
    GPT_OSS_20B_CLOUD,
    GLM_5_3_CLOUD,
    GLM_5_3_FLASH_CLOUD,
    MISTRAL_LARGE_4_CLOUD,
    MUSE_SPARK_OPEN_CLOUD,
    GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD,
    MINIMAX_M3_CLOUD,
    MINIMAX_M3_1_FLASH_CLOUD,
    KIMI_K3_CLOUD,
    GEMMA_4,
    LAGUNA_XS_2,
    DEEPSEEK_V4_1_FLASH_CLOUD,
];

/// Models that emit structured reasoning traces when `think` is enabled
pub const REASONING_MODELS: &[&str] = &[
    GPT_OSS_20B,
    GPT_OSS_20B_CLOUD,
    GPT_OSS_120B_CLOUD,
    GLM_5_3_CLOUD,
    GLM_5_3_FLASH_CLOUD,
    MISTRAL_LARGE_4_CLOUD,
    MUSE_SPARK_OPEN_CLOUD,
    GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD,
    MINIMAX_M3_CLOUD,
    MINIMAX_M3_1_FLASH_CLOUD,
    LAGUNA_XS_2,
    KIMI_K3_CLOUD,
    DEEPSEEK_V4_1_FLASH_CLOUD,
];

/// Models that require an explicit reasoning effort level instead of boolean toggle
pub const REASONING_LEVEL_MODELS: &[&str] = &[
    GPT_OSS_20B,
    GPT_OSS_20B_CLOUD,
    GPT_OSS_120B_CLOUD,
    GLM_5_3_CLOUD,
    GLM_5_3_FLASH_CLOUD,
];

pub const GPT_OSS_20B: &str = DEFAULT_LOCAL_MODEL;
pub const GPT_OSS_20B_CLOUD: &str = "gpt-oss:20b-cloud";
pub const GPT_OSS_120B_CLOUD: &str = DEFAULT_CLOUD_MODEL;
pub const GLM_5_3_CLOUD: &str = "glm-5.3:cloud";
pub const GLM_5_3_FLASH_CLOUD: &str = "glm-5.3-flash:cloud";
pub const MISTRAL_LARGE_4_CLOUD: &str = "mistral-large-4:cloud";
pub const MUSE_SPARK_OPEN_CLOUD: &str = "muse-spark-open:cloud";
const GEMINI_3_1_PRO_PREVIEW_LATEST_CLOUD: &str = "gemini-3.1-pro-preview:latest";
pub const MINIMAX_M3_CLOUD: &str = "minimax-m3:cloud";
pub const MINIMAX_M3_1_FLASH_CLOUD: &str = "minimax-m3.1-flash:cloud";
pub const KIMI_K3_CLOUD: &str = "kimi-k3:cloud";
pub(crate) const GEMMA_4: &str = "gemma4";
pub(crate) const LAGUNA_XS_2: &str = "laguna-xs.2";
pub const DEEPSEEK_V4_1_FLASH_CLOUD: &str = "deepseek-v4.1-flash:cloud";
