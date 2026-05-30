use crate::models::Provider;

use super::ModelId;

impl ModelId {
    /// Get recommended fallback models in order of preference
    pub fn fallback_models() -> Vec<ModelId> {
        vec![
            ModelId::Gemini35Flash,
            ModelId::GPT54,
            ModelId::GPT55,
            ModelId::OpenAIGptOss20b,
            ModelId::ClaudeOpus48,
            ModelId::ClaudeSonnet46,
            ModelId::DeepSeekV4Pro,
            ModelId::ZaiGlm5,
        ]
    }

    /// Get the default model for general use
    pub fn default_model() -> Self {
        ModelId::Gemini35Flash
    }

    /// Get the default orchestrator model (more capable)
    pub fn default_orchestrator() -> Self {
        ModelId::Gemini31ProPreview
    }

    /// Get provider-specific defaults for orchestrator
    pub fn default_orchestrator_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini31ProPreview,
            Provider::OpenAI => ModelId::GPT54,
            Provider::Anthropic => ModelId::ClaudeOpus48,
            Provider::Copilot => ModelId::CopilotAuto,
            Provider::Minimax => ModelId::MinimaxM27,
            Provider::MiMo => ModelId::MiMoV25Pro,
            Provider::Mistral => ModelId::MistralLarge3,
            Provider::DeepSeek => ModelId::DeepSeekV4Pro,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::MoonshotKimiK26,
            Provider::OpenRouter => ModelId::OpenRouterQwen3Coder,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::GPT54,
            Provider::ZAI => ModelId::ZaiGlm5,
            Provider::OpenCodeZen => ModelId::OpenCodeZenGPT54,
            Provider::OpenCodeGo => ModelId::OpenCodeGoMinimaxM27,
            Provider::Qwen => ModelId::Qwen37Max,
            Provider::StepFun => ModelId::StepFun37Flash,
            Provider::Poolside => ModelId::PoolsideLagunaM1,
        }
    }

    /// Get provider-specific defaults for single agent
    pub fn default_single_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini35Flash,
            Provider::OpenAI => ModelId::GPT54,
            Provider::Anthropic => ModelId::ClaudeSonnet46,
            Provider::Copilot => ModelId::CopilotAuto,
            Provider::Minimax => ModelId::MinimaxM27,
            Provider::MiMo => ModelId::MiMoV25Pro,
            Provider::Mistral => ModelId::MistralLarge3,
            Provider::DeepSeek => ModelId::DeepSeekV4Pro,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::MoonshotKimiK26,
            Provider::OpenRouter => ModelId::OpenRouterQwen3Coder,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::GPT54,
            Provider::ZAI => ModelId::ZaiGlm5,
            Provider::OpenCodeZen => ModelId::OpenCodeZenGPT54,
            Provider::OpenCodeGo => ModelId::OpenCodeGoMinimaxM27,
            Provider::Qwen => ModelId::Qwen36Plus,
            Provider::StepFun => ModelId::StepFun37Flash,
            Provider::Poolside => ModelId::PoolsideLagunaXs2,
        }
    }
}
