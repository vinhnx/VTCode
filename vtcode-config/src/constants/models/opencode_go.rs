// OpenCode Go models (low-cost subscription)
// https://opencode.ai/docs/go/
pub const DEFAULT_MODEL: &str = GLM_5_1;

pub const GLM_5_1: &str = "glm-5.1";
pub const MINIMAX_M2_7: &str = "minimax-m2.7";

pub const MESSAGES_API_MODELS: &[&str] = &[MINIMAX_M2_7];
pub const CHAT_COMPLETIONS_MODELS: &[&str] = &[GLM_5_1];

// Curated models VT Code currently exposes in config flows and ModelId metadata.
pub const CONFIGURED_MODELS: &[&str] = &[GLM_5_1, MINIMAX_M2_7];

pub const SUPPORTED_MODELS: &[&str] = &[GLM_5_1, MINIMAX_M2_7];
pub const REASONING_MODELS: &[&str] = &[];
