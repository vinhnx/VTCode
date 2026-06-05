// Evolink models (OpenAI-compatible multi-model gateway)
// https://docs.evolink.ai/llms.txt
// Chat completions base URL: https://direct.evolink.ai/v1
//
// Evolink is an aggregator that exposes many upstream models behind a single
// OpenAI-compatible endpoint using bare model names (e.g. `gpt-5.2`). Those bare
// names collide with VT Code's first-class providers, so the curated `ModelId`
// catalog entries are namespaced with an `evolink/` prefix; the provider strips
// that prefix before sending the request upstream.

pub const GPT_5_2: &str = "gpt-5.2";
pub const GPT_5_5: &str = "gpt-5.5";
pub const DEEPSEEK_V4_PRO: &str = "deepseek-v4-pro";
pub const DOUBAO_SEED_2_0_PRO: &str = "doubao-seed-2.0-pro";

pub const DEFAULT_MODEL: &str = GPT_5_2;

/// Curated models VT Code exposes in config flows and `ModelId` metadata.
pub const SUPPORTED_MODELS: &[&str] = &[GPT_5_2, GPT_5_5, DEEPSEEK_V4_PRO, DOUBAO_SEED_2_0_PRO];

/// Models that emit reasoning traces / accept `reasoning_effort`.
pub const REASONING_MODELS: &[&str] = &[DEEPSEEK_V4_PRO, DOUBAO_SEED_2_0_PRO];
