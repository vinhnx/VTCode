use crate::models::Provider;

use super::ModelId;

impl ModelId {
    /// Get the provider for this model
    pub fn provider(&self) -> Provider {
        if self.openrouter_metadata().is_some() {
            return Provider::OpenRouter;
        }
        match self {
            ModelId::Gemini25FlashPreview
            | ModelId::Gemini25Flash
            | ModelId::Gemini25FlashLite
            | ModelId::Gemini25Pro
            | ModelId::Gemini3ProPreview
            | ModelId::Gemini3FlashPreview => Provider::Gemini,
            ModelId::GPT5
            | ModelId::GPT52
            | ModelId::GPT52Codex
            | ModelId::GPT5Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::GPT51
            | ModelId::GPT51Codex
            | ModelId::GPT51CodexMax
            | ModelId::GPT51Mini
            | ModelId::CodexMiniLatest
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => Provider::OpenAI,
            ModelId::ClaudeOpus46
            | ModelId::ClaudeOpus45
            | ModelId::ClaudeOpus41
            | ModelId::ClaudeSonnet45
            | ModelId::ClaudeHaiku45
            | ModelId::ClaudeSonnet4
            | ModelId::ClaudeOpus4
            | ModelId::ClaudeSonnet37
            | ModelId::ClaudeHaiku35 => Provider::Anthropic,
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => Provider::DeepSeek,
            ModelId::HuggingFaceDeepseekV32
            | ModelId::HuggingFaceOpenAIGptOss20b
            | ModelId::HuggingFaceOpenAIGptOss120b
            | ModelId::HuggingFaceGlm47
            | ModelId::HuggingFaceGlm47Novita
            | ModelId::HuggingFaceGlm47FlashNovita
            | ModelId::HuggingFaceKimiK2Thinking
            | ModelId::HuggingFaceKimiK25Novita
            | ModelId::HuggingFaceMinimaxM21Novita
            | ModelId::HuggingFaceDeepseekV32Novita
            | ModelId::HuggingFaceXiaomiMimoV2FlashNovita
            | ModelId::HuggingFaceGlm5Novita
            | ModelId::HuggingFaceQwen3CoderNextNovita => Provider::HuggingFace,
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => Provider::XAI,
            ModelId::ZaiGlm4Plus
            | ModelId::ZaiGlm4PlusDeepThinking
            | ModelId::ZaiGlm47
            | ModelId::ZaiGlm47DeepThinking
            | ModelId::ZaiGlm47Flash
            | ModelId::ZaiGlm5
            | ModelId::ZaiGlm432b0414128k => Provider::ZAI,
            ModelId::MoonshotKimiK25 => Provider::Moonshot,
            ModelId::OllamaGptOss20b
            | ModelId::OllamaGptOss20bCloud
            | ModelId::OllamaGptOss120bCloud
            | ModelId::OllamaQwen317b
            | ModelId::OllamaDeepseekV32Cloud
            | ModelId::OllamaQwen3Next80bCloud
            | ModelId::OllamaMistralLarge3675bCloud
            | ModelId::OllamaKimiK2ThinkingCloud
            | ModelId::OllamaKimiK25Cloud
            | ModelId::OllamaQwen3Coder480bCloud
            | ModelId::OllamaGemini3ProPreviewLatestCloud
            | ModelId::OllamaGemini3FlashPreviewCloud
            | ModelId::OllamaDevstral2123bCloud
            | ModelId::OllamaMinimaxM2Cloud
            | ModelId::OllamaMinimaxM21Cloud
            | ModelId::OllamaNemotron3Nano30bCloud
            | ModelId::OllamaGlm47Cloud => Provider::Ollama,
            ModelId::LmStudioMetaLlama38BInstruct
            | ModelId::LmStudioMetaLlama318BInstruct
            | ModelId::LmStudioQwen257BInstruct
            | ModelId::LmStudioGemma22BIt
            | ModelId::LmStudioGemma29BIt
            | ModelId::LmStudioPhi31Mini4kInstruct => Provider::LmStudio,
            ModelId::MinimaxM21 | ModelId::MinimaxM21Lightning | ModelId::MinimaxM2 => {
                Provider::Minimax
            }
            _ => unreachable!(),
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }
}
