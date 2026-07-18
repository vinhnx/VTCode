//! Provider extension methods that depend on vtcode-config model catalogs.
//!
//! The `Provider` enum and its core methods are defined in `vtcode-commons`.
//! This module adds vtcode-config-specific extension methods via the
//! [`ProviderModelSupport`] trait.

pub use vtcode_commons::provider::Provider;

use super::ModelId;
use std::str::FromStr;

/// Extension trait on `Provider` for model-specific capability queries.
///
/// These methods require vtcode-config model catalogs and constants,
/// so they cannot live in vtcode-commons with the core `Provider` type.
pub trait ProviderModelSupport {
    /// Determine if the provider supports configurable reasoning effort for the model.
    fn supports_reasoning_effort(&self, model: &str) -> bool;

    /// Determine if the provider supports the `service_tier` request parameter.
    fn supports_service_tier(&self, model: &str) -> bool;
}

impl ProviderModelSupport for Provider {
    fn supports_reasoning_effort(&self, model: &str) -> bool {
        use crate::constants::models;

        match self {
            Provider::Gemini => models::google::REASONING_MODELS.contains(&model),
            Provider::OpenAI => models::openai::REASONING_MODELS.contains(&model),
            Provider::Anthropic => models::anthropic::REASONING_MODELS.contains(&model),
            Provider::Copilot => false,
            Provider::DeepSeek => model == models::deepseek::DEEPSEEK_V4_PRO || model == "deepseek-reasoner",
            Provider::OpenRouter => {
                if let Ok(model_id) = ModelId::from_str(model) {
                    if let Some(meta) = crate::models::openrouter_generated::metadata_for(model_id) {
                        return meta.reasoning;
                    }
                    return false;
                }
                models::openrouter::REASONING_MODELS.contains(&model)
            }
            Provider::Ollama => models::ollama::REASONING_LEVEL_MODELS.contains(&model),
            Provider::LmStudio => models::lmstudio::REASONING_MODELS.contains(&model),
            Provider::LlamaCpp => models::llamacpp::REASONING_MODELS.contains(&model),
            Provider::Moonshot => models::moonshot::REASONING_MODELS.contains(&model),
            Provider::ZAI => models::zai::REASONING_MODELS.contains(&model),
            Provider::Minimax => models::minimax::SUPPORTED_MODELS.contains(&model),
            Provider::MiMo => models::mimo::SUPPORTED_MODELS.contains(&model),
            Provider::Mistral => models::mistral::SUPPORTED_MODELS.contains(&model),
            Provider::HuggingFace => models::huggingface::REASONING_MODELS.contains(&model),
            Provider::OpenCodeZen => {
                if models::opencode_zen::OPENAI_MODELS.contains(&model) {
                    Provider::OpenAI.supports_reasoning_effort(model)
                } else if models::opencode_zen::ANTHROPIC_MODELS.contains(&model) {
                    Provider::Anthropic.supports_reasoning_effort(model)
                } else {
                    false
                }
            }
            Provider::OpenCodeGo => false,
            Provider::Qwen => models::qwen::REASONING_MODELS.contains(&model),
            Provider::StepFun => models::stepfun::REASONING_MODELS.contains(&model),
            Provider::Evolink => models::evolink::REASONING_MODELS.contains(&model),
            Provider::Poolside => false,
        }
    }

    fn supports_service_tier(&self, model: &str) -> bool {
        use crate::constants::models;

        match self {
            Provider::OpenAI => models::openai::SERVICE_TIER_MODELS.contains(&model),
            _ => false,
        }
    }
}
