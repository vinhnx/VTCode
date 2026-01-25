//! Formatting and parsing for model identifiers.

use std::fmt;
use std::str::FromStr;

use super::{ModelId, ModelParseError};

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ModelId {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use crate::config::constants::models;
        if let Some(model) = Self::parse_openrouter_model(s) {
            return Ok(model);
        }
        match s {
            // Gemini models
            s if s == models::GEMINI_2_5_FLASH_PREVIEW => Ok(ModelId::Gemini25FlashPreview),
            s if s == models::GEMINI_2_5_FLASH => Ok(ModelId::Gemini25Flash),
            s if s == models::GEMINI_2_5_FLASH_LITE => Ok(ModelId::Gemini25FlashLite),
            s if s == models::GEMINI_2_5_PRO => Ok(ModelId::Gemini25Pro),
            // OpenAI models
            s if s == models::GPT_5 => Ok(ModelId::GPT5),
            s if s == models::openai::GPT_5_2 => Ok(ModelId::GPT52),
            s if s == models::openai::GPT_5_2_ALIAS => Ok(ModelId::GPT52),
            s if s == models::openai::GPT_5_2_CODEX => Ok(ModelId::GPT52Codex),
            s if s == models::GPT_5_CODEX => Ok(ModelId::GPT5Codex),
            s if s == models::GPT_5_MINI => Ok(ModelId::GPT5Mini),
            s if s == models::GPT_5_NANO => Ok(ModelId::GPT5Nano),
            s if s == models::openai::GPT_5_1 => Ok(ModelId::GPT51),
            s if s == models::openai::GPT_5_1_CODEX => Ok(ModelId::GPT51Codex),
            s if s == models::openai::GPT_5_1_CODEX_MAX => Ok(ModelId::GPT51CodexMax),
            s if s == models::openai::GPT_5_1_MINI => Ok(ModelId::GPT51Mini),
            s if s == models::CODEX_MINI_LATEST => Ok(ModelId::CodexMiniLatest),
            s if s == models::openai::GPT_OSS_20B => Ok(ModelId::OpenAIGptOss20b),
            s if s == models::openai::GPT_OSS_120B => Ok(ModelId::OpenAIGptOss120b),
            // Anthropic models
            s if s == models::CLAUDE_OPUS_4_1_20250805 => Ok(ModelId::ClaudeOpus41),
            s if s == models::CLAUDE_OPUS_4_1 => Ok(ModelId::ClaudeOpus41),
            s if s == models::CLAUDE_OPUS_4_5_20251101 => Ok(ModelId::ClaudeOpus45),
            s if s == models::CLAUDE_OPUS_4_5 => Ok(ModelId::ClaudeOpus45),
            s if s == models::CLAUDE_SONNET_4_5 => Ok(ModelId::ClaudeSonnet45),
            s if s == models::CLAUDE_HAIKU_4_5 => Ok(ModelId::ClaudeHaiku45),
            s if s == models::CLAUDE_SONNET_4_5_20250929 => Ok(ModelId::ClaudeSonnet45),
            s if s == models::CLAUDE_SONNET_4_20250514 => Ok(ModelId::ClaudeSonnet4),
            s if s == models::CLAUDE_SONNET_4_0 => Ok(ModelId::ClaudeSonnet4),
            s if s == models::CLAUDE_OPUS_4_20250514 => Ok(ModelId::ClaudeOpus4),
            s if s == models::CLAUDE_OPUS_4_0 => Ok(ModelId::ClaudeOpus4),
            s if s == models::CLAUDE_3_7_SONNET_20250219 => Ok(ModelId::ClaudeSonnet37),
            s if s == models::CLAUDE_3_7_SONNET_LATEST => Ok(ModelId::ClaudeSonnet37),
            s if s == models::CLAUDE_3_5_HAIKU_20241022 => Ok(ModelId::ClaudeHaiku35),
            s if s == models::CLAUDE_3_5_HAIKU_LATEST => Ok(ModelId::ClaudeHaiku35),
            // DeepSeek models
            s if s == models::DEEPSEEK_CHAT => Ok(ModelId::DeepSeekChat),
            s if s == models::DEEPSEEK_REASONER => Ok(ModelId::DeepSeekReasoner),
            // Hugging Face models
            s if s == models::huggingface::DEEPSEEK_V32 => Ok(ModelId::HuggingFaceDeepseekV32),
            s if s == models::huggingface::OPENAI_GPT_OSS_20B => {
                Ok(ModelId::HuggingFaceOpenAIGptOss20b)
            }
            s if s == models::huggingface::OPENAI_GPT_OSS_120B => {
                Ok(ModelId::HuggingFaceOpenAIGptOss120b)
            }
            s if s == models::huggingface::ZAI_GLM_47 => Ok(ModelId::HuggingFaceGlm47),
            s if s == models::huggingface::ZAI_GLM_47_NOVITA => Ok(ModelId::HuggingFaceGlm47Novita),
            s if s == models::huggingface::MOONSHOT_KIMI_K2_THINKING => {
                Ok(ModelId::HuggingFaceKimiK2Thinking)
            }
            s if s == models::huggingface::MINIMAX_M2_1_NOVITA => {
                Ok(ModelId::HuggingFaceMinimaxM21Novita)
            }
            s if s == models::huggingface::DEEPSEEK_V32_NOVITA => {
                Ok(ModelId::HuggingFaceDeepseekV32Novita)
            }
            s if s == models::huggingface::XIAOMI_MIMO_V2_FLASH_NOVITA => {
                Ok(ModelId::HuggingFaceXiaomiMimoV2FlashNovita)
            }
            // xAI models
            s if s == models::xai::GROK_4 => Ok(ModelId::XaiGrok4),
            s if s == models::xai::GROK_4_MINI => Ok(ModelId::XaiGrok4Mini),
            s if s == models::xai::GROK_4_CODE => Ok(ModelId::XaiGrok4Code),
            s if s == models::xai::GROK_4_CODE_LATEST => Ok(ModelId::XaiGrok4CodeLatest),
            s if s == models::xai::GROK_4_VISION => Ok(ModelId::XaiGrok4Vision),
            // Z.AI models
            s if s == models::zai::GLM_4_7 => Ok(ModelId::ZaiGlm47),
            s if s == models::zai::GLM_4_7_DEEP_THINKING => Ok(ModelId::ZaiGlm47DeepThinking),
            s if s == models::zai::GLM_4_6 => Ok(ModelId::ZaiGlm46),
            s if s == models::zai::GLM_4_6_DEEP_THINKING => Ok(ModelId::ZaiGlm46DeepThinking),
            s if s == models::zai::GLM_4_5 => Ok(ModelId::ZaiGlm45),
            s if s == models::zai::GLM_4_5_DEEP_THINKING => Ok(ModelId::ZaiGlm45DeepThinking),
            s if s == models::zai::GLM_4_5_AIR => Ok(ModelId::ZaiGlm45Air),
            s if s == models::zai::GLM_4_5_X => Ok(ModelId::ZaiGlm45X),
            s if s == models::zai::GLM_4_5_AIRX => Ok(ModelId::ZaiGlm45Airx),
            s if s == models::zai::GLM_4_5_FLASH => Ok(ModelId::ZaiGlm45Flash),
            s if s == models::zai::GLM_4_32B_0414_128K => Ok(ModelId::ZaiGlm432b0414128k),
            // Ollama models
            s if s == models::ollama::GPT_OSS_20B => Ok(ModelId::OllamaGptOss20b),
            s if s == models::ollama::GPT_OSS_20B_CLOUD => Ok(ModelId::OllamaGptOss20bCloud),
            s if s == models::ollama::GPT_OSS_120B_CLOUD => Ok(ModelId::OllamaGptOss120bCloud),
            s if s == models::ollama::QWEN3_1_7B => Ok(ModelId::OllamaQwen317b),
            s if s == models::ollama::DEEPSEEK_V32_CLOUD => Ok(ModelId::OllamaDeepseekV32Cloud),
            s if s == models::ollama::QWEN3_NEXT_80B_CLOUD => Ok(ModelId::OllamaQwen3Next80bCloud),
            s if s == models::ollama::MISTRAL_LARGE_3_675B_CLOUD => {
                Ok(ModelId::OllamaMistralLarge3675bCloud)
            }
            s if s == models::ollama::KIMI_K2_THINKING_CLOUD => {
                Ok(ModelId::OllamaKimiK2ThinkingCloud)
            }

            s if s == models::ollama::QWEN3_CODER_480B_CLOUD => {
                Ok(ModelId::OllamaQwen3Coder480bCloud)
            }
            s if s == models::ollama::GLM_46_CLOUD => Ok(ModelId::OllamaGlm46Cloud),
            s if s == models::ollama::GLM_47_CLOUD => Ok(ModelId::OllamaGlm47Cloud),
            s if s == models::ollama::GEMINI_3_PRO_PREVIEW_LATEST_CLOUD => {
                Ok(ModelId::OllamaGemini3ProPreviewLatestCloud)
            }
            s if s == models::ollama::GEMINI_3_FLASH_PREVIEW_CLOUD => {
                Ok(ModelId::OllamaGemini3FlashPreviewCloud)
            }
            s if s == models::ollama::DEVSTRAL_2_123B_CLOUD => {
                Ok(ModelId::OllamaDevstral2123bCloud)
            }
            s if s == models::ollama::MINIMAX_M2_CLOUD => Ok(ModelId::OllamaMinimaxM2Cloud),
            s if s == models::ollama::MINIMAX_M21_CLOUD => Ok(ModelId::OllamaMinimaxM21Cloud),
            s if s == models::ollama::NEMOTRON_3_NANO_30B_CLOUD => {
                Ok(ModelId::OllamaNemotron3Nano30bCloud)
            }
            s if s == models::lmstudio::META_LLAMA_3_8B_INSTRUCT => {
                Ok(ModelId::LmStudioMetaLlama38BInstruct)
            }
            s if s == models::lmstudio::META_LLAMA_31_8B_INSTRUCT => {
                Ok(ModelId::LmStudioMetaLlama318BInstruct)
            }
            s if s == models::lmstudio::QWEN25_7B_INSTRUCT => Ok(ModelId::LmStudioQwen257BInstruct),
            s if s == models::lmstudio::GEMMA_2_2B_IT => Ok(ModelId::LmStudioGemma22BIt),
            s if s == models::lmstudio::GEMMA_2_9B_IT => Ok(ModelId::LmStudioGemma29BIt),
            s if s == models::lmstudio::PHI_31_MINI_4K_INSTRUCT => {
                Ok(ModelId::LmStudioPhi31Mini4kInstruct)
            }
            // MiniMax models
            s if s == models::minimax::MINIMAX_M2_1 => Ok(ModelId::MinimaxM21),
            s if s == models::minimax::MINIMAX_M2_1_LIGHTNING => Ok(ModelId::MinimaxM21Lightning),
            s if s == models::minimax::MINIMAX_M2 => Ok(ModelId::MinimaxM2),
            _ => Err(ModelParseError::InvalidModel(s.into())),
        }
    }
}
