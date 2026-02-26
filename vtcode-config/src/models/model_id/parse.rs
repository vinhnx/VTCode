use std::str::FromStr;

use crate::models::ModelParseError;

use super::ModelId;

impl FromStr for ModelId {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use crate::constants::models;

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
            s if s == models::GEMINI_3_FLASH_PREVIEW => Ok(ModelId::Gemini3FlashPreview),
            // OpenAI models
            s if s == models::GPT_5 => Ok(ModelId::GPT5),
            s if s == models::GPT_5_2 => Ok(ModelId::GPT52),
            s if s == models::GPT_5_MINI => Ok(ModelId::GPT5Mini),
            s if s == models::GPT_5_NANO => Ok(ModelId::GPT5Nano),
            s if s == models::openai::GPT_5_3_CODEX => Ok(ModelId::GPT53Codex),
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
            // DeepSeek models
            s if s == models::DEEPSEEK_CHAT => Ok(ModelId::DeepSeekChat),
            s if s == models::DEEPSEEK_REASONER => Ok(ModelId::DeepSeekReasoner),
            // xAI models
            s if s == models::xai::GROK_4 => Ok(ModelId::XaiGrok4),
            s if s == models::xai::GROK_4_MINI => Ok(ModelId::XaiGrok4Mini),
            s if s == models::xai::GROK_4_CODE => Ok(ModelId::XaiGrok4Code),
            s if s == models::xai::GROK_4_CODE_LATEST => Ok(ModelId::XaiGrok4CodeLatest),
            s if s == models::xai::GROK_4_VISION => Ok(ModelId::XaiGrok4Vision),
            // Z.AI models
            s if s == models::zai::GLM_5 || s == models::zai::GLM_5_LEGACY => Ok(ModelId::ZaiGlm5),
            // Moonshot models
            s if s == models::moonshot::MINIMAX_M2_5 => Ok(ModelId::MoonshotMinimaxM25),
            s if s == models::moonshot::QWEN3_CODER_NEXT => Ok(ModelId::MoonshotQwen3CoderNext),
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
            s if s == models::ollama::GEMINI_3_FLASH_PREVIEW_CLOUD => {
                Ok(ModelId::OllamaGemini3FlashPreviewCloud)
            }
            s if s == models::ollama::MINIMAX_M2_CLOUD => Ok(ModelId::OllamaMinimaxM2Cloud),
            s if s == models::ollama::MINIMAX_M25_CLOUD => Ok(ModelId::OllamaMinimaxM25Cloud),
            s if s == models::ollama::DEVSTRAL_2_123B_CLOUD => {
                Ok(ModelId::OllamaDevstral2123bCloud)
            }
            s if s == models::ollama::NEMOTRON_3_NANO_30B_CLOUD => {
                Ok(ModelId::OllamaNemotron3Nano30bCloud)
            }
            s if s == models::minimax::MINIMAX_M2_5 => Ok(ModelId::MinimaxM25),
            s if s == models::minimax::MINIMAX_M2 => Ok(ModelId::MinimaxM2),
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
            s if s == models::huggingface::QWEN3_5_397B_A17B_TOGETHER => {
                Ok(ModelId::HuggingFaceQwen35397BA17BTogether)
            }
            "minimax/minimax-m2.5" => Ok(ModelId::OpenRouterMinimaxM25),
            "qwen/qwen3-coder-next" => Ok(ModelId::OpenRouterQwen3CoderNext),
            _ => {
                if let Some(model) = Self::parse_openrouter_model(s) {
                    Ok(model)
                } else {
                    Err(ModelParseError::InvalidModel(s.to_string()))
                }
            }
        }
    }
}
