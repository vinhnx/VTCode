//! Provider metadata and enums for model configuration.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::{ModelId, ModelParseError};

#[derive(Clone, Copy)]
pub struct OpenRouterMetadata {
    pub(crate) id: &'static str,
    pub(crate) vendor: &'static str,
    pub(crate) display: &'static str,
    pub(crate) description: &'static str,
    pub(crate) efficient: bool,
    pub(crate) top_tier: bool,
    pub(crate) generation: &'static str,
    pub(crate) reasoning: bool,
    pub(crate) tool_call: bool,
}

/// Supported AI model providers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Provider {
    /// Google Gemini models
    #[default]
    Gemini,
    /// OpenAI GPT models
    OpenAI,
    /// Anthropic Claude models
    Anthropic,
    /// DeepSeek native models
    DeepSeek,
    /// Hugging Face Inference Providers
    HuggingFace,
    /// OpenRouter marketplace models
    OpenRouter,
    /// Local Ollama models
    Ollama,
    /// LM Studio local server (OpenAI-compatible)
    LmStudio,
    /// Moonshot.ai models
    Moonshot,
    /// xAI Grok models
    XAI,
    /// Z.AI GLM models
    ZAI,
    /// MiniMax models
    Minimax,
}

impl Provider {
    /// Get the default API key environment variable for this provider
    pub fn default_api_key_env(&self) -> &'static str {
        match self {
            Provider::Gemini => "GEMINI_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::HuggingFace => "HF_TOKEN",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::Ollama => "OLLAMA_API_KEY",
            Provider::LmStudio => "LMSTUDIO_API_KEY",
            Provider::Moonshot => "MOONSHOT_API_KEY",
            Provider::XAI => "XAI_API_KEY",
            Provider::ZAI => "ZAI_API_KEY",
            Provider::Minimax => "MINIMAX_API_KEY",
        }
    }

    /// Get all supported providers
    pub fn all_providers() -> Vec<Provider> {
        vec![
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Minimax,
            Provider::Gemini,
            Provider::DeepSeek,
            Provider::HuggingFace,
            Provider::OpenRouter,
            Provider::Ollama,
            Provider::LmStudio,
            Provider::Moonshot,
            Provider::XAI,
            Provider::ZAI,
        ]
    }

    /// Human-friendly label for display purposes
    pub fn label(&self) -> &'static str {
        match self {
            Provider::Gemini => "Gemini",
            Provider::OpenAI => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::DeepSeek => "DeepSeek",
            Provider::HuggingFace => "Hugging Face",
            Provider::OpenRouter => "OpenRouter",
            Provider::Ollama => "Ollama",
            Provider::LmStudio => "LM Studio",
            Provider::Moonshot => "Moonshot",
            Provider::XAI => "xAI",
            Provider::ZAI => "Z.AI",
            Provider::Minimax => "MiniMax",
        }
    }

    /// Determine if the provider supports configurable reasoning effort for the model
    pub fn supports_reasoning_effort(&self, model: &str) -> bool {
        use crate::config::constants::models;

        match self {
            Provider::Gemini => models::google::REASONING_MODELS.contains(&model),
            Provider::OpenAI => models::openai::REASONING_MODELS.contains(&model),
            Provider::Anthropic => models::anthropic::REASONING_MODELS.contains(&model),
            Provider::DeepSeek => model == models::deepseek::DEEPSEEK_REASONER,
            Provider::HuggingFace => models::huggingface::REASONING_MODELS.contains(&model),
            Provider::OpenRouter => {
                if let Ok(model_id) = ModelId::from_str(model) {
                    return model_id.is_reasoning_variant();
                }
                models::openrouter::REASONING_MODELS.contains(&model)
            }
            Provider::Ollama => models::ollama::REASONING_LEVEL_MODELS.contains(&model),
            Provider::LmStudio => false,
            Provider::Moonshot => models::moonshot::REASONING_MODELS.contains(&model),
            Provider::XAI => model == models::xai::GROK_4 || model == models::xai::GROK_4_CODE,
            Provider::ZAI => model == models::zai::GLM_4_6 || model == models::zai::GLM_4_7,
            Provider::Minimax => {
                model == models::minimax::MINIMAX_M2_1
                    || model == models::minimax::MINIMAX_M2_1_LIGHTNING
                    || model == models::minimax::MINIMAX_M2
            }
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Provider::Gemini => write!(f, "gemini"),
            Provider::OpenAI => write!(f, "openai"),
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::DeepSeek => write!(f, "deepseek"),
            Provider::HuggingFace => write!(f, "huggingface"),
            Provider::OpenRouter => write!(f, "openrouter"),
            Provider::Ollama => write!(f, "ollama"),
            Provider::LmStudio => write!(f, "lmstudio"),
            Provider::Moonshot => write!(f, "moonshot"),
            Provider::XAI => write!(f, "xai"),
            Provider::ZAI => write!(f, "zai"),
            Provider::Minimax => write!(f, "minimax"),
        }
    }
}

impl FromStr for Provider {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "gemini" => Ok(Provider::Gemini),
            "openai" => Ok(Provider::OpenAI),
            "anthropic" => Ok(Provider::Anthropic),
            "deepseek" => Ok(Provider::DeepSeek),
            "huggingface" => Ok(Provider::HuggingFace),
            "openrouter" => Ok(Provider::OpenRouter),
            "ollama" => Ok(Provider::Ollama),
            "lmstudio" => Ok(Provider::LmStudio),
            "moonshot" => Ok(Provider::Moonshot),
            "xai" => Ok(Provider::XAI),
            "zai" => Ok(Provider::ZAI),
            "minimax" => Ok(Provider::Minimax),
            _ => Err(ModelParseError::InvalidProvider(s.into())),
        }
    }
}
