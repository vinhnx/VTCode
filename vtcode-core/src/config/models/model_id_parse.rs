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

        // Explicitly handle built-in models that might be shadowed by OpenRouter
        if s == models::zai::GLM_5 || s == models::zai::GLM_5_LEGACY {
            return Ok(ModelId::ZaiGlm5);
        }

        if let Some(model) = Self::parse_openrouter_model(s) {
            return Ok(model);
        }

        match s {
            // Gemini models
            s if s == models::GEMINI_3_1_PRO_PREVIEW => Ok(ModelId::Gemini31ProPreview),
            s if s == models::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS => {
                Ok(ModelId::Gemini31ProPreviewCustomTools)
            }
            s if s == models::GEMINI_3_PRO_PREVIEW => Ok(ModelId::Gemini3ProPreview),
            s if s == models::GEMINI_3_FLASH_PREVIEW => Ok(ModelId::Gemini3FlashPreview),
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
            s if s == models::CLAUDE_OPUS_4_6 => Ok(ModelId::ClaudeOpus46),
            s if s == models::CLAUDE_SONNET_4_6 => Ok(ModelId::ClaudeSonnet46),
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
            s if s == models::huggingface::MINIMAX_M2_5_NOVITA => {
                Ok(ModelId::HuggingFaceMinimaxM25Novita)
            }
            s if s == models::huggingface::DEEPSEEK_V32_NOVITA => {
                Ok(ModelId::HuggingFaceDeepseekV32Novita)
            }
            s if s == models::huggingface::XIAOMI_MIMO_V2_FLASH_NOVITA => {
                Ok(ModelId::HuggingFaceXiaomiMimoV2FlashNovita)
            }
            s if s == models::huggingface::ZAI_GLM_5_NOVITA => Ok(ModelId::HuggingFaceGlm5Novita),
            s if s == models::huggingface::QWEN3_CODER_NEXT_NOVITA => {
                Ok(ModelId::HuggingFaceQwen3CoderNextNovita)
            }
            // xAI models
            s if s == models::xai::GROK_4 => Ok(ModelId::XaiGrok4),
            s if s == models::xai::GROK_4_MINI => Ok(ModelId::XaiGrok4Mini),
            s if s == models::xai::GROK_4_CODE => Ok(ModelId::XaiGrok4Code),
            s if s == models::xai::GROK_4_CODE_LATEST => Ok(ModelId::XaiGrok4CodeLatest),
            s if s == models::xai::GROK_4_VISION => Ok(ModelId::XaiGrok4Vision),
            // Z.AI models
            s if s == models::zai::GLM_5 || s == models::zai::GLM_5_LEGACY => {
                Ok(ModelId::ZaiGlm5)
            }
            // Moonshot models
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
            s if s == models::ollama::QWEN3_CODER_480B_CLOUD => {
                Ok(ModelId::OllamaQwen3Coder480bCloud)
            }
            s if s == models::ollama::GLM_5_CLOUD => Ok(ModelId::OllamaGlm5Cloud),
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
            s if s == models::ollama::MINIMAX_M25_CLOUD => Ok(ModelId::OllamaMinimaxM25Cloud),
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
            s if s == models::minimax::MINIMAX_M2_5 => Ok(ModelId::MinimaxM25),
            s if s == models::minimax::MINIMAX_M2 => Ok(ModelId::MinimaxM2),
            _ => Err(ModelParseError::InvalidModel(s.into())),
        }
    }
}
