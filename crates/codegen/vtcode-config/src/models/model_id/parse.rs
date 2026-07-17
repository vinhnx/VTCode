use std::str::FromStr;

use crate::models::ModelParseError;

use super::ModelId;

impl FromStr for ModelId {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use crate::constants::models;
        let trimmed = s.trim();

        // Explicitly handle built-in models that might be shadowed by OpenRouter
        if trimmed == models::zai::GLM_5_1 {
            return Ok(ModelId::ZaiGlm51);
        }

        if trimmed == models::zai::GLM_5_2 {
            return Ok(ModelId::ZaiGlm52);
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
                m if m == models::opencode_go::GLM_5_2 => Ok(ModelId::OpenCodeGoGlm52),
                m if m == models::opencode_go::GLM_5_1 => Ok(ModelId::OpenCodeGoGlm51),
                m if m == models::opencode_go::KIMI_K2_7_CODE => Ok(ModelId::OpenCodeGoKimiK27Code),
                m if m == models::opencode_go::KIMI_K2_6 => Ok(ModelId::OpenCodeGoKimiK26),
                m if m == models::opencode_go::MIMO_V2_5 => Ok(ModelId::OpenCodeGoMimoV25),
                m if m == models::opencode_go::MIMO_V2_5_PRO => Ok(ModelId::OpenCodeGoMimoV25Pro),
                m if m == models::opencode_go::MINIMAX_M3 => Ok(ModelId::OpenCodeGoMinimaxM3),
                m if m == models::opencode_go::MINIMAX_M2_7 => Ok(ModelId::OpenCodeGoMinimaxM27),
                m if m == models::opencode_go::QWEN_3_7_MAX => Ok(ModelId::OpenCodeGoQwen37Max),
                m if m == models::opencode_go::QWEN_3_7_PLUS => Ok(ModelId::OpenCodeGoQwen37Plus),
                m if m == models::opencode_go::QWEN_3_6_PLUS => Ok(ModelId::OpenCodeGoQwen36Plus),
                m if m == models::opencode_go::DEEPSEEK_V4_PRO => {
                    Ok(ModelId::OpenCodeGoDeepseekV4Pro)
                }
                m if m == models::opencode_go::DEEPSEEK_V4_FLASH => {
                    Ok(ModelId::OpenCodeGoDeepseekV4Flash)
                }
                _ => Err(ModelParseError::InvalidModel(trimmed.to_string())),
            };
        }

        if let Some(model) = Self::parse_openrouter_model(trimmed) {
            return Ok(model);
        }

        if let Some(model) = Self::parse_table(trimmed) {
            return Ok(model);
        }

        match trimmed {
            // OpenRouter models without generated metadata
            "moonshotai/kimi-k3" => Ok(ModelId::OpenRouterMoonshotaiKimiK3),
            "moonshotai/kimi-k2.6" => Ok(ModelId::OpenRouterMoonshotaiKimiK26),
            "moonshotai/kimi-k2.7-code" => Ok(ModelId::OpenRouterMoonshotaiKimiK27Code),
            "z-ai/glm-5.1" => Ok(ModelId::OpenRouterZaiGlm51),
            "z-ai/glm-5.2" => Ok(ModelId::OpenRouterZaiGlm52),
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
