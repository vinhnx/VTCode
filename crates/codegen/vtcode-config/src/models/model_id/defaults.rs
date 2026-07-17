use crate::models::Provider;

use super::ModelId;

impl ModelId {
    /// Get recommended fallback models in order of preference
    pub fn fallback_models() -> Vec<ModelId> {
        vec![
            ModelId::ClaudeSonnet5,
            ModelId::Gemini35Flash,
            ModelId::GPT54,
            ModelId::GPT55,
            ModelId::OpenAIGptOss20b,
            ModelId::ClaudeOpus48,
            ModelId::ClaudeSonnet46,
            ModelId::DeepSeekV4Pro,
            ModelId::ZaiGlm51,
        ]
    }

    /// Get the default model for general use
    pub fn default_model() -> Self {
        ModelId::ClaudeSonnet5
    }

    /// Get the default orchestrator model (more capable)
    pub fn default_orchestrator() -> Self {
        ModelId::ClaudeSonnet5
    }

    /// Get provider-specific defaults for orchestrator
    pub fn default_orchestrator_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini31ProPreview,
            Provider::OpenAI => ModelId::GPT54,
            Provider::Anthropic => ModelId::ClaudeOpus48,
            Provider::Copilot => ModelId::CopilotAuto,
            Provider::Minimax => ModelId::MinimaxM3,
            Provider::MiMo => ModelId::MiMoV25Pro,
            Provider::Mistral => ModelId::MistralLarge3,
            Provider::DeepSeek => ModelId::DeepSeekV4Pro,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::MoonshotKimiK3,
            Provider::OpenRouter => ModelId::OpenRouterXiaomiMimoV25Pro,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::GPT54,
            Provider::LlamaCpp => ModelId::LlamaCppGptOss20b,
            Provider::ZAI => ModelId::ZaiGlm51,
            Provider::OpenCodeZen => ModelId::OpenCodeZenGPT54,
            Provider::OpenCodeGo => ModelId::OpenCodeGoMinimaxM27,
            Provider::Qwen => ModelId::QwenDeepSeekV4Flash,
            Provider::StepFun => ModelId::StepFun37Flash,
            Provider::Evolink => ModelId::EvolinkGpt52,
            Provider::Poolside => ModelId::PoolsideLagunaM1,
        }
    }

    /// Get provider-specific defaults for single agent
    pub fn default_single_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini35Flash,
            Provider::OpenAI => ModelId::GPT54,
            Provider::Anthropic => ModelId::ClaudeSonnet5,
            Provider::Copilot => ModelId::CopilotAuto,
            Provider::Minimax => ModelId::MinimaxM3,
            Provider::MiMo => ModelId::MiMoV25Pro,
            Provider::Mistral => ModelId::MistralLarge3,
            Provider::DeepSeek => ModelId::DeepSeekV4Pro,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::MoonshotKimiK3,
            Provider::OpenRouter => ModelId::OpenRouterXiaomiMimoV25Pro,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::GPT54,
            Provider::LlamaCpp => ModelId::LlamaCppGptOss20b,
            Provider::ZAI => ModelId::ZaiGlm51,
            Provider::OpenCodeZen => ModelId::OpenCodeZenGPT54,
            Provider::OpenCodeGo => ModelId::OpenCodeGoMinimaxM27,
            Provider::Qwen => ModelId::QwenDeepSeekV4Flash,
            Provider::StepFun => ModelId::StepFun37Flash,
            Provider::Evolink => ModelId::EvolinkGpt52,
            Provider::Poolside => ModelId::PoolsideLagunaXs2,
        }
    }
}
