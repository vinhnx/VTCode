use crate::models::{Provider, ProviderModelSupport};

use super::ModelId;

impl ModelId {
    /// Get the provider for this model
    pub fn provider(&self) -> Provider {
        if self.openrouter_metadata().is_some() {
            return Provider::OpenRouter;
        }
        match self {
            ModelId::Gemini31ProPreview
            | ModelId::Gemini31ProPreviewCustomTools
            | ModelId::Gemini35Flash => Provider::Gemini,
            ModelId::GPT55
            | ModelId::GPT54
            | ModelId::GPT54Pro
            | ModelId::GPT54Nano
            | ModelId::GPT54Mini
            | ModelId::GPT53Codex
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => Provider::OpenAI,
            ModelId::ClaudeOpus48 | ModelId::ClaudeSonnet46 | ModelId::ClaudeHaiku45 => {
                Provider::Anthropic
            }
            ModelId::CopilotAuto
            | ModelId::CopilotGPT52Codex
            | ModelId::CopilotGPT51CodexMax
            | ModelId::CopilotGPT54
            | ModelId::CopilotGPT54Mini
            | ModelId::CopilotClaudeSonnet46 => Provider::Copilot,
            ModelId::DeepSeekV4Pro | ModelId::DeepSeekV4Flash => Provider::DeepSeek,
            ModelId::MistralLarge3 => Provider::Mistral,
            ModelId::MiMoV25Pro | ModelId::MiMoV25 => Provider::MiMo,
            ModelId::HuggingFaceOpenAIGptOss20b
            | ModelId::HuggingFaceOpenAIGptOss120b
            | ModelId::HuggingFaceGlm51ZaiOrg
            | ModelId::HuggingFaceGlm52Novita
            | ModelId::HuggingFaceKimiK26Novita
            | ModelId::HuggingFaceDeepseekV4FlashNovita
            | ModelId::HuggingFaceDeepseekV4ProTogether
            | ModelId::HuggingFaceStep35Flash
            | ModelId::HuggingFaceGlm51Deepinfra
            | ModelId::HuggingFaceMinimaxM27Novita
            | ModelId::HuggingFaceMinimaxM3Novita
            | ModelId::HuggingFaceDeepseekV4ProNovita => Provider::HuggingFace,
            ModelId::StepFun37Flash => Provider::StepFun,
            ModelId::EvolinkGpt52
            | ModelId::EvolinkGpt55
            | ModelId::EvolinkDeepseekV4Pro
            | ModelId::EvolinkDeepseekV4Flash
            | ModelId::EvolinkDoubaoSeed20Pro
            | ModelId::EvolinkGemini31Pro
            | ModelId::EvolinkGemini35Flash
            | ModelId::EvolinkMinimaxM3
            | ModelId::EvolinkClaudeSonnet46
            | ModelId::EvolinkClaudeOpus48
            | ModelId::EvolinkClaudeHaiku45 => Provider::Evolink,
            ModelId::ZaiGlm52 | ModelId::ZaiGlm51 => Provider::ZAI,
            ModelId::MoonshotKimiK27Code | ModelId::MoonshotKimiK26 => Provider::Moonshot,
            ModelId::OpenCodeZenGPT54
            | ModelId::OpenCodeZenGPT54Mini
            | ModelId::OpenCodeZenClaudeSonnet46
            | ModelId::OpenCodeZenGlm51 => Provider::OpenCodeZen,
            ModelId::OpenCodeGoGlm51 | ModelId::OpenCodeGoMinimaxM27 => Provider::OpenCodeGo,
            ModelId::QwenDeepSeekV4Flash | ModelId::QwenDeepSeekV4Pro | ModelId::QwenGlm51 => {
                Provider::Qwen
            }
            ModelId::OllamaGptOss20b
            | ModelId::OllamaGptOss20bCloud
            | ModelId::OllamaGptOss120bCloud
            | ModelId::OllamaDeepseekV4FlashCloud
            | ModelId::OllamaDeepseekV4ProCloud
            | ModelId::OllamaGlm51Cloud
            | ModelId::OllamaGlm52Cloud
            | ModelId::OllamaMinimaxM27Cloud
            | ModelId::OllamaMinimaxM3Cloud
            | ModelId::OllamaKimiK26Cloud
            | ModelId::OllamaKimiK27CodeCloud
            | ModelId::OllamaGemma4
            | ModelId::OllamaLagunaXs2 => Provider::Ollama,
            ModelId::LlamaCppGemma426bA4b
            | ModelId::LlamaCppGemma4E4b
            | ModelId::LlamaCppGptOss20b
            | ModelId::LlamaCppStep35Flash => Provider::LlamaCpp,
            ModelId::MinimaxM3 | ModelId::MinimaxM27 => Provider::Minimax,
            // OpenRouter models - explicitly handled even if openrouter_metadata() returns Some
            ModelId::OpenRouterDeepSeekV4Pro
            | ModelId::OpenRouterDeepSeekV4Flash
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGptOss120b
            | ModelId::OpenRouterOpenAIGptOss120bFree
            | ModelId::OpenRouterOpenAIGptOss20b
            | ModelId::OpenRouterOpenAIGpt5
            | ModelId::OpenRouterOpenAIGpt55
            | ModelId::OpenRouterOpenAIGpt5Chat
            | ModelId::OpenRouterGoogleGemini31ProPreview
            | ModelId::OpenRouterAnthropicClaudeSonnet46
            | ModelId::OpenRouterAnthropicClaudeHaiku45
            | ModelId::OpenRouterMistralaiMistralLarge2512
            | ModelId::OpenRouterNexAgiDeepseekV31NexN1
            | ModelId::OpenRouterStepfunStep35FlashFree
            | ModelId::OpenRouterZaiGlm51
            | ModelId::OpenRouterZaiGlm52
            | ModelId::OpenRouterMoonshotaiKimiK26
            | ModelId::OpenRouterMoonshotaiKimiK27Code
            | ModelId::OpenRouterTencentHy3Preview
            | ModelId::OpenRouterXAiGrokBuild01
            | ModelId::OpenRouterXiaomiMimoV25
            | ModelId::OpenRouterXiaomiMimoV25Pro
            | ModelId::OpenRouterPoolsideLagunaXs2Free
            | ModelId::OpenRouterPoolsideLagunaM1Free => Provider::OpenRouter,
            ModelId::PoolsideLagunaM1 | ModelId::PoolsideLagunaXs2 => Provider::Poolside,
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }
}
