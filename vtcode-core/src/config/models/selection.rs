//! Model selection helpers (defaults, fallbacks, and provider filtering).

use super::{ModelId, Provider};

impl ModelId {
    /// Get all models for a specific provider
    pub fn models_for_provider(provider: Provider) -> Vec<ModelId> {
        Self::all_models()
            .into_iter()
            .filter(|model| model.provider() == provider)
            .collect()
    }

    /// Get recommended fallback models in order of preference
    pub fn fallback_models() -> Vec<ModelId> {
        vec![
            ModelId::Gemini25FlashPreview,
            ModelId::Gemini25Pro,
            ModelId::GPT52,
            ModelId::GPT5,
            ModelId::GPT51,
            ModelId::OpenAIGptOss20b,
            ModelId::ClaudeOpus45,
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::DeepSeekReasoner,
            ModelId::XaiGrok4,
            ModelId::ZaiGlm47,
            ModelId::ZaiGlm46,
            ModelId::OpenRouterGrokCodeFast1,
        ]
    }

    /// Get the default model for general use
    pub fn default_model() -> Self {
        ModelId::Gemini25FlashPreview
    }

    /// Get the default orchestrator model (more capable)
    pub fn default_orchestrator() -> Self {
        ModelId::Gemini25Pro
    }

    /// Get the default subagent model (fast and efficient)
    pub fn default_subagent() -> Self {
        ModelId::Gemini25FlashPreview
    }

    /// Get provider-specific defaults for orchestrator
    pub fn default_orchestrator_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25Pro,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus45,
            Provider::Minimax => ModelId::MinimaxM21,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::OpenRouterGrokCodeFast1, // Fallback: no Moonshot models available
            Provider::XAI => ModelId::XaiGrok4,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::LmStudioMetaLlama318BInstruct,
            Provider::ZAI => ModelId::ZaiGlm47,
        }
    }

    /// Get provider-specific defaults for subagent
    pub fn default_subagent_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5Mini,
            Provider::Anthropic => ModelId::ClaudeSonnet45,
            Provider::Minimax => ModelId::MinimaxM21Lightning,
            Provider::DeepSeek => ModelId::DeepSeekChat,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss20b,
            Provider::Moonshot => ModelId::OpenRouterGrokCodeFast1, // Fallback: no Moonshot models available
            Provider::XAI => ModelId::XaiGrok4Code,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaQwen317b,
            Provider::LmStudio => ModelId::LmStudioQwen257BInstruct,
            Provider::ZAI => ModelId::ZaiGlm45Flash,
        }
    }

    /// Get provider-specific defaults for single agent
    pub fn default_single_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus41,
            Provider::Minimax => ModelId::MinimaxM21,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::OpenRouterGrokCodeFast1, // Fallback: no Moonshot models available
            Provider::XAI => ModelId::XaiGrok4,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::LmStudioMetaLlama318BInstruct,
            Provider::ZAI => ModelId::ZaiGlm47,
        }
    }
}
