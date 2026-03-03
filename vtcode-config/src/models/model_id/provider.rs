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
            | ModelId::Gemini3FlashPreview => Provider::Gemini,
            ModelId::GPT5
            | ModelId::GPT52
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::GPT53Codex
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => Provider::OpenAI,
            ModelId::ClaudeOpus46
            | ModelId::ClaudeSonnet46
            | ModelId::ClaudeHaiku45 => Provider::Anthropic,
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => Provider::DeepSeek,
            ModelId::HuggingFaceDeepseekV32
            | ModelId::HuggingFaceOpenAIGptOss20b
            | ModelId::HuggingFaceOpenAIGptOss120b
            | ModelId::HuggingFaceMinimaxM25Novita
            | ModelId::HuggingFaceDeepseekV32Novita
            | ModelId::HuggingFaceXiaomiMimoV2FlashNovita
            | ModelId::HuggingFaceGlm5Novita
            | ModelId::HuggingFaceQwen3CoderNextNovita
            | ModelId::HuggingFaceQwen35397BA17BTogether => Provider::HuggingFace,
            ModelId::ZaiGlm5 => Provider::ZAI,
            ModelId::MoonshotKimiK25 => Provider::Moonshot,
            ModelId::OllamaGptOss20b
            | ModelId::OllamaGptOss20bCloud
            | ModelId::OllamaGptOss120bCloud
            | ModelId::OllamaQwen317b
            | ModelId::OllamaQwen3CoderNext
            | ModelId::OllamaDeepseekV32Cloud
            | ModelId::OllamaQwen3Next80bCloud
            | ModelId::OllamaGemini3FlashPreviewCloud
            | ModelId::OllamaMinimaxM2Cloud
            | ModelId::OllamaMinimaxM25Cloud
            | ModelId::OllamaGlm5Cloud => Provider::Ollama,
            ModelId::MinimaxM25 => Provider::Minimax,
            ModelId::OpenRouterMinimaxM25 | ModelId::OpenRouterQwen3CoderNext => {
                Provider::OpenRouter
            }
            _ => unreachable!(),
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }
}
