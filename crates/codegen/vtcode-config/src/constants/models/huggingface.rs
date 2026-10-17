pub const DEFAULT_MODEL: &str = OPENAI_GPT_OSS_120B;
pub const META_MUSE_SPARK_OPEN_BASETEN: &str = "meta-models/Muse-Spark-Open-816B-A42B:baseten";
pub const SUPPORTED_MODELS: &[&str] = &[
    OPENAI_GPT_OSS_120B,
    DEEPSEEK_R1,
    // Additional supported models
    OPENAI_GPT_OSS_20B,
    // Together inference provider models (incl. Z.AI Flash via Together)
    ZAI_GLM_5_3_FLASH_TOGETHER,
    ZAI_GLM_5_3_TOGETHER,
    // Moonshot inference provider models
    KIMI_K3_TOGETHER,
    // DeepInfra inference provider models
    // Additional Novita models
    MINIMAX_M3_NOVITA,
    // Baseten inference provider models (Meta open-weight)
    META_MUSE_SPARK_OPEN_BASETEN,
];

pub(crate) const OPENAI_GPT_OSS_120B: &str = "openai/gpt-oss-120b:huggingface";
const DEEPSEEK_R1: &str = "deepseek-ai/DeepSeek-R1";

// Additional supported models
pub(crate) const OPENAI_GPT_OSS_20B: &str = "openai/gpt-oss-20b:huggingface";

pub const ZAI_GLM_5_3_FLASH_TOGETHER: &str = "zai-org/GLM-5.3-Flash:together";
pub const ZAI_GLM_5_3_TOGETHER: &str = "zai-org/GLM-5.3:together";
pub const KIMI_K3_TOGETHER: &str = "moonshotai/Kimi-K3:together";

// DeepInfra inference provider models

// Additional Novita models

// MiniMax M3 via Novita
pub(crate) const MINIMAX_M3_NOVITA: &str = "MiniMaxAI/MiniMax-M3:novita";

pub const REASONING_MODELS: &[&str] = &[
    OPENAI_GPT_OSS_120B,
    DEEPSEEK_R1,
    // Additional reasoning models
    OPENAI_GPT_OSS_20B,
    ZAI_GLM_5_3_FLASH_TOGETHER,
    ZAI_GLM_5_3_TOGETHER,
    MINIMAX_M3_NOVITA,
    KIMI_K3_TOGETHER,
    META_MUSE_SPARK_OPEN_BASETEN,
];
