use crate::models::Provider;

use super::ModelId;

impl ModelId {
    /// Get recommended fallback models in order of preference
    pub fn fallback_models() -> Vec<ModelId> {
        vec![
            ModelId::Gemini25FlashPreview,
            ModelId::Gemini25Pro,
            ModelId::GPT5,
            ModelId::OpenAIGptOss20b,
            ModelId::ClaudeOpus45,
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::DeepSeekReasoner,
            ModelId::XaiGrok4,
            ModelId::ZaiGlm5,
            ModelId::OpenRouterGrokCodeFast1,
        ]
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
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::XAI => ModelId::XaiGrok4,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::LmStudioMetaLlama318BInstruct,
            Provider::ZAI => ModelId::ZaiGlm5,
            Provider::Moonshot => ModelId::MoonshotKimiK25,
            Provider::Minimax => ModelId::MinimaxM21,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
        }
    }

    /// Get provider-specific defaults for subagent
    pub fn default_subagent_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5Mini,
            Provider::Anthropic => ModelId::ClaudeSonnet45,
            Provider::DeepSeek => ModelId::DeepSeekChat,
            Provider::XAI => ModelId::XaiGrok4Code,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaQwen317b,
            Provider::LmStudio => ModelId::LmStudioQwen257BInstruct,
            Provider::ZAI => ModelId::ZaiGlm47Flash,
            Provider::Moonshot => ModelId::MoonshotKimiK25,
            Provider::Minimax => ModelId::MinimaxM21Lightning,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss20b,
        }
    }

    /// Get provider-specific defaults for single agent
    pub fn default_single_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus45,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::XAI => ModelId::XaiGrok4,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaGptOss20b,
            Provider::LmStudio => ModelId::LmStudioMetaLlama318BInstruct,
            Provider::ZAI => ModelId::ZaiGlm5,
            Provider::Moonshot => ModelId::MoonshotKimiK25,
            Provider::Minimax => ModelId::MinimaxM21,
            Provider::HuggingFace => ModelId::HuggingFaceOpenAIGptOss120b,
        }
    }
}
