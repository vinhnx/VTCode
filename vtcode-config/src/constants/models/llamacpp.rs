pub const DEFAULT_MODEL: &str = GPT_OSS_20B;
pub const SUPPORTED_MODELS: &[&str] = &[
    QWEN36_27B,
    QWEN36_35B_A3B,
    GEMMA_4_26B_A4B,
    GEMMA_4_E4B,
    GPT_OSS_20B,
    STEP_3_5_FLASH,
];
pub const REASONING_MODELS: &[&str] = &[QWEN36_27B, QWEN36_35B_A3B, GPT_OSS_20B];

pub const QWEN36_27B: &str = "qwen3.6-27b";
pub const QWEN36_35B_A3B: &str = "qwen3.6-35b-a3b";
pub const GEMMA_4_26B_A4B: &str = "gemma-4-26b-a4b";
pub const GEMMA_4_E4B: &str = "gemma-4-e4b";
pub const GPT_OSS_20B: &str = "gpt-oss-20b";
pub const STEP_3_5_FLASH: &str = "step-3.5-flash";
