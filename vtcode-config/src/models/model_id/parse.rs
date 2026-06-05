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
                _ => Err(ModelParseError::InvalidModel(trimmed.to_string())),
            };
        }

        if let Some(opencode_model) = trimmed.strip_prefix("opencode-go/") {
            return match opencode_model {
                m if m == models::opencode_go::GLM_5_1 => Ok(ModelId::OpenCodeGoGlm51),
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
            s if s == models::GEMINI_3_5_FLASH || s == models::GEMINI_3_FLASH_PREVIEW => {
                Ok(ModelId::Gemini35Flash)
            }
            // OpenAI models
            s if s == models::GPT => Ok(ModelId::GPT54),
            s if s == models::openai::GPT_5_5 || s == models::openai::GPT_5_5_DATED => {
                Ok(ModelId::GPT55)
            }
            s if s == models::GPT_5_4 => Ok(ModelId::GPT54),
            s if s == models::GPT_5_4_PRO => Ok(ModelId::GPT54Pro),
            s if s == models::openai::GPT_5_4_NANO => Ok(ModelId::GPT54Nano),
            s if s == models::openai::GPT_5_4_MINI => Ok(ModelId::GPT54Mini),
            s if s == models::openai::GPT_5_3_CODEX => Ok(ModelId::GPT53Codex),
            s if s == models::openai::GPT_OSS_20B => Ok(ModelId::OpenAIGptOss20b),
            s if s == models::openai::GPT_OSS_120B => Ok(ModelId::OpenAIGptOss120b),
            // Anthropic models
            s if s == models::CLAUDE_OPUS_4_8 => Ok(ModelId::ClaudeOpus48),
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
            s if s == models::deepseek::DEEPSEEK_V4_PRO => Ok(ModelId::DeepSeekV4Pro),
            s if s == models::deepseek::DEEPSEEK_V4_FLASH => Ok(ModelId::DeepSeekV4Flash),
            // Mistral models
            s if s == models::mistral::MISTRAL_LARGE_3 => Ok(ModelId::MistralLarge3),
            // MiMo models
            s if s == models::mimo::MIMO_V2_5_PRO => Ok(ModelId::MiMoV25Pro),
            s if s == models::mimo::MIMO_V2_5 => Ok(ModelId::MiMoV25),
            // Qwen models
            s if s == models::qwen::QWEN3_7_MAX => Ok(ModelId::Qwen37Max),
            s if s == models::qwen::QWEN3_6_FLASH => Ok(ModelId::Qwen36Flash),
            s if s == models::qwen::QWEN3_6_PLUS => Ok(ModelId::Qwen36Plus),
            // Note: deepseek-v4-flash, deepseek-v4-pro, glm-5.1 resolve to their native
            // variants (DeepSeekV4Flash, DeepSeekV4Pro, ZaiGlm51) via the matches above.
            // The Qwen-prefixed variants (QwenDeepSeekV4Flash, etc.) are picker-only entries.
            // Z.AI models
            s if s == models::zai::GLM_5 || s == models::zai::GLM_5_LEGACY => Ok(ModelId::ZaiGlm5),
            s if s == models::zai::GLM_5_1 => Ok(ModelId::ZaiGlm51),
            // Moonshot models
            s if s == models::moonshot::KIMI_K2_6 => Ok(ModelId::MoonshotKimiK26),
            s if s == models::ollama::GPT_OSS_20B => Ok(ModelId::OllamaGptOss20b),
            s if s == models::ollama::GPT_OSS_20B_CLOUD => Ok(ModelId::OllamaGptOss20bCloud),
            s if s == models::ollama::GPT_OSS_120B_CLOUD => Ok(ModelId::OllamaGptOss120bCloud),
            s if s == models::ollama::QWEN3_1_7B => Ok(ModelId::OllamaQwen317b),
            s if s == models::ollama::QWEN3_CODER_NEXT => Ok(ModelId::OllamaQwen3CoderNext),
            "qwen3-coder-next" => Ok(ModelId::OllamaQwen3CoderNext),
            s if s == models::ollama::DEEPSEEK_V4_FLASH_CLOUD => {
                Ok(ModelId::OllamaDeepseekV4FlashCloud)
            }
            s if s == models::ollama::DEEPSEEK_V4_PRO_CLOUD => {
                Ok(ModelId::OllamaDeepseekV4ProCloud)
            }
            s if s == models::ollama::QWEN3_NEXT_80B_CLOUD => Ok(ModelId::OllamaQwen3Next80bCloud),
            s if s == models::ollama::GLM_5_CLOUD => Ok(ModelId::OllamaGlm5Cloud),
            s if s == models::ollama::GLM_5_1_CLOUD => Ok(ModelId::OllamaGlm51Cloud),
            s if s == models::ollama::GEMINI_3_FLASH_PREVIEW_CLOUD => {
                Ok(ModelId::OllamaGemini3FlashPreviewCloud)
            }
            s if s == models::ollama::MINIMAX_M2_CLOUD => Ok(ModelId::OllamaMinimaxM2Cloud),
            s if s == models::ollama::MINIMAX_M27_CLOUD => Ok(ModelId::OllamaMinimaxM27Cloud),
            s if s == models::ollama::MINIMAX_M3_CLOUD => Ok(ModelId::OllamaMinimaxM3Cloud),
            s if s == models::ollama::MINIMAX_M25_CLOUD => Ok(ModelId::OllamaMinimaxM25Cloud),
            s if s == models::ollama::KIMI_K2_6_CLOUD => Ok(ModelId::OllamaKimiK26Cloud),
            s if s == models::ollama::NEMOTRON_3_SUPER_CLOUD => {
                Ok(ModelId::OllamaNemotron3SuperCloud)
            }
            s if s == models::ollama::LAGUNA_XS_2 => Ok(ModelId::OllamaLagunaXs2),
            s if s == models::llamacpp::QWEN36_27B => Ok(ModelId::LlamaCppQwen3627b),
            s if s == models::llamacpp::QWEN36_35B_A3B => Ok(ModelId::LlamaCppQwen3635bA3b),
            s if s == models::llamacpp::GEMMA_4_26B_A4B => Ok(ModelId::LlamaCppGemma426bA4b),
            s if s == models::llamacpp::GEMMA_4_E4B => Ok(ModelId::LlamaCppGemma4E4b),
            s if s == models::llamacpp::GPT_OSS_20B => Ok(ModelId::LlamaCppGptOss20b),
            s if s == models::llamacpp::STEP_3_5_FLASH => Ok(ModelId::LlamaCppStep35Flash),
            // Poolside models
            s if s == models::poolside::LAGUNA_M1 => Ok(ModelId::PoolsideLagunaM1),
            s if s == models::poolside::LAGUNA_XS2 => Ok(ModelId::PoolsideLagunaXs2),
            s if s == models::minimax::MINIMAX_M3 => Ok(ModelId::MinimaxM3),
            s if s == models::minimax::MINIMAX_M2_7 => Ok(ModelId::MinimaxM27),
            s if s == models::minimax::MINIMAX_M2_5 => Ok(ModelId::MinimaxM25),
            // Hugging Face models
            s if s == models::huggingface::OPENAI_GPT_OSS_20B => {
                Ok(ModelId::HuggingFaceOpenAIGptOss20b)
            }
            s if s == models::huggingface::OPENAI_GPT_OSS_120B => {
                Ok(ModelId::HuggingFaceOpenAIGptOss120b)
            }
            s if s == models::huggingface::MINIMAX_M2_5_NOVITA => {
                Ok(ModelId::HuggingFaceMinimaxM25Novita)
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
            s if s == models::huggingface::DEEPSEEK_V4_FLASH_NOVITA => {
                Ok(ModelId::HuggingFaceDeepseekV4FlashNovita)
            }
            s if s == models::huggingface::DEEPSEEK_V4_PRO_TOGETHER => {
                Ok(ModelId::HuggingFaceDeepseekV4ProTogether)
            }
            s if s == models::huggingface::STEP_3_5_FLASH
                || s == models::huggingface::STEP_3_5_FLASH_BASE
                || s == models::huggingface::STEP_3_5_FLASH_LEGACY_FASTEST =>
            {
                Ok(ModelId::HuggingFaceStep35Flash)
            }
            s if s == models::huggingface::ZAI_GLM_5_1_DEEPINFRA => {
                Ok(ModelId::HuggingFaceGlm51Deepinfra)
            }
            s if s == models::huggingface::MINIMAX_M2_7_NOVITA => {
                Ok(ModelId::HuggingFaceMinimaxM27Novita)
            }
            s if s == models::huggingface::DEEPSEEK_V4_PRO_NOVITA => {
                Ok(ModelId::HuggingFaceDeepseekV4ProNovita)
            }
            s if s == models::stepfun::STEP_3_7_FLASH => Ok(ModelId::StepFun37Flash),
            "evolink/gpt-5.2" => Ok(ModelId::EvolinkGpt52),
            "evolink/gpt-5.5" => Ok(ModelId::EvolinkGpt55),
            "evolink/deepseek-v4-pro" => Ok(ModelId::EvolinkDeepseekV4Pro),
            "evolink/deepseek-v4-flash" => Ok(ModelId::EvolinkDeepseekV4Flash),
            "evolink/doubao-seed-2.0-pro" => Ok(ModelId::EvolinkDoubaoSeed20Pro),
            "evolink/gemini-3.1-pro-preview" => Ok(ModelId::EvolinkGemini31Pro),
            "evolink/gemini-3.5-flash" => Ok(ModelId::EvolinkGemini35Flash),
            "evolink/MiniMax-M3" => Ok(ModelId::EvolinkMinimaxM3),
            "evolink/claude-sonnet-4-6" => Ok(ModelId::EvolinkClaudeSonnet46),
            "evolink/claude-opus-4-8" => Ok(ModelId::EvolinkClaudeOpus48),
            "evolink/claude-haiku-4-5-20251001" => Ok(ModelId::EvolinkClaudeHaiku45),
            "minimax/minimax-m2.5" => Ok(ModelId::OpenRouterMinimaxM25),
            "qwen/qwen3-coder-next" => Ok(ModelId::OpenRouterQwen3CoderNext),
            "moonshotai/kimi-k2.6" => Ok(ModelId::OpenRouterMoonshotaiKimiK26),
            "z-ai/glm-5.1" => Ok(ModelId::OpenRouterZaiGlm51),
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
