// Evolink models (OpenAI-compatible multi-model gateway)
// https://docs.evolink.ai/llms.txt
//
// Evolink is an aggregator that exposes many upstream models behind a single
// gateway. GPT/DeepSeek/Doubao models use the OpenAI Chat Completions format
// at `/v1/chat/completions`; Claude models use the Anthropic Messages format
// at `/v1/messages`. Bare model names collide with VT Code's first-class
// providers, so curated `ModelId` catalog entries are namespaced with an
// `evolink/` prefix; the provider strips that prefix before sending upstream.

// --- OpenAI-compatible models (Chat Completions API) ---
pub const GPT_5_2: &str = "gpt-5.2";
pub const GPT_5_5: &str = "gpt-5.5";
pub const DEEPSEEK_V4_PRO: &str = "deepseek-v4-pro";
pub const DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";
pub const DOUBAO_SEED_2_0_PRO: &str = "doubao-seed-2.0-pro";
pub const GEMINI_3_1_PRO: &str = "gemini-3.1-pro-preview";
pub const GEMINI_3_5_FLASH: &str = "gemini-3.5-flash";
pub const MINIMAX_M3: &str = "MiniMax-M3";

// --- Anthropic-compatible models (Messages API) ---
pub const CLAUDE_SONNET_4_6: &str = "claude-sonnet-4-6";
pub const CLAUDE_OPUS_4_8: &str = "claude-opus-4-8";
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5-20251001";

pub const DEFAULT_MODEL: &str = GPT_5_2;

/// Curated models VT Code exposes in config flows and `ModelId` metadata.
pub const SUPPORTED_MODELS: &[&str] = &[
    GPT_5_2,
    GPT_5_5,
    DEEPSEEK_V4_PRO,
    DEEPSEEK_V4_FLASH,
    DOUBAO_SEED_2_0_PRO,
    GEMINI_3_1_PRO,
    GEMINI_3_5_FLASH,
    MINIMAX_M3,
    CLAUDE_SONNET_4_6,
    CLAUDE_OPUS_4_8,
    CLAUDE_HAIKU_4_5,
];

/// Models that use the Anthropic Messages API instead of OpenAI Chat Completions.
pub const ANTHROPIC_FORMAT_MODELS: &[&str] = &[CLAUDE_SONNET_4_6, CLAUDE_OPUS_4_8, CLAUDE_HAIKU_4_5];

/// Models that emit reasoning traces / accept `reasoning_effort`.
pub const REASONING_MODELS: &[&str] = &[DEEPSEEK_V4_PRO, DEEPSEEK_V4_FLASH, DOUBAO_SEED_2_0_PRO];

/// Returns `true` if the model should use the Anthropic Messages API format.
pub fn is_anthropic_format(model: &str) -> bool {
    ANTHROPIC_FORMAT_MODELS.contains(&model)
}
