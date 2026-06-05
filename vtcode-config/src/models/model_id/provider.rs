use crate::models::Provider;

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
            | ModelId::Gemini31FlashLitePreview
            | ModelId::Gemini35Flash => Provider::Gemini,
            ModelId::GPT55
            | ModelId::GPT54
            | ModelId::GPT54Pro
            | ModelId::GPT54Nano
            | ModelId::GPT54Mini
            | ModelId::GPT53Codex
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => Provider::OpenAI,
            ModelId::ClaudeOpus48
            | ModelId::ClaudeSonnet46
            | ModelId::ClaudeHaiku45
            | ModelId::ClaudeMythosPreview => Provider::Anthropic,
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
            | ModelId::HuggingFaceMinimaxM25Novita
            | ModelId::HuggingFaceGlm5Novita
            | ModelId::HuggingFaceGlm51ZaiOrg
            | ModelId::HuggingFaceQwen3CoderNextNovita
            | ModelId::HuggingFaceQwen35397BA17BTogether
            | ModelId::HuggingFaceKimiK26Novita
            | ModelId::HuggingFaceDeepseekV4FlashNovita
            | ModelId::HuggingFaceDeepseekV4ProTogether
            | ModelId::HuggingFaceStep35Flash
            | ModelId::HuggingFaceGlm51Deepinfra
            | ModelId::HuggingFaceMinimaxM27Novita
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
            ModelId::ZaiGlm5 | ModelId::ZaiGlm51 => Provider::ZAI,
            ModelId::MoonshotKimiK26 => Provider::Moonshot,
            ModelId::OpenCodeZenGPT54
            | ModelId::OpenCodeZenGPT54Mini
            | ModelId::OpenCodeZenClaudeSonnet46
            | ModelId::OpenCodeZenGlm51 => Provider::OpenCodeZen,
            ModelId::OpenCodeGoGlm51
            | ModelId::OpenCodeGoMinimaxM25
            | ModelId::OpenCodeGoMinimaxM27 => Provider::OpenCodeGo,
            ModelId::Qwen37Max
            | ModelId::Qwen36Flash
            | ModelId::Qwen36Plus
            | ModelId::QwenDeepSeekV4Flash
            | ModelId::QwenDeepSeekV4Pro
            | ModelId::QwenGlm51 => Provider::Qwen,
            ModelId::OllamaGptOss20b
            | ModelId::OllamaGptOss20bCloud
            | ModelId::OllamaGptOss120bCloud
            | ModelId::OllamaQwen317b
            | ModelId::OllamaQwen3CoderNext
            | ModelId::OllamaDeepseekV4FlashCloud
            | ModelId::OllamaDeepseekV4ProCloud
            | ModelId::OllamaQwen3Next80bCloud
            | ModelId::OllamaGemini3FlashPreviewCloud
            | ModelId::OllamaMinimaxM2Cloud
            | ModelId::OllamaMinimaxM27Cloud
            | ModelId::OllamaMinimaxM3Cloud
            | ModelId::OllamaMinimaxM25Cloud
            | ModelId::OllamaNemotron3SuperCloud
            | ModelId::OllamaKimiK26Cloud
            | ModelId::OllamaGlm5Cloud
            | ModelId::OllamaGlm51Cloud
            | ModelId::OllamaLagunaXs2 => Provider::Ollama,
            ModelId::LlamaCppQwen3627b
            | ModelId::LlamaCppQwen3635bA3b
            | ModelId::LlamaCppGemma426bA4b
            | ModelId::LlamaCppGemma4E4b
            | ModelId::LlamaCppGptOss20b
            | ModelId::LlamaCppStep35Flash => Provider::LlamaCpp,
            ModelId::MinimaxM3 | ModelId::MinimaxM27 | ModelId::MinimaxM25 => Provider::Minimax,
            ModelId::OpenRouterMinimaxM25 | ModelId::OpenRouterQwen3CoderNext => {
                Provider::OpenRouter
            }
            // OpenRouter models - explicitly handled even if openrouter_metadata() returns Some
            ModelId::OpenRouterQwen332b
            | ModelId::OpenRouterQwen330bA3b
            | ModelId::OpenRouterQwen330bA3bInstruct2507
            | ModelId::OpenRouterQwen330bA3bThinking2507
            | ModelId::OpenRouterQwen314b
            | ModelId::OpenRouterQwen38b
            | ModelId::OpenRouterQwen3Next80bA3bInstruct
            | ModelId::OpenRouterQwen3Next80bA3bThinking
            | ModelId::OpenRouterQwen35Plus0215
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash
            | ModelId::OpenRouterQwen3Coder30bA3bInstruct
            | ModelId::OpenRouterDeepSeekV4Pro
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
            | ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree
            | ModelId::OpenRouterZaiGlm5
            | ModelId::OpenRouterZaiGlm51
            | ModelId::OpenRouterMoonshotaiKimiK26
            | ModelId::OpenRouterQwenQwen37Max
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
