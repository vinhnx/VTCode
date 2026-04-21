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
            | ModelId::Gemini3FlashPreview => Provider::Gemini,
            ModelId::GPT5
            | ModelId::GPT52
            | ModelId::GPT52Codex
            | ModelId::GPT54
            | ModelId::GPT54Pro
            | ModelId::GPT54Nano
            | ModelId::GPT54Mini
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::GPT53Codex
            | ModelId::GPT51Codex
            | ModelId::GPT51CodexMax
            | ModelId::GPT5Codex
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => Provider::OpenAI,
            ModelId::ClaudeOpus47
            | ModelId::ClaudeOpus46
            | ModelId::ClaudeSonnet46
            | ModelId::ClaudeHaiku45
            | ModelId::ClaudeMythosPreview => Provider::Anthropic,
            ModelId::CopilotAuto
            | ModelId::CopilotGPT52Codex
            | ModelId::CopilotGPT51CodexMax
            | ModelId::CopilotGPT54
            | ModelId::CopilotGPT54Mini
            | ModelId::CopilotClaudeSonnet46 => Provider::Copilot,
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => Provider::DeepSeek,
            ModelId::HuggingFaceDeepseekV32
            | ModelId::HuggingFaceOpenAIGptOss20b
            | ModelId::HuggingFaceOpenAIGptOss120b
            | ModelId::HuggingFaceMinimaxM25Novita
            | ModelId::HuggingFaceDeepseekV32Novita
            | ModelId::HuggingFaceXiaomiMimoV2FlashNovita
            | ModelId::HuggingFaceGlm5Novita
            | ModelId::HuggingFaceGlm51ZaiOrg
            | ModelId::HuggingFaceQwen3CoderNextNovita
            | ModelId::HuggingFaceQwen35397BA17BTogether
            | ModelId::HuggingFaceKimiK26Novita
            | ModelId::HuggingFaceStep35Flash => Provider::HuggingFace,
            ModelId::ZaiGlm5 | ModelId::ZaiGlm51 => Provider::ZAI,
            ModelId::MoonshotKimiK26 | ModelId::MoonshotKimiK25 => Provider::Moonshot,
            ModelId::OpenCodeZenGPT54
            | ModelId::OpenCodeZenGPT54Mini
            | ModelId::OpenCodeZenClaudeSonnet46
            | ModelId::OpenCodeZenGlm51
            | ModelId::OpenCodeZenKimiK25 => Provider::OpenCodeZen,
            ModelId::OpenCodeGoGlm51
            | ModelId::OpenCodeGoKimiK25
            | ModelId::OpenCodeGoMinimaxM25
            | ModelId::OpenCodeGoMinimaxM27 => Provider::OpenCodeGo,
            ModelId::OllamaGptOss20b
            | ModelId::OllamaGptOss20bCloud
            | ModelId::OllamaGptOss120bCloud
            | ModelId::OllamaQwen317b
            | ModelId::OllamaQwen3CoderNext
            | ModelId::OllamaDeepseekV32Cloud
            | ModelId::OllamaQwen3Next80bCloud
            | ModelId::OllamaGemini3FlashPreviewCloud
            | ModelId::OllamaMinimaxM2Cloud
            | ModelId::OllamaMinimaxM27Cloud
            | ModelId::OllamaMinimaxM25Cloud
            | ModelId::OllamaNemotron3SuperCloud
            | ModelId::OllamaKimiK26Cloud
            | ModelId::OllamaGlm5Cloud
            | ModelId::OllamaGlm51Cloud => Provider::Ollama,
            ModelId::MinimaxM27 | ModelId::MinimaxM25 => Provider::Minimax,
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
            | ModelId::OpenRouterDeepseekChat
            | ModelId::OpenRouterDeepSeekV32
            | ModelId::OpenRouterDeepseekReasoner
            | ModelId::OpenRouterDeepSeekV32Speciale
            | ModelId::OpenRouterDeepSeekV32Exp
            | ModelId::OpenRouterDeepSeekChatV31
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGptOss120b
            | ModelId::OpenRouterOpenAIGptOss120bFree
            | ModelId::OpenRouterOpenAIGptOss20b
            | ModelId::OpenRouterOpenAIGpt5
            | ModelId::OpenRouterOpenAIGpt5Chat
            | ModelId::OpenRouterGoogleGemini31ProPreview
            | ModelId::OpenRouterAnthropicClaudeSonnet45
            | ModelId::OpenRouterAnthropicClaudeSonnet46
            | ModelId::OpenRouterAnthropicClaudeHaiku45
            | ModelId::OpenRouterMistralaiMistralLarge2512
            | ModelId::OpenRouterNexAgiDeepseekV31NexN1
            | ModelId::OpenRouterStepfunStep35FlashFree
            | ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree
            | ModelId::OpenRouterZaiGlm5
            | ModelId::OpenRouterZaiGlm47
            | ModelId::OpenRouterZaiGlm51
            | ModelId::OpenRouterMoonshotaiKimiK26 => Provider::OpenRouter,
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }
}
