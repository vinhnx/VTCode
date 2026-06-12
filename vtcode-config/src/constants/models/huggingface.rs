pub const DEFAULT_MODEL: &str = OPENAI_GPT_OSS_120B;
pub const SUPPORTED_MODELS: &[&str] = &[
    OPENAI_GPT_OSS_120B,
    DEEPSEEK_R1,
    // Additional supported models
    OPENAI_GPT_OSS_20B,
    // Novita inference provider models
    ZAI_GLM_5_1_ZAI_ORG,
    // Moonshot inference provider models
    KIMI_K2_6_NOVITA,
    // Together inference provider models
    DEEPSEEK_V4_PRO_TOGETHER,
    STEP_3_5_FLASH,
    // DeepInfra inference provider models
    ZAI_GLM_5_1_DEEPINFRA,
    // Additional Novita models
    MINIMAX_M2_7_NOVITA,
    DEEPSEEK_V4_PRO_NOVITA,
    MINIMAX_M3_NOVITA,
];

pub const OPENAI_GPT_OSS_120B: &str = "openai/gpt-oss-120b:huggingface";
pub const DEEPSEEK_R1: &str = "deepseek-ai/DeepSeek-R1";
pub const STEP_3_5_FLASH_BASE: &str = "stepfun-ai/Step-3.5-Flash";
pub const STEP_3_5_FLASH_PROVIDER: &str = "featherless-ai";
pub const STEP_3_5_FLASH: &str = "stepfun-ai/Step-3.5-Flash:featherless-ai";
pub const STEP_3_5_FLASH_LEGACY_FASTEST: &str = "stepfun-ai/Step-3.5-Flash:fastest";

// Additional supported models
pub const OPENAI_GPT_OSS_20B: &str = "openai/gpt-oss-20b:huggingface";

pub const ZAI_GLM_5_1_ZAI_ORG: &str = "zai-org/GLM-5.1:zai-org";
pub const KIMI_K2_6_NOVITA: &str = "moonshotai/Kimi-K2.6:novita";

// DeepSeek V4 models via HF router
pub const DEEPSEEK_V4_FLASH_NOVITA: &str = "deepseek-ai/DeepSeek-V4-Flash:novita";
pub const DEEPSEEK_V4_PRO_TOGETHER: &str = "deepseek-ai/DeepSeek-V4-Pro:together";
pub const DEEPSEEK_V4_PRO_NOVITA: &str = "deepseek-ai/DeepSeek-V4-Pro:novita";

// DeepInfra inference provider models
pub const ZAI_GLM_5_1_DEEPINFRA: &str = "zai-org/GLM-5.1:deepinfra";

// Additional Novita models
pub const MINIMAX_M2_7_NOVITA: &str = "MiniMaxAI/MiniMax-M2.7:novita";

// MiniMax M3 via Novita
pub const MINIMAX_M3_NOVITA: &str = "MiniMaxAI/MiniMax-M3:novita";

pub const REASONING_MODELS: &[&str] = &[
    OPENAI_GPT_OSS_120B,
    DEEPSEEK_R1,
    // Additional reasoning models
    OPENAI_GPT_OSS_20B,
    ZAI_GLM_5_1_ZAI_ORG,
    ZAI_GLM_5_1_DEEPINFRA,
    MINIMAX_M2_7_NOVITA,
    MINIMAX_M3_NOVITA,
    DEEPSEEK_V4_PRO_TOGETHER,
    DEEPSEEK_V4_PRO_NOVITA,
    DEEPSEEK_V4_FLASH_NOVITA,
    STEP_3_5_FLASH,
];
