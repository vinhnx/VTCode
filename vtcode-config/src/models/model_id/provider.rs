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
            | ModelId::ClaudeOpus45
            | ModelId::ClaudeOpus41
            | ModelId::ClaudeSonnet45
            | ModelId::ClaudeHaiku45
            | ModelId::ClaudeSonnet4 => Provider::Anthropic,
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => Provider::DeepSeek,
            ModelId::HuggingFaceDeepseekV32
            | ModelId::HuggingFaceOpenAIGptOss20b
            | ModelId::HuggingFaceOpenAIGptOss120b
            | ModelId::HuggingFaceMinimaxM25Novita
            | ModelId::HuggingFaceDeepseekV32Novita
            | ModelId::HuggingFaceXiaomiMimoV2FlashNovita
            | ModelId::HuggingFaceGlm5Novita
            | ModelId::HuggingFaceQwen3CoderNextNovita => Provider::HuggingFace,
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => Provider::XAI,
            ModelId::ZaiGlm5 => Provider::ZAI,
            ModelId::MoonshotMinimaxM25 | ModelId::MoonshotQwen3CoderNext => Provider::Moonshot,
            ModelId::OllamaGptOss20b
            | ModelId::OllamaGptOss20bCloud
            | ModelId::OllamaGptOss120bCloud
            | ModelId::OllamaQwen317b
            | ModelId::OllamaDeepseekV32Cloud
            | ModelId::OllamaQwen3Next80bCloud
            | ModelId::OllamaMistralLarge3675bCloud
            | ModelId::OllamaQwen3Coder480bCloud
            | ModelId::OllamaGemini3FlashPreviewCloud
            | ModelId::OllamaDevstral2123bCloud
            | ModelId::OllamaMinimaxM2Cloud
            | ModelId::OllamaMinimaxM25Cloud
            | ModelId::OllamaNemotron3Nano30bCloud
            | ModelId::OllamaGlm5Cloud => Provider::Ollama,
            ModelId::MinimaxM25 | ModelId::MinimaxM2 => Provider::Minimax,
            ModelId::OpenRouterMinimaxM25
            | ModelId::OpenRouterQwen3CoderNext => Provider::OpenRouter,
            _ => unreachable!(),
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }
}
