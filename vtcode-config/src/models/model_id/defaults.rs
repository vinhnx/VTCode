use crate::models::Provider;

use super::ModelId;

impl ModelId {
    /// Get recommended fallback models in order of preference
    pub fn fallback_models() -> Vec<ModelId> {
        vec![
            ModelId::Gemini3FlashPreview,
            ModelId::GPT52,
            ModelId::GPT5,
            ModelId::OpenAIGptOss20b,
            ModelId::ClaudeOpus45,
            ModelId::ClaudeSonnet46,
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::DeepSeekReasoner,
            ModelId::ZaiGlm5,
        ]
    }

    /// Get the default model for general use
    pub fn default_model() -> Self {
        ModelId::Gemini3FlashPreview
    }

    /// Get the default orchestrator model (more capable)
    pub fn default_orchestrator() -> Self {
        ModelId::Gemini31ProPreview
    }

    /// Get the default subagent model (fast and efficient)
    pub fn default_subagent() -> Self {
        ModelId::Gemini3FlashPreview
    }

    /// Get provider-specific defaults for orchestrator
    pub fn default_orchestrator_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini31ProPreview,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus45,
            Provider::Minimax => ModelId::MinimaxM25,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::Gemini31ProPreview,
            Provider::OpenRouter => ModelId::OpenRouterQwen3Coder,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::GPT5,
            Provider::ZAI => ModelId::ZaiGlm5,
        }
    }

    /// Get provider-specific defaults for subagent
    pub fn default_subagent_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini3FlashPreview,
            Provider::OpenAI => ModelId::GPT5Mini,
            Provider::Anthropic => ModelId::ClaudeSonnet45,
            Provider::Minimax => ModelId::MinimaxM25,
            Provider::DeepSeek => ModelId::DeepSeekChat,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss20b,
            Provider::Moonshot => ModelId::Gemini3FlashPreview,
            Provider::OpenRouter => ModelId::OpenRouterQwen3Coder,
            Provider::Ollama => ModelId::OllamaQwen317b,
            Provider::LmStudio => ModelId::GPT5Mini,
            Provider::ZAI => ModelId::OllamaGlm5Cloud,
        }
    }

    /// Get provider-specific defaults for single agent
    pub fn default_single_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini3FlashPreview,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus41,
            Provider::Minimax => ModelId::MinimaxM25,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
            Provider::Moonshot => ModelId::Gemini3FlashPreview,
            Provider::OpenRouter => ModelId::OpenRouterQwen3Coder,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::GPT5,
            Provider::ZAI => ModelId::ZaiGlm5,
        }
    }
}
