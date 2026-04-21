use std::str::FromStr;

use crate::models::ModelParseError;

use super::ModelId;

impl FromStr for ModelId {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use crate::constants::models;
        let trimmed = s.trim();

        // Explicitly handle built-in models that might be shadowed by OpenRouter
        if trimmed == models::zai::GLM_5 || trimmed == models::zai::GLM_5_LEGACY {
            return Ok(ModelId::ZaiGlm5);
        }
        if trimmed == models::zai::GLM_5_1 {
            return Ok(ModelId::ZaiGlm51);
        }

        if let Some(opencode_model) = trimmed
            .strip_prefix("opencode/")
            .or_else(|| trimmed.strip_prefix("opencode-zen/"))
        {
            return match opencode_model {
                m if m == models::opencode_zen::GPT_5_4 => Ok(ModelId::OpenCodeZenGPT54),
                m if m == models::opencode_zen::GPT_5_4_MINI => Ok(ModelId::OpenCodeZenGPT54Mini),
                m if m == models::opencode_zen::CLAUDE_SONNET_4_6 => {
                    Ok(ModelId::OpenCodeZenClaudeSonnet46)
                }
                m if m == models::opencode_zen::GLM_5_1 => Ok(ModelId::OpenCodeZenGlm51),
                m if m == models::opencode_zen::KIMI_K2_5 => Ok(ModelId::OpenCodeZenKimiK25),
                _ => Err(ModelParseError::InvalidModel(trimmed.to_string())),
            };
        }

        if let Some(opencode_model) = trimmed.strip_prefix("opencode-go/") {
            return match opencode_model {
                m if m == models::opencode_go::GLM_5_1 => Ok(ModelId::OpenCodeGoGlm51),
                m if m == models::opencode_go::KIMI_K2_5 => Ok(ModelId::OpenCodeGoKimiK25),
                m if m == models::opencode_go::MINIMAX_M2_5 => Ok(ModelId::OpenCodeGoMinimaxM25),
                m if m == models::opencode_go::MINIMAX_M2_7 => Ok(ModelId::OpenCodeGoMinimaxM27),
                _ => Err(ModelParseError::InvalidModel(trimmed.to_string())),
            };
        }

        if let Some(model) = Self::parse_openrouter_model(trimmed) {
            return Ok(model);
        }

        match trimmed {
            // Gemini models
            s if s == models::GEMINI_3_1_PRO_PREVIEW => Ok(ModelId::Gemini31ProPreview),
            s if s == models::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS => {
                Ok(ModelId::Gemini31ProPreviewCustomTools)
            }
            s if s == models::GEMINI_3_1_FLASH_LITE_PREVIEW => {
                Ok(ModelId::Gemini31FlashLitePreview)
            }
            s if s == models::GEMINI_3_FLASH_PREVIEW => Ok(ModelId::Gemini3FlashPreview),
            // OpenAI models
            s if s == models::GPT => Ok(ModelId::GPT54),
            s if s == models::GPT_5 => Ok(ModelId::GPT5),
            s if s == models::GPT_5_2 => Ok(ModelId::GPT52),
            s if s == models::openai::GPT_5_2_CODEX => Ok(ModelId::GPT52Codex),
            s if s == models::GPT_5_4 => Ok(ModelId::GPT54),
            s if s == models::GPT_5_4_PRO => Ok(ModelId::GPT54Pro),
            s if s == models::openai::GPT_5_4_NANO => Ok(ModelId::GPT54Nano),
            s if s == models::openai::GPT_5_4_MINI => Ok(ModelId::GPT54Mini),
            s if s == models::openai::GPT_5_3_CODEX => Ok(ModelId::GPT53Codex),
            s if s == models::openai::GPT_5_1_CODEX => Ok(ModelId::GPT51Codex),
            s if s == models::openai::GPT_5_1_CODEX_MAX => Ok(ModelId::GPT51CodexMax),
            s if s == models::GPT_5_MINI => Ok(ModelId::GPT5Mini),
            s if s == models::GPT_5_NANO => Ok(ModelId::GPT5Nano),
            s if s == models::openai::GPT_5_CODEX => Ok(ModelId::GPT5Codex),
            s if s == models::openai::GPT_OSS_20B => Ok(ModelId::OpenAIGptOss20b),
            s if s == models::openai::GPT_OSS_120B => Ok(ModelId::OpenAIGptOss120b),
            // Anthropic models
            s if s == models::CLAUDE_OPUS_4_7 => Ok(ModelId::ClaudeOpus47),
            s if s == models::CLAUDE_OPUS_4_6 => Ok(ModelId::ClaudeOpus46),
            s if s == models::CLAUDE_SONNET_4_6 => Ok(ModelId::ClaudeSonnet46),
            s if s == models::CLAUDE_HAIKU_4_5 => Ok(ModelId::ClaudeHaiku45),
            s if s == models::CLAUDE_HAIKU_4_5_20251001 => Ok(ModelId::ClaudeHaiku45),
            s if s == models::CLAUDE_MYTHOS_PREVIEW => Ok(ModelId::ClaudeMythosPreview),
            s if s == models::copilot::AUTO => Ok(ModelId::CopilotAuto),
            s if s == models::copilot::GPT_5_2_CODEX => Ok(ModelId::CopilotGPT52Codex),
            s if s == models::copilot::GPT_5_1_CODEX_MAX => Ok(ModelId::CopilotGPT51CodexMax),
            s if s == models::copilot::GPT_5_4 => Ok(ModelId::CopilotGPT54),
            s if s == models::copilot::GPT_5_4_MINI => Ok(ModelId::CopilotGPT54Mini),
            s if s == models::copilot::CLAUDE_SONNET_4_6 => Ok(ModelId::CopilotClaudeSonnet46),
            // DeepSeek models
            s if s == models::DEEPSEEK_CHAT => Ok(ModelId::DeepSeekChat),
            s if s == models::DEEPSEEK_REASONER => Ok(ModelId::DeepSeekReasoner),
            // Z.AI models
            s if s == models::zai::GLM_5 || s == models::zai::GLM_5_LEGACY => Ok(ModelId::ZaiGlm5),
            s if s == models::zai::GLM_5_1 => Ok(ModelId::ZaiGlm51),
            // Moonshot models
            s if s == models::moonshot::KIMI_K2_6 => Ok(ModelId::MoonshotKimiK26),
            s if s == models::moonshot::KIMI_K2_5 => Ok(ModelId::MoonshotKimiK25),
            s if s == models::ollama::GPT_OSS_20B => Ok(ModelId::OllamaGptOss20b),
            s if s == models::ollama::GPT_OSS_20B_CLOUD => Ok(ModelId::OllamaGptOss20bCloud),
            s if s == models::ollama::GPT_OSS_120B_CLOUD => Ok(ModelId::OllamaGptOss120bCloud),
            s if s == models::ollama::QWEN3_1_7B => Ok(ModelId::OllamaQwen317b),
            s if s == models::ollama::QWEN3_CODER_NEXT => Ok(ModelId::OllamaQwen3CoderNext),
            "qwen3-coder-next" => Ok(ModelId::OllamaQwen3CoderNext),
            s if s == models::ollama::DEEPSEEK_V32_CLOUD => Ok(ModelId::OllamaDeepseekV32Cloud),
            s if s == models::ollama::QWEN3_NEXT_80B_CLOUD => Ok(ModelId::OllamaQwen3Next80bCloud),
            s if s == models::ollama::GLM_5_CLOUD => Ok(ModelId::OllamaGlm5Cloud),
            s if s == models::ollama::GLM_5_1_CLOUD => Ok(ModelId::OllamaGlm51Cloud),
            s if s == models::ollama::GEMINI_3_FLASH_PREVIEW_CLOUD => {
                Ok(ModelId::OllamaGemini3FlashPreviewCloud)
            }
            s if s == models::ollama::MINIMAX_M2_CLOUD => Ok(ModelId::OllamaMinimaxM2Cloud),
            s if s == models::ollama::MINIMAX_M27_CLOUD => Ok(ModelId::OllamaMinimaxM27Cloud),
            s if s == models::ollama::MINIMAX_M25_CLOUD => Ok(ModelId::OllamaMinimaxM25Cloud),
            s if s == models::ollama::KIMI_K2_6_CLOUD => Ok(ModelId::OllamaKimiK26Cloud),
            s if s == models::ollama::NEMOTRON_3_SUPER_CLOUD => {
                Ok(ModelId::OllamaNemotron3SuperCloud)
            }
            s if s == models::minimax::MINIMAX_M2_7 => Ok(ModelId::MinimaxM27),
            s if s == models::minimax::MINIMAX_M2_5 => Ok(ModelId::MinimaxM25),
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
            s if s == models::huggingface::ZAI_GLM_5_1_ZAI_ORG => {
                Ok(ModelId::HuggingFaceGlm51ZaiOrg)
            }
            s if s == models::huggingface::QWEN3_CODER_NEXT_NOVITA => {
                Ok(ModelId::HuggingFaceQwen3CoderNextNovita)
            }
            s if s == models::huggingface::QWEN3_5_397B_A17B_TOGETHER => {
                Ok(ModelId::HuggingFaceQwen35397BA17BTogether)
            }
            s if s == models::huggingface::KIMI_K2_6_NOVITA => {
                Ok(ModelId::HuggingFaceKimiK26Novita)
            }
            s if s == models::huggingface::STEP_3_5_FLASH
                || s == models::huggingface::STEP_3_5_FLASH_BASE
                || s == models::huggingface::STEP_3_5_FLASH_LEGACY_FASTEST =>
            {
                Ok(ModelId::HuggingFaceStep35Flash)
            }
            "minimax/minimax-m2.5" => Ok(ModelId::OpenRouterMinimaxM25),
            "qwen/qwen3-coder-next" => Ok(ModelId::OpenRouterQwen3CoderNext),
            "moonshotai/kimi-k2.6" => Ok(ModelId::OpenRouterMoonshotaiKimiK26),
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
