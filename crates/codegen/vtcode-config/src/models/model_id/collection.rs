use std::collections::BTreeMap;

use crate::core::ProviderOverrideConfig;
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
            ModelId::Gemini35Flash,
            ModelId::Gemini35FlashLite,
            ModelId::Gemini36Flash,
            // OpenAI models
            ModelId::GPT56Sol,
            ModelId::GPT56Terra,
            ModelId::GPT56Luna,
            ModelId::GPT55,
            ModelId::GPT54,
            ModelId::GPT54Pro,
            ModelId::GPT54Nano,
            ModelId::GPT54Mini,
            ModelId::GPT53Codex,
            ModelId::OpenAIGptOss20b,
            ModelId::OpenAIGptOss120b,
            // Anthropic models
            ModelId::ClaudeSonnet5,
            ModelId::ClaudeFable5,
            ModelId::ClaudeMythos5,
            ModelId::ClaudeOpus48,
            ModelId::ClaudeSonnet46,
            ModelId::ClaudeHaiku45,
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
            ModelId::ZaiGlm52,
            ModelId::ZaiGlm51,
            // MiMo models
            ModelId::MiMoV25Pro,
            ModelId::MiMoV25,
            // Moonshot models
            ModelId::MoonshotKimiK3,
            ModelId::MoonshotKimiK27Code,
            ModelId::MoonshotKimiK26,
            // OpenCode Zen models
            ModelId::OpenCodeZenGPT54,
            ModelId::OpenCodeZenGPT54Mini,
            ModelId::OpenCodeZenClaudeSonnet46,
            ModelId::OpenCodeZenGlm51,
            // OpenCode Go models
            ModelId::OpenCodeGoGlm52,
            ModelId::OpenCodeGoGlm51,
            ModelId::OpenCodeGoKimiK27Code,
            ModelId::OpenCodeGoKimiK26,
            ModelId::OpenCodeGoMimoV25,
            ModelId::OpenCodeGoMimoV25Pro,
            ModelId::OpenCodeGoMinimaxM3,
            ModelId::OpenCodeGoMinimaxM27,
            ModelId::OpenCodeGoQwen37Max,
            ModelId::OpenCodeGoQwen37Plus,
            ModelId::OpenCodeGoQwen36Plus,
            ModelId::OpenCodeGoDeepseekV4Pro,
            ModelId::OpenCodeGoDeepseekV4Flash,
            // Qwen models
            ModelId::QwenDeepSeekV4Flash,
            ModelId::QwenDeepSeekV4Pro,
            ModelId::QwenGlm51,
            // Ollama models
            ModelId::OllamaGptOss20b,
            ModelId::OllamaGptOss20bCloud,
            ModelId::OllamaGptOss120bCloud,
            ModelId::OllamaDeepseekV4FlashCloud,
            ModelId::OllamaDeepseekV4ProCloud,
            ModelId::OllamaGlm51Cloud,
            ModelId::OllamaGlm52Cloud,
            ModelId::OllamaMinimaxM27Cloud,
            ModelId::OllamaMinimaxM3Cloud,
            ModelId::OllamaKimiK26Cloud,
            ModelId::OllamaKimiK27CodeCloud,
            ModelId::OllamaGemma4,
            ModelId::OllamaLagunaXs2,
            // llama.cpp models
            ModelId::LlamaCppGemma426bA4b,
            ModelId::LlamaCppGemma4E4b,
            ModelId::LlamaCppGptOss20b,
            ModelId::LlamaCppStep35Flash,
            // MiniMax models
            ModelId::MinimaxM3,
            ModelId::MinimaxM27,
            // Hugging Face models
            ModelId::HuggingFaceOpenAIGptOss20b,
            ModelId::HuggingFaceOpenAIGptOss120b,
            ModelId::HuggingFaceGlm51ZaiOrg,
            ModelId::HuggingFaceGlm52Novita,
            ModelId::HuggingFaceKimiK26Novita,
            ModelId::HuggingFaceDeepseekV4FlashNovita,
            ModelId::HuggingFaceDeepseekV4ProTogether,
            ModelId::HuggingFaceStep35Flash,
            ModelId::HuggingFaceGlm51Deepinfra,
            ModelId::HuggingFaceMinimaxM27Novita,
            ModelId::HuggingFaceMinimaxM3Novita,
            ModelId::HuggingFaceDeepseekV4ProNovita,
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
            ModelId::OpenRouterMoonshotaiKimiK3,
            ModelId::OpenRouterMoonshotaiKimiK26,
            ModelId::OpenRouterMoonshotaiKimiK27Code,
            ModelId::OpenRouterZaiGlm51,
            ModelId::OpenRouterZaiGlm52,
            // xAI models
            ModelId::XaiGrokBuild01,
            ModelId::XaiGrok45,
            ModelId::XaiGrok43,
            ModelId::XaiGrok420Reasoning,
            // Poolside models
            ModelId::PoolsideLagunaM1,
            ModelId::PoolsideLagunaXs2,
            ModelId::PoolsideLagunaS21,
        ];
        models.extend(Self::openrouter_models());
        let mut seen = HashSet::new();
        models.retain(|model| seen.insert(model.clone()));
        models
    }

    /// Get all models for a specific provider
    pub fn models_for_provider(provider: Provider) -> Vec<ModelId> {
        Self::all_models()
            .into_iter()
            .filter(|model| model.provider() == provider)
            .collect()
    }

    /// Return all models including user-defined overrides from config.
    ///
    /// Merges the hardcoded model list with custom models defined in
    /// `[providers.<name>]` config sections. Custom models are appended
    /// as `ModelId::Custom` variants keyed by provider name.
    pub fn all_models_with_overrides(overrides: &BTreeMap<String, ProviderOverrideConfig>) -> Vec<ModelId> {
        let mut models = Self::all_models();
        for (provider_key, config) in overrides {
            for model_name in &config.models {
                let trimmed = model_name.trim().to_string();
                if !trimmed.is_empty() {
                    models.push(ModelId::Custom(provider_key.clone(), trimmed));
                }
            }
        }
        models
    }

    /// Get all models for a specific provider, including user-defined overrides.
    pub fn models_for_provider_with_overrides(
        provider: Provider,
        overrides: &BTreeMap<String, ProviderOverrideConfig>,
    ) -> Vec<ModelId> {
        Self::all_models_with_overrides(overrides)
            .into_iter()
            .filter(|model| model.provider() == provider)
            .collect()
    }
}
