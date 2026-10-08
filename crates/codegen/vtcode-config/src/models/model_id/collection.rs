use std::collections::BTreeMap;
use std::str::FromStr;

use crate::core::{CustomProviderConfig, ProviderOverrideConfig};
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
            ModelId::Gemini38Flash,
            // OpenAI models
            ModelId::GPT6Astra,
            ModelId::GPT6Sol,
            ModelId::GPT61Sol,
            ModelId::GPT6Luna,
            ModelId::GPT56Sol,
            ModelId::GPT56Terra,
            ModelId::GPT56Luna,
            ModelId::OpenAIGptOss20b,
            ModelId::OpenAIGptOss120b,
            // Anthropic models
            ModelId::ClaudeSonnet5,
            ModelId::ClaudeSonnet55,
            ModelId::ClaudeHaiku55,
            ModelId::ClaudeFable5,
            ModelId::ClaudeFable51,
            ModelId::ClaudeOpus5,
            ModelId::ClaudeOpus55,
            ModelId::CopilotAuto,
            ModelId::CopilotGPT52Codex,
            ModelId::CopilotGPT51CodexMax,
            ModelId::CopilotGPT54,
            ModelId::CopilotGPT54Mini,
            // DeepSeek models
            ModelId::DeepSeekFlash,
            // Official Meta AI models (kept before marketplace entries)
            ModelId::MetaMuseSpark13,
            ModelId::MetaMuseSpark13Contributor,
            ModelId::MetaMuseSpark11,
            // NVIDIA NIM models
            ModelId::NvidiaNemotron3Ultra550bA55b,
            ModelId::NvidiaNemotron3Super120bA12b,
            ModelId::NvidiaNemotron3Nano30bA3b,
            // Merge Gateway routes
            ModelId::MergeGatewayDefaultRouting,
            ModelId::MergeGatewayAnthropicClaudeOpus5,
            ModelId::MergeGatewayAnthropicClaudeOpus55,
            ModelId::MergeGatewayAnthropicClaudeSonnet5,
            ModelId::MergeGatewayAnthropicClaudeSonnet55,
            ModelId::MergeGatewayXaiGrok46,
            ModelId::MergeGatewayXaiGrok47,
            ModelId::MergeGatewayMinimaxH3,
            ModelId::MergeGatewayMoonshotKimiK3,
            ModelId::MergeGatewayThinkingMachinesInkling,
            ModelId::MergeGatewayZaiGlm53Flash,
            ModelId::MergeGatewayZaiGlm53Flashx,
            ModelId::MergeGatewayOpenAIGpt56Luna,
            ModelId::MergeGatewayOpenAIGpt56Sol,
            ModelId::MergeGatewayOpenAIGpt56Terra,
            ModelId::MergeGatewayGoogleGemini38Flash,
            ModelId::MergeGatewayAnthropicClaudeHaiku4520251001,
            ModelId::MergeGatewayAnthropicClaudeHaiku55,
            ModelId::MergeGatewayAnthropicClaudeFable51,
            ModelId::MergeGatewayDeepseekFlash,
            ModelId::MergeGatewayOpenAIGpt6Astra,
            ModelId::MergeGatewayOpenAIGpt6Sol,
            ModelId::MergeGatewayOpenAIGpt61Sol,
            ModelId::MergeGatewayOpenAIGpt6Luna,
            ModelId::MergeGatewayXiaomimimoMimoV26Pro,
            ModelId::MergeGatewayXiaomimimoMimoV26Flash,
            ModelId::MergeGatewayMistralLarge4,
            // Mistral models
            ModelId::MistralLarge3,
            ModelId::MistralLarge4,
            // Z.AI models
            ModelId::ZaiGlm53,
            ModelId::ZaiGlm53Flash,
            ModelId::ZaiGlm53Flashx,
            // MiMo models
            ModelId::MiMoV26Pro,
            ModelId::MiMoV26Flash,
            ModelId::MiMoV26ProUltraspeed,
            // Moonshot models
            ModelId::MoonshotKimiK3,
            // OpenCode Zen models
            // OpenCode Go models
            ModelId::OpenCodeGoGlm53,
            ModelId::OpenCodeGoGpt56Luna,
            ModelId::OpenCodeGoKimiK3,
            ModelId::OpenCodeGoMinimaxM3,
            // Ollama models
            ModelId::OllamaGptOss20b,
            ModelId::OllamaGptOss20bCloud,
            ModelId::OllamaGptOss120bCloud,
            ModelId::OllamaGlm53Cloud,
            ModelId::OllamaMinimaxM3Cloud,
            ModelId::OllamaKimiK3Cloud,
            ModelId::OllamaGemma4,
            // llama.cpp models
            ModelId::LlamaCppGemma426bA4b,
            ModelId::LlamaCppGemma4E4b,
            ModelId::LlamaCppGptOss20b,
            // MiniMax models
            ModelId::MinimaxM3,
            // Hugging Face models
            ModelId::HuggingFaceOpenAIGptOss20b,
            ModelId::HuggingFaceOpenAIGptOss120b,
            ModelId::HuggingFaceGlm53FlashTogether,
            ModelId::HuggingFaceGlm53Together,
            ModelId::HuggingFaceKimiK3Together,
            ModelId::HuggingFaceMinimaxM3Novita,
            ModelId::StepFun37Flash,
            ModelId::StepFun5Preview,
            ModelId::EvolinkGemini31Pro,
            ModelId::EvolinkMinimaxM3,
            ModelId::EvolinkClaudeHaiku45,
            ModelId::OpenRouterMoonshotaiKimiK3,
            ModelId::OpenRouterZaiGlm53Flash,
            ModelId::OpenRouterZaiGlm53Flashx,
            // xAI models
            ModelId::XaiGrok46,
            ModelId::XaiGrok47,
            // Vercel AI Gateway models
            ModelId::VercelAnthropicClaudeSonnet5,
            ModelId::VercelAnthropicClaudeOpus5,
            ModelId::VercelAnthropicClaudeOpus55,
            ModelId::VercelAnthropicClaudeHaiku45,
            ModelId::VercelOpenAiGpt56Sol,
            ModelId::VercelOpenAiGpt6Astra,
            ModelId::VercelOpenAiGpt56Luna,
            ModelId::VercelGoogleGemini38Flash,
            ModelId::VercelDeepseekFlash,
            ModelId::VercelMoonshotaiKimiK3,
            ModelId::VercelMinimaxM3,
            ModelId::VercelSpacexaiGrok47,
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

    /// Resolve a model identifier against a configuration, falling back to
    /// [`ModelId::from_str`].
    ///
    /// Models declared under the active provider in `[providers.<name>]`
    /// overrides or by a `[[custom_providers]]` profile are not part of the
    /// static catalog, so they are represented as [`ModelId::Custom`].
    /// Matching is scoped to the active provider to avoid mis-routing a model
    /// ID shared across providers, mirroring the catalog and custom-provider
    /// branches of the subagent resolution path. Local-provider pass-through
    /// (arbitrary Ollama/llama.cpp IDs) is intentionally not handled here.
    pub fn from_config(
        model: &str,
        provider: &str,
        provider_overrides: &BTreeMap<String, ProviderOverrideConfig>,
        custom_providers: &[CustomProviderConfig],
    ) -> Result<Self, crate::models::ModelParseError> {
        let trimmed = model.trim();
        if let Ok(parsed) = Self::from_str(trimmed) {
            return Ok(parsed);
        }
        let hinted_provider = provider.trim().parse::<Provider>().ok();
        for (provider_key, override_cfg) in provider_overrides {
            let matches_hint = match hinted_provider {
                Some(active) => provider_key.parse::<Provider>().ok() == Some(active),
                None => provider_key.eq_ignore_ascii_case(provider.trim()),
            };
            if matches_hint && override_cfg.models.iter().any(|candidate| candidate.trim() == trimmed) {
                return Ok(ModelId::Custom(provider_key.clone(), trimmed.to_owned()));
            }
        }
        for custom in custom_providers {
            if custom.name.eq_ignore_ascii_case(provider.trim())
                && custom.effective_models().iter().any(|candidate| candidate == trimmed)
            {
                return Ok(ModelId::Custom(custom.name.to_lowercase(), trimmed.to_owned()));
            }
        }
        Self::from_str(trimmed)
    }
}
