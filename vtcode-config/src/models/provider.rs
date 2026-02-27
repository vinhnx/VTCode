use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::{ModelId, ModelParseError};

/// Supported AI model providers
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// OpenRouter marketplace models
    OpenRouter,
    /// Local Ollama models
    Ollama,
    /// Moonshot.ai models
    Moonshot,
    /// Z.AI GLM models
    ZAI,
    /// MiniMax models
    Minimax,
    /// Hugging Face Inference Providers
    HuggingFace,
}

impl Provider {
    /// Get the default API key environment variable for this provider
    pub fn default_api_key_env(&self) -> &'static str {
        match self {
            Provider::Gemini => "GEMINI_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::Ollama => "OLLAMA_API_KEY",
            Provider::Moonshot => "MOONSHOT_API_KEY",
            Provider::ZAI => "ZAI_API_KEY",
            Provider::Minimax => "MINIMAX_API_KEY",
            Provider::HuggingFace => "HF_TOKEN",
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
            Provider::Moonshot,
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
            Provider::OpenRouter => "OpenRouter",
            Provider::Ollama => "Ollama",
            Provider::Moonshot => "Moonshot",
            Provider::ZAI => "Z.AI",
            Provider::Minimax => "MiniMax",
            Provider::HuggingFace => "Hugging Face",
        }
    }

    pub fn is_dynamic(&self) -> bool {
        self.is_local()
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Provider::Ollama)
    }

    pub fn local_install_instructions(&self) -> Option<&'static str> {
        match self {
            Provider::Ollama => Some(
                "Ollama server is not running. To start:\n  1. Install Ollama from https://ollama.com\n  2. Run 'ollama serve' in a terminal\n  3. Pull models using 'ollama pull <model-name>' (e.g., 'ollama pull gpt-oss:20b')",
            ),
            _ => None,
        }
    }

    /// Determine if the provider supports configurable reasoning effort for the model
    pub fn supports_reasoning_effort(&self, model: &str) -> bool {
        use crate::constants::models;

        match self {
            Provider::Gemini => models::google::REASONING_MODELS.contains(&model),
            Provider::OpenAI => models::openai::REASONING_MODELS.contains(&model),
            Provider::Anthropic => models::anthropic::REASONING_MODELS.contains(&model),
            Provider::DeepSeek => model == models::deepseek::DEEPSEEK_REASONER,
            Provider::OpenRouter => {
                if let Ok(model_id) = ModelId::from_str(model) {
                    if let Some(meta) = crate::models::openrouter_generated::metadata_for(model_id)
                    {
                        return meta.reasoning;
                    }
                    return matches!(
                        model_id,
                        ModelId::OpenRouterMinimaxM25 | ModelId::OpenRouterQwen3CoderNext
                    );
                }
                models::openrouter::REASONING_MODELS.contains(&model)
            }
            Provider::Ollama => models::ollama::REASONING_LEVEL_MODELS.contains(&model),
            Provider::Moonshot => models::moonshot::REASONING_MODELS.contains(&model),
            Provider::ZAI => models::zai::REASONING_MODELS.contains(&model),
            Provider::Minimax => {
                model == models::minimax::MINIMAX_M2_5 || model == models::minimax::MINIMAX_M2
            }
            Provider::HuggingFace => models::huggingface::REASONING_MODELS.contains(&model),
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
            Provider::OpenRouter => write!(f, "openrouter"),
            Provider::Ollama => write!(f, "ollama"),
            Provider::Moonshot => write!(f, "moonshot"),
            Provider::ZAI => write!(f, "zai"),
            Provider::Minimax => write!(f, "minimax"),
            Provider::HuggingFace => write!(f, "huggingface"),
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
            "openrouter" => Ok(Provider::OpenRouter),
            "ollama" => Ok(Provider::Ollama),
            "moonshot" => Ok(Provider::Moonshot),
            "zai" => Ok(Provider::ZAI),
            "minimax" => Ok(Provider::Minimax),
            "huggingface" => Ok(Provider::HuggingFace),
            _ => Err(ModelParseError::InvalidProvider(s.to_string())),
        }
    }
}
