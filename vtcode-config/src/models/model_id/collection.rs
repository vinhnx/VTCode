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
            ModelId::Gemini35Flash,
            // OpenAI models
            ModelId::GPT55,
            ModelId::GPT54,
            ModelId::GPT54Pro,
            ModelId::GPT54Nano,
            ModelId::GPT54Mini,
            ModelId::GPT53Codex,
            ModelId::OpenAIGptOss20b,
            ModelId::OpenAIGptOss120b,
            // Anthropic models
            ModelId::ClaudeOpus48,
            ModelId::ClaudeSonnet46,
            ModelId::ClaudeHaiku45,
            ModelId::ClaudeMythosPreview,
            ModelId::CopilotAuto,
            ModelId::CopilotGPT52Codex,
            ModelId::CopilotGPT51CodexMax,
            ModelId::CopilotGPT54,
            ModelId::CopilotGPT54Mini,
            ModelId::CopilotClaudeSonnet46,
            // DeepSeek models
            ModelId::DeepSeekV4Pro,
            ModelId::DeepSeekV4Flash,
            // Mistral models
            ModelId::MistralLarge3,
            // Z.AI models
            ModelId::ZaiGlm5,
            ModelId::ZaiGlm51,
            // MiMo models
            ModelId::MiMoV25Pro,
            ModelId::MiMoV25,
            // Moonshot models
            ModelId::MoonshotKimiK26,
            // OpenCode Zen models
            ModelId::OpenCodeZenGPT54,
            ModelId::OpenCodeZenGPT54Mini,
            ModelId::OpenCodeZenClaudeSonnet46,
            ModelId::OpenCodeZenGlm51,
            // OpenCode Go models
            ModelId::OpenCodeGoGlm51,
            ModelId::OpenCodeGoMinimaxM25,
            ModelId::OpenCodeGoMinimaxM27,
            // Qwen models
            ModelId::Qwen37Max,
            ModelId::Qwen36Flash,
            ModelId::Qwen36Plus,
            ModelId::QwenDeepSeekV4Flash,
            ModelId::QwenDeepSeekV4Pro,
            ModelId::QwenGlm51,
            // Ollama models
            ModelId::OllamaGptOss20b,
            ModelId::OllamaGptOss20bCloud,
            ModelId::OllamaGptOss120bCloud,
            ModelId::OllamaQwen317b,
            ModelId::OllamaQwen3CoderNext,
            ModelId::OllamaDeepseekV4FlashCloud,
            ModelId::OllamaQwen3Next80bCloud,
            ModelId::OllamaDeepseekV4ProCloud,
            ModelId::OllamaGlm5Cloud,
            ModelId::OllamaGlm51Cloud,
            ModelId::OllamaGemini3FlashPreviewCloud,
            ModelId::OllamaMinimaxM2Cloud,
            ModelId::OllamaMinimaxM27Cloud,
            ModelId::OllamaMinimaxM3Cloud,
            ModelId::OllamaMinimaxM25Cloud,
            ModelId::OllamaKimiK26Cloud,
            ModelId::OllamaNemotron3SuperCloud,
            ModelId::OllamaNemotron3UltraCloud,
            ModelId::OllamaGemma4,
            ModelId::OllamaLagunaXs2,
            // llama.cpp models
            ModelId::LlamaCppQwen3627b,
            ModelId::LlamaCppQwen3635bA3b,
            ModelId::LlamaCppGemma426bA4b,
            ModelId::LlamaCppGemma4E4b,
            ModelId::LlamaCppGptOss20b,
            ModelId::LlamaCppStep35Flash,
            // MiniMax models
            ModelId::MinimaxM3,
            ModelId::MinimaxM27,
            ModelId::MinimaxM25,
            // Hugging Face models
            ModelId::HuggingFaceOpenAIGptOss20b,
            ModelId::HuggingFaceOpenAIGptOss120b,
            ModelId::HuggingFaceMinimaxM25Novita,
            ModelId::HuggingFaceGlm5Novita,
            ModelId::HuggingFaceGlm51ZaiOrg,
            ModelId::HuggingFaceQwen3CoderNextNovita,
            ModelId::HuggingFaceQwen35397BA17BTogether,
            ModelId::HuggingFaceKimiK26Novita,
            ModelId::HuggingFaceDeepseekV4FlashNovita,
            ModelId::HuggingFaceDeepseekV4ProTogether,
            ModelId::HuggingFaceStep35Flash,
            ModelId::HuggingFaceGlm51Deepinfra,
            ModelId::HuggingFaceMinimaxM27Novita,
            ModelId::HuggingFaceDeepseekV4ProNovita,
            ModelId::HuggingFaceNvidiaNemotron3Ultra550bA55bNvfp4Together,
            ModelId::StepFun37Flash,
            ModelId::EvolinkGpt52,
            ModelId::EvolinkGpt55,
            ModelId::EvolinkDeepseekV4Pro,
            ModelId::EvolinkDeepseekV4Flash,
            ModelId::EvolinkDoubaoSeed20Pro,
            ModelId::EvolinkGemini31Pro,
            ModelId::EvolinkGemini35Flash,
            ModelId::EvolinkMinimaxM3,
            ModelId::EvolinkClaudeSonnet46,
            ModelId::EvolinkClaudeOpus48,
            ModelId::EvolinkClaudeHaiku45,
            ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree,
            ModelId::OpenRouterMinimaxM25,
            ModelId::OpenRouterQwen3CoderNext,
            ModelId::OpenRouterMoonshotaiKimiK26,
            ModelId::OpenRouterZaiGlm51,
            // Poolside models
            ModelId::PoolsideLagunaM1,
            ModelId::PoolsideLagunaXs2,
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
