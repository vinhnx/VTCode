pub const DEFAULT_MODEL: &str = OPENAI_GPT_OSS_120B;
pub const SUPPORTED_MODELS: &[&str] = &[
    // Recommended conversational LLMs from HF docs
    GOOGLE_GEMMA_2_2B_IT,
    QWEN3_CODER_480B_A35B_INSTRUCT,
    OPENAI_GPT_OSS_120B,
    ZAI_GLM_45,
    QWEN3_4B_THINKING_2507,
    QWEN25_7B_INSTRUCT_1M,
    QWEN25_CODER_32B_INSTRUCT,
    DEEPSEEK_R1,
    // Additional supported models
    DEEPSEEK_V32,
    OPENAI_GPT_OSS_20B,
    ZAI_GLM_46,
    ZAI_GLM_47,
    ZAI_GLM_47_NOVITA,
    ZAI_GLM_47_FLASH_NOVITA,
    MOONSHOT_KIMI_K2_THINKING,
    MOONSHOT_KIMI_K2_5_NOVITA,
    // Recommended VLM
    ZAI_GLM_45V,
    // Novita inference provider models
    MINIMAX_M2_1_NOVITA,
    DEEPSEEK_V32_NOVITA,
    XIAOMI_MIMO_V2_FLASH_NOVITA,
];

// Recommended conversational LLMs
pub const GOOGLE_GEMMA_2_2B_IT: &str = "google/gemma-2-2b-it";
pub const QWEN3_CODER_480B_A35B_INSTRUCT: &str = "Qwen/Qwen3-Coder-480B-A35B-Instruct";
pub const OPENAI_GPT_OSS_120B: &str = "openai/gpt-oss-120b";
pub const ZAI_GLM_45: &str = "zai-org/GLM-4.5:zai-org";
pub const QWEN3_4B_THINKING_2507: &str = "Qwen/Qwen3-4B-Thinking-2507";
pub const QWEN25_7B_INSTRUCT_1M: &str = "Qwen/Qwen2.5-7B-Instruct-1M";
pub const QWEN25_CODER_32B_INSTRUCT: &str = "Qwen/Qwen2.5-Coder-32B-Instruct";
pub const DEEPSEEK_R1: &str = "deepseek-ai/DeepSeek-R1";

// Additional supported models
pub const DEEPSEEK_V32: &str = "deepseek-ai/DeepSeek-V3.2";
pub const OPENAI_GPT_OSS_20B: &str = "openai/gpt-oss-20b";
pub const ZAI_GLM_46: &str = "zai-org/GLM-4.6:zai-org";
pub const ZAI_GLM_47: &str = "zai-org/GLM-4.7:zai-org";
pub const ZAI_GLM_47_NOVITA: &str = "zai-org/GLM-4.7:novita";
pub const ZAI_GLM_47_FLASH_NOVITA: &str = "zai-org/GLM-4.7-Flash:novita";
pub const MOONSHOT_KIMI_K2_THINKING: &str = "moonshotai/Kimi-K2-Thinking";
pub const MOONSHOT_KIMI_K2_5_NOVITA: &str = "moonshotai/Kimi-K2.5:novita";

// Recommended VLM
pub const ZAI_GLM_45V: &str = "zai-org/GLM-4.5V:zai-org";
pub const MINIMAX_M2_1_NOVITA: &str = "MiniMaxAI/MiniMax-M2.1:novita";
pub const DEEPSEEK_V32_NOVITA: &str = "deepseek-ai/DeepSeek-V3.2:novita";
pub const XIAOMI_MIMO_V2_FLASH_NOVITA: &str = "XiaomiMiMo/MiMo-V2-Flash:novita";

pub const REASONING_MODELS: &[&str] = &[
    // All recommended models support reasoning
    QWEN3_CODER_480B_A35B_INSTRUCT,
    OPENAI_GPT_OSS_120B,
    ZAI_GLM_45,
    QWEN3_4B_THINKING_2507,
    DEEPSEEK_R1,
    // Additional reasoning models
    DEEPSEEK_V32,
    OPENAI_GPT_OSS_20B,
    ZAI_GLM_46,
    ZAI_GLM_47,
    ZAI_GLM_47_NOVITA,
    ZAI_GLM_47_FLASH_NOVITA,
    MOONSHOT_KIMI_K2_THINKING,
    MOONSHOT_KIMI_K2_5_NOVITA,
    ZAI_GLM_45V,
    DEEPSEEK_V32_NOVITA,
    MINIMAX_M2_1_NOVITA,
    XIAOMI_MIMO_V2_FLASH_NOVITA,
];
