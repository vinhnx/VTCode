pub const DEFAULT_MODEL: &str = OPENAI_GPT_OSS_120B;
pub const SUPPORTED_MODELS: &[&str] = &[
    // Recommended conversational LLMs from HF docs
    GOOGLE_GEMMA_2_2B_IT,
    QWEN3_CODER_480B_A35B_INSTRUCT,
    OPENAI_GPT_OSS_120B,
    QWEN3_4B_THINKING_2507,
    QWEN25_7B_INSTRUCT_1M,
    QWEN25_CODER_32B_INSTRUCT,
    DEEPSEEK_R1,
    // Additional supported models
    DEEPSEEK_V32,
    OPENAI_GPT_OSS_20B,
    // Novita inference provider models
    MINIMAX_M2_5_NOVITA,
    DEEPSEEK_V32_NOVITA,
    XIAOMI_MIMO_V2_FLASH_NOVITA,
    QWEN3_CODER_NEXT_NOVITA,
    ZAI_GLM_5_NOVITA,
    ZAI_GLM_5_1_ZAI_ORG,
    // Together inference provider models
    QWEN3_5_397B_A17B_TOGETHER,
    STEP_3_5_FLASH,
];

// Recommended conversational LLMs
pub const GOOGLE_GEMMA_2_2B_IT: &str = "google/gemma-2-2b-it";
pub const QWEN3_CODER_480B_A35B_INSTRUCT: &str = "Qwen/Qwen3-Coder-480B-A35B-Instruct";
pub const OPENAI_GPT_OSS_120B: &str = "openai/gpt-oss-120b:huggingface";
pub const QWEN3_4B_THINKING_2507: &str = "Qwen/Qwen3-4B-Thinking-2507";
pub const QWEN25_7B_INSTRUCT_1M: &str = "Qwen/Qwen2.5-7B-Instruct-1M";
pub const QWEN25_CODER_32B_INSTRUCT: &str = "Qwen/Qwen2.5-Coder-32B-Instruct";
pub const DEEPSEEK_R1: &str = "deepseek-ai/DeepSeek-R1";
pub const STEP_3_5_FLASH_BASE: &str = "stepfun-ai/Step-3.5-Flash";
pub const STEP_3_5_FLASH_PROVIDER: &str = "featherless-ai";
pub const STEP_3_5_FLASH: &str = "stepfun-ai/Step-3.5-Flash:featherless-ai";
pub const STEP_3_5_FLASH_LEGACY_FASTEST: &str = "stepfun-ai/Step-3.5-Flash:fastest";

// Additional supported models
pub const DEEPSEEK_V32: &str = "deepseek-ai/DeepSeek-V3.2:huggingface";
pub const OPENAI_GPT_OSS_20B: &str = "openai/gpt-oss-20b:huggingface";

pub const MINIMAX_M2_5_NOVITA: &str = "MiniMaxAI/MiniMax-M2.5:novita";
pub const DEEPSEEK_V32_NOVITA: &str = "deepseek-ai/DeepSeek-V3.2:novita";
pub const XIAOMI_MIMO_V2_FLASH_NOVITA: &str = "XiaomiMiMo/MiMo-V2-Flash:novita";
pub const QWEN3_CODER_NEXT_NOVITA: &str = "Qwen/Qwen3-Coder-Next:novita";
pub const ZAI_GLM_5_NOVITA: &str = "zai-org/GLM-5:novita";
pub const ZAI_GLM_5_1_ZAI_ORG: &str = "zai-org/GLM-5.1:zai-org";
pub const QWEN3_5_397B_A17B_TOGETHER: &str = "Qwen/Qwen3.5-397B-A17B:together";

pub const REASONING_MODELS: &[&str] = &[
    // All recommended conversational LLMs support reasoning
    QWEN3_CODER_480B_A35B_INSTRUCT,
    OPENAI_GPT_OSS_120B,
    QWEN3_4B_THINKING_2507,
    DEEPSEEK_R1,
    // Additional reasoning models
    DEEPSEEK_V32,
    OPENAI_GPT_OSS_20B,
    DEEPSEEK_V32_NOVITA,
    MINIMAX_M2_5_NOVITA,
    XIAOMI_MIMO_V2_FLASH_NOVITA,
    QWEN3_CODER_NEXT_NOVITA,
    ZAI_GLM_5_1_ZAI_ORG,
    QWEN3_5_397B_A17B_TOGETHER,
    STEP_3_5_FLASH,
];
