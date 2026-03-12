use crate::models::Provider;
use hashbrown::HashSet;

use super::ModelId;

impl ModelId {
    /// Return the OpenRouter vendor slug when this identifier maps to a marketplace listing
    pub fn openrouter_vendor(&self) -> Option<&'static str> {
        self.openrouter_metadata().map(|meta| meta.vendor)
    }

    /// Get all available models as a vector
    pub fn all_models() -> Vec<ModelId> {
        let mut models = vec![
            // Gemini models
            ModelId::Gemini31ProPreview,
            ModelId::Gemini31ProPreviewCustomTools,
            ModelId::Gemini31FlashLitePreview,
            ModelId::Gemini3FlashPreview,
            // OpenAI models
            ModelId::GPT,
            ModelId::GPT5,
            ModelId::GPT52,
            ModelId::GPT54,
            ModelId::GPT54Pro,
            ModelId::GPT5Mini,
            ModelId::GPT5Nano,
            ModelId::GPT53Codex,
            ModelId::OpenAIGptOss20b,
            ModelId::OpenAIGptOss120b,
            // Anthropic models
            ModelId::ClaudeOpus46,
            ModelId::ClaudeSonnet46,
            ModelId::ClaudeHaiku45,
            // DeepSeek models
            ModelId::DeepSeekChat,
            ModelId::DeepSeekReasoner,
            // Z.AI models
            ModelId::ZaiGlm5,
            // Moonshot models
            ModelId::MoonshotKimiK25,
            // Ollama models
            ModelId::OllamaGptOss20b,
            ModelId::OllamaGptOss20bCloud,
            ModelId::OllamaGptOss120bCloud,
            ModelId::OllamaQwen317b,
            ModelId::OllamaQwen3CoderNext,
            ModelId::OllamaDeepseekV32Cloud,
            ModelId::OllamaQwen3Next80bCloud,
            ModelId::OllamaGlm5Cloud,
            ModelId::OllamaGemini3FlashPreviewCloud,
            ModelId::OllamaMinimaxM2Cloud,
            ModelId::OllamaMinimaxM25Cloud,
            // MiniMax models
            ModelId::MinimaxM25,
            // Hugging Face models
            ModelId::HuggingFaceDeepseekV32,
            ModelId::HuggingFaceOpenAIGptOss20b,
            ModelId::HuggingFaceOpenAIGptOss120b,
            ModelId::HuggingFaceMinimaxM25Novita,
            ModelId::HuggingFaceDeepseekV32Novita,
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita,
            ModelId::HuggingFaceGlm5Novita,
            ModelId::HuggingFaceQwen3CoderNextNovita,
            ModelId::HuggingFaceQwen35397BA17BTogether,
            ModelId::HuggingFaceStep35Flash,
            ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree,
            ModelId::OpenRouterMinimaxM25,
            ModelId::OpenRouterQwen3CoderNext,
        ];
        models.extend(Self::openrouter_models());
        let mut seen = HashSet::new();
        models.retain(|model| seen.insert(*model));
        models
    }

    /// Get all models for a specific provider
    pub fn models_for_provider(provider: Provider) -> Vec<ModelId> {
        Self::all_models()
            .into_iter()
            .filter(|model| model.provider() == provider)
            .collect()
    }
}
