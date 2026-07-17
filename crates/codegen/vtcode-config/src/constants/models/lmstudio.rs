pub const DEFAULT_MODEL: &str = OPENAI_GPT_OSS_20B;
pub const SUPPORTED_MODELS: &[&str] = &[
    OPENAI_GPT_OSS_20B,
    META_LLAMA_31_8B_INSTRUCT,
    GEMMA_3_12B_IT,
    PHI_31_MINI_4K_INSTRUCT,
];
pub const REASONING_MODELS: &[&str] = &[OPENAI_GPT_OSS_20B];

pub const OPENAI_GPT_OSS_20B: &str = "lmstudio-community/openai-gpt-oss-20b";

pub const META_LLAMA_31_8B_INSTRUCT: &str = "lmstudio-community/meta-llama-3.1-8b-instruct";
pub const GEMMA_3_12B_IT: &str = "lmstudio-community/gemma-3-12b-it";
pub const PHI_31_MINI_4K_INSTRUCT: &str = "lmstudio-community/phi-3.1-mini-4k-instruct";
