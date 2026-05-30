pub const DEFAULT_MODEL: &str = QWEN3_8B;
pub const SUPPORTED_MODELS: &[&str] = &[
    // Reasoning models
    DEEPSEEK_R1_0528_QWEN3_8B,
    QWEN3_8B,
    OPENAI_GPT_OSS_20B,
    // General-purpose models
    META_LLAMA_31_8B_INSTRUCT,
    QWEN25_7B_INSTRUCT,
    GEMMA_3_12B_IT,
    PHI_31_MINI_4K_INSTRUCT,
];
pub const REASONING_MODELS: &[&str] = &[
    DEEPSEEK_R1_0528_QWEN3_8B,
    QWEN3_8B,
    OPENAI_GPT_OSS_20B,
];

// Reasoning models
pub const DEEPSEEK_R1_0528_QWEN3_8B: &str = "lmstudio-community/DeepSeek-R1-0528-Qwen3-8B";
pub const QWEN3_8B: &str = "lmstudio-community/Qwen3-8B";
pub const OPENAI_GPT_OSS_20B: &str = "lmstudio-community/openai-gpt-oss-20b";

// General-purpose models
pub const META_LLAMA_31_8B_INSTRUCT: &str = "lmstudio-community/meta-llama-3.1-8b-instruct";
pub const QWEN25_7B_INSTRUCT: &str = "lmstudio-community/qwen2.5-7b-instruct";
pub const GEMMA_3_12B_IT: &str = "lmstudio-community/gemma-3-12b-it";
pub const PHI_31_MINI_4K_INSTRUCT: &str = "lmstudio-community/phi-3.1-mini-4k-instruct";
