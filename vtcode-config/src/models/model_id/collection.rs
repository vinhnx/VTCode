use crate::models::Provider;

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
            ModelId::Gemini25FlashPreview,
            ModelId::Gemini25Flash,
            ModelId::Gemini25FlashLite,
            ModelId::Gemini25Pro,
            // OpenAI models
            ModelId::GPT5,
            ModelId::GPT5Codex,
            ModelId::GPT5Mini,
            ModelId::GPT5Nano,
            ModelId::CodexMiniLatest,
            // Anthropic models
            ModelId::ClaudeOpus45,
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::ClaudeHaiku45,
            ModelId::ClaudeSonnet4,
            // DeepSeek models
            ModelId::DeepSeekChat,
            ModelId::DeepSeekReasoner,
            // xAI models
            ModelId::XaiGrok4,
            ModelId::XaiGrok4Mini,
            ModelId::XaiGrok4Code,
            ModelId::XaiGrok4CodeLatest,
            ModelId::XaiGrok4Vision,
            // Z.AI models
            ModelId::ZaiGlm46,
            ModelId::ZaiGlm47Flash,
            ModelId::ZaiGlm45,
            ModelId::ZaiGlm45Air,
            ModelId::ZaiGlm45X,
            ModelId::ZaiGlm45Airx,
            ModelId::ZaiGlm45Flash,
            ModelId::ZaiGlm432b0414128k,
            // Moonshot models
            ModelId::MoonshotKimiK25,
            // Ollama models
            ModelId::OllamaGptOss20b,
            ModelId::OllamaGptOss20bCloud,
            ModelId::OllamaGptOss120bCloud,
            ModelId::OllamaQwen317b,
            ModelId::OllamaDeepseekV32Cloud,
            ModelId::OllamaQwen3Next80bCloud,
            ModelId::OllamaMistralLarge3675bCloud,
            ModelId::OllamaKimiK2ThinkingCloud,
            ModelId::OllamaKimiK25Cloud,
            ModelId::OllamaQwen3Coder480bCloud,
            ModelId::OllamaGlm46Cloud,
            ModelId::OllamaGlm47Cloud,
            ModelId::OllamaGemini3ProPreviewLatestCloud,
            ModelId::OllamaGemini3FlashPreviewCloud,
            ModelId::OllamaDevstral2123bCloud,
            ModelId::OllamaMinimaxM2Cloud,
            ModelId::OllamaMinimaxM21Cloud,
            ModelId::OllamaNemotron3Nano30bCloud,
            // LM Studio models
            ModelId::LmStudioMetaLlama38BInstruct,
            ModelId::LmStudioMetaLlama318BInstruct,
            ModelId::LmStudioQwen257BInstruct,
            ModelId::LmStudioGemma22BIt,
            ModelId::LmStudioGemma29BIt,
            ModelId::LmStudioPhi31Mini4kInstruct,
            // MiniMax models
            ModelId::MinimaxM21,
            ModelId::MinimaxM21Lightning,
            ModelId::MinimaxM2,
            // Hugging Face models
            ModelId::HuggingFaceDeepseekV32,
            ModelId::HuggingFaceOpenAIGptOss20b,
            ModelId::HuggingFaceOpenAIGptOss120b,
            ModelId::HuggingFaceGlm47,
            ModelId::HuggingFaceGlm47FlashNovita,
            ModelId::HuggingFaceKimiK2Thinking,
            ModelId::HuggingFaceKimiK25Novita,
            ModelId::HuggingFaceMinimaxM21Novita,
            ModelId::HuggingFaceDeepseekV32Novita,
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita,
        ];
        models.extend(Self::openrouter_models());
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
