//! Model configuration and identification module
//!
//! This module provides a centralized enum for model identifiers and their configurations,
//! replacing hardcoded model strings throughout the codebase for better maintainability.
//! Read the model list in `docs/models.json`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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
    /// OpenRouter marketplace models
    OpenRouter,
    /// xAI Grok models
    XAI,
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
            Provider::XAI => "XAI_API_KEY",
        }
    }

    /// Get all supported providers
    pub fn all_providers() -> Vec<Provider> {
        vec![
            Provider::Gemini,
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::DeepSeek,
            Provider::OpenRouter,
            Provider::XAI,
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
            Provider::XAI => "xAI",
        }
    }

    /// Determine if the provider supports configurable reasoning effort for the model
    pub fn supports_reasoning_effort(&self, model: &str) -> bool {
        use crate::config::constants::models;

        match self {
            Provider::Gemini => model == models::google::GEMINI_2_5_PRO,
            Provider::OpenAI => models::openai::REASONING_MODELS.contains(&model),
            Provider::Anthropic => models::anthropic::SUPPORTED_MODELS.contains(&model),
            Provider::DeepSeek => model == models::deepseek::DEEPSEEK_REASONER,
            Provider::OpenRouter => models::openrouter::REASONING_MODELS.contains(&model),
            Provider::XAI => model == models::xai::GROK_2_REASONING,
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
            Provider::XAI => write!(f, "xai"),
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
            "xai" => Ok(Provider::XAI),
            _ => Err(ModelParseError::InvalidProvider(s.to_string())),
        }
    }
}

/// Centralized enum for all supported model identifiers
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelId {
    // Gemini models
    /// Gemini 2.5 Flash Preview - Latest fast model with advanced capabilities
    Gemini25FlashPreview,
    /// Gemini 2.5 Flash - Legacy alias for flash preview
    Gemini25Flash,
    /// Gemini 2.5 Flash Lite - Legacy alias for flash preview (lite)
    Gemini25FlashLite,
    /// Gemini 2.5 Pro - Latest most capable Gemini model
    Gemini25Pro,

    // OpenAI models
    /// GPT-5 - Latest most capable OpenAI model (2025-08-07)
    GPT5,
    /// GPT-5 Codex - Code-focused GPT-5 variant using the Responses API
    GPT5Codex,
    /// GPT-5 Mini - Latest efficient OpenAI model (2025-08-07)
    GPT5Mini,
    /// GPT-5 Nano - Latest most cost-effective OpenAI model (2025-08-07)
    GPT5Nano,
    /// Codex Mini Latest - Latest Codex model for code generation (2025-05-16)
    CodexMiniLatest,

    // Anthropic models
    /// Claude Opus 4.1 - Latest most capable Anthropic model (2025-08-05)
    ClaudeOpus41,
    /// Claude Sonnet 4.5 - Latest balanced Anthropic model (2025-09-29)
    ClaudeSonnet45,
    /// Claude Sonnet 4 - Previous balanced Anthropic model (2025-05-14)
    ClaudeSonnet4,

    // DeepSeek models
    /// DeepSeek V3.2-Exp Chat - Non-thinking mode
    DeepSeekChat,
    /// DeepSeek V3.2-Exp Reasoner - Thinking mode with deliberate reasoning output
    DeepSeekReasoner,

    // xAI models
    /// Grok-2 Latest - Flagship xAI model with advanced reasoning
    XaiGrok2Latest,
    /// Grok-2 - Stable xAI model variant
    XaiGrok2,
    /// Grok-2 Mini - Efficient xAI model
    XaiGrok2Mini,
    /// Grok-2 Reasoning - Enhanced reasoning trace variant
    XaiGrok2Reasoning,
    /// Grok-2 Vision - Multimodal xAI model
    XaiGrok2Vision,

    // OpenRouter models
    /// Grok Code Fast 1 - Fast OpenRouter coding model
    OpenRouterGrokCodeFast1,
    /// Grok 4 Fast - Reasoning-focused Grok endpoint
    OpenRouterGrok4Fast,
    /// Grok 4 - Highest quality Grok endpoint on OpenRouter
    OpenRouterGrok4,
    /// Qwen3 Coder - Balanced OpenRouter coding model
    OpenRouterQwen3Coder,
    /// Qwen3 Coder Plus - High quality Qwen3 coding model
    OpenRouterQwen3CoderPlus,
    /// Qwen3 Coder Flash - Low-latency Qwen3 coding model
    OpenRouterQwen3CoderFlash,
    /// DeepSeek Chat v3.1 - Advanced DeepSeek model via OpenRouter
    OpenRouterDeepSeekChatV31,
    /// DeepSeek R1 - Reasoning model via OpenRouter
    OpenRouterDeepSeekR1,
    /// OpenAI GPT-5 via OpenRouter
    OpenRouterOpenAIGPT5,
    /// OpenAI GPT-5 Codex via OpenRouter
    OpenRouterOpenAIGPT5Codex,
    /// OpenAI o4 Mini via OpenRouter
    OpenRouterOpenAIO4Mini,
    /// OpenAI o3 Mini via OpenRouter
    OpenRouterOpenAIO3Mini,
    /// Anthropic Claude Sonnet 4.5 via OpenRouter
    OpenRouterAnthropicClaudeSonnet45,
    /// Anthropic Claude Opus 4.1 via OpenRouter
    OpenRouterAnthropicClaudeOpus41,
}
impl ModelId {
    /// Convert the model identifier to its string representation
    /// used in API calls and configurations
    pub fn as_str(&self) -> &'static str {
        use crate::config::constants::models;
        match self {
            // Gemini models
            ModelId::Gemini25FlashPreview => models::GEMINI_2_5_FLASH_PREVIEW,
            ModelId::Gemini25Flash => models::GEMINI_2_5_FLASH,
            ModelId::Gemini25FlashLite => models::GEMINI_2_5_FLASH_LITE,
            ModelId::Gemini25Pro => models::GEMINI_2_5_PRO,
            // OpenAI models
            ModelId::GPT5 => models::GPT_5,
            ModelId::GPT5Codex => models::GPT_5_CODEX,
            ModelId::GPT5Mini => models::GPT_5_MINI,
            ModelId::GPT5Nano => models::GPT_5_NANO,
            ModelId::CodexMiniLatest => models::CODEX_MINI_LATEST,
            // Anthropic models
            ModelId::ClaudeOpus41 => models::CLAUDE_OPUS_4_1_20250805,
            ModelId::ClaudeSonnet45 => models::CLAUDE_SONNET_4_5,
            ModelId::ClaudeSonnet4 => models::CLAUDE_SONNET_4_20250514,
            // DeepSeek models
            ModelId::DeepSeekChat => models::DEEPSEEK_CHAT,
            ModelId::DeepSeekReasoner => models::DEEPSEEK_REASONER,
            // xAI models
            ModelId::XaiGrok2Latest => models::xai::GROK_2_LATEST,
            ModelId::XaiGrok2 => models::xai::GROK_2,
            ModelId::XaiGrok2Mini => models::xai::GROK_2_MINI,
            ModelId::XaiGrok2Reasoning => models::xai::GROK_2_REASONING,
            ModelId::XaiGrok2Vision => models::xai::GROK_2_VISION,
            // OpenRouter models
            ModelId::OpenRouterGrokCodeFast1 => models::OPENROUTER_X_AI_GROK_CODE_FAST_1,
            ModelId::OpenRouterGrok4Fast => models::OPENROUTER_X_AI_GROK_4_FAST,
            ModelId::OpenRouterGrok4 => models::OPENROUTER_X_AI_GROK_4,
            ModelId::OpenRouterQwen3Coder => models::OPENROUTER_QWEN3_CODER,
            ModelId::OpenRouterQwen3CoderPlus => models::OPENROUTER_QWEN3_CODER_PLUS,
            ModelId::OpenRouterQwen3CoderFlash => models::OPENROUTER_QWEN3_CODER_FLASH,
            ModelId::OpenRouterDeepSeekChatV31 => models::OPENROUTER_DEEPSEEK_CHAT_V3_1,
            ModelId::OpenRouterDeepSeekR1 => models::OPENROUTER_DEEPSEEK_R1,
            ModelId::OpenRouterOpenAIGPT5 => models::OPENROUTER_OPENAI_GPT_5,
            ModelId::OpenRouterOpenAIGPT5Codex => models::OPENROUTER_OPENAI_GPT_5_CODEX,
            ModelId::OpenRouterOpenAIO4Mini => models::OPENROUTER_OPENAI_O4_MINI,
            ModelId::OpenRouterOpenAIO3Mini => models::OPENROUTER_OPENAI_O3_MINI,
            ModelId::OpenRouterAnthropicClaudeSonnet45 => {
                models::OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5
            }
            ModelId::OpenRouterAnthropicClaudeOpus41 => {
                models::OPENROUTER_ANTHROPIC_CLAUDE_OPUS_4_1
            }
        }
    }

    /// Get the provider for this model
    pub fn provider(&self) -> Provider {
        match self {
            ModelId::Gemini25FlashPreview
            | ModelId::Gemini25Flash
            | ModelId::Gemini25FlashLite
            | ModelId::Gemini25Pro => Provider::Gemini,
            ModelId::GPT5
            | ModelId::GPT5Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::CodexMiniLatest => Provider::OpenAI,
            ModelId::ClaudeOpus41 | ModelId::ClaudeSonnet45 | ModelId::ClaudeSonnet4 => {
                Provider::Anthropic
            }
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => Provider::DeepSeek,
            ModelId::XaiGrok2Latest
            | ModelId::XaiGrok2
            | ModelId::XaiGrok2Mini
            | ModelId::XaiGrok2Reasoning
            | ModelId::XaiGrok2Vision => Provider::XAI,
            ModelId::OpenRouterGrokCodeFast1
            | ModelId::OpenRouterGrok4Fast
            | ModelId::OpenRouterGrok4
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash
            | ModelId::OpenRouterDeepSeekChatV31
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGPT5
            | ModelId::OpenRouterOpenAIGPT5Codex
            | ModelId::OpenRouterOpenAIO4Mini
            | ModelId::OpenRouterOpenAIO3Mini
            | ModelId::OpenRouterAnthropicClaudeSonnet45
            | ModelId::OpenRouterAnthropicClaudeOpus41 => Provider::OpenRouter,
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }

    /// Get the display name for the model (human-readable)
    pub fn display_name(&self) -> &'static str {
        match self {
            // Gemini models
            ModelId::Gemini25FlashPreview => "Gemini 2.5 Flash Preview",
            ModelId::Gemini25Flash => "Gemini 2.5 Flash",
            ModelId::Gemini25FlashLite => "Gemini 2.5 Flash Lite",
            ModelId::Gemini25Pro => "Gemini 2.5 Pro",
            // OpenAI models
            ModelId::GPT5 => "GPT-5",
            ModelId::GPT5Codex => "GPT-5 Codex",
            ModelId::GPT5Mini => "GPT-5 Mini",
            ModelId::GPT5Nano => "GPT-5 Nano",
            ModelId::CodexMiniLatest => "Codex Mini Latest",
            // Anthropic models
            ModelId::ClaudeOpus41 => "Claude Opus 4.1",
            ModelId::ClaudeSonnet45 => "Claude Sonnet 4.5",
            ModelId::ClaudeSonnet4 => "Claude Sonnet 4",
            // DeepSeek models
            ModelId::DeepSeekChat => "DeepSeek V3.2-Exp (Chat)",
            ModelId::DeepSeekReasoner => "DeepSeek V3.2-Exp (Reasoner)",
            // xAI models
            ModelId::XaiGrok2Latest => "Grok-2 Latest",
            ModelId::XaiGrok2 => "Grok-2",
            ModelId::XaiGrok2Mini => "Grok-2 Mini",
            ModelId::XaiGrok2Reasoning => "Grok-2 Reasoning",
            ModelId::XaiGrok2Vision => "Grok-2 Vision",
            // OpenRouter models
            ModelId::OpenRouterGrokCodeFast1 => "Grok Code Fast 1",
            ModelId::OpenRouterGrok4Fast => "Grok 4 Fast",
            ModelId::OpenRouterGrok4 => "Grok 4",
            ModelId::OpenRouterQwen3Coder => "Qwen3 Coder",
            ModelId::OpenRouterQwen3CoderPlus => "Qwen3 Coder Plus",
            ModelId::OpenRouterQwen3CoderFlash => "Qwen3 Coder Flash",
            ModelId::OpenRouterDeepSeekChatV31 => "DeepSeek Chat v3.1",
            ModelId::OpenRouterDeepSeekR1 => "DeepSeek R1",
            ModelId::OpenRouterOpenAIGPT5 => "OpenAI GPT-5 via OpenRouter",
            ModelId::OpenRouterOpenAIGPT5Codex => "OpenAI GPT-5 Codex via OpenRouter",
            ModelId::OpenRouterOpenAIO4Mini => "OpenAI o4 Mini via OpenRouter",
            ModelId::OpenRouterOpenAIO3Mini => "OpenAI o3 Mini via OpenRouter",
            ModelId::OpenRouterAnthropicClaudeSonnet45 => {
                "Anthropic Claude Sonnet 4.5 via OpenRouter"
            }
            ModelId::OpenRouterAnthropicClaudeOpus41 => "Anthropic Claude Opus 4.1 via OpenRouter",
        }
    }

    /// Get a description of the model's characteristics
    pub fn description(&self) -> &'static str {
        match self {
            // Gemini models
            ModelId::Gemini25FlashPreview => {
                "Latest fast Gemini model with advanced multimodal capabilities"
            }
            ModelId::Gemini25Flash => {
                "Legacy alias for Gemini 2.5 Flash Preview (same capabilities)"
            }
            ModelId::Gemini25FlashLite => {
                "Legacy alias for Gemini 2.5 Flash Preview optimized for efficiency"
            }
            ModelId::Gemini25Pro => "Latest most capable Gemini model with reasoning",
            // OpenAI models
            ModelId::GPT5 => "Latest most capable OpenAI model with advanced reasoning",
            ModelId::GPT5Codex => {
                "Code-focused GPT-5 variant optimized for tool calling and structured outputs"
            }
            ModelId::GPT5Mini => "Latest efficient OpenAI model, great for most tasks",
            ModelId::GPT5Nano => "Latest most cost-effective OpenAI model",
            ModelId::CodexMiniLatest => "Latest Codex model optimized for code generation",
            // Anthropic models
            ModelId::ClaudeOpus41 => "Latest most capable Anthropic model with advanced reasoning",
            ModelId::ClaudeSonnet45 => "Latest balanced Anthropic model for general tasks",
            ModelId::ClaudeSonnet4 => {
                "Previous balanced Anthropic model maintained for compatibility"
            }
            // DeepSeek models
            ModelId::DeepSeekChat => {
                "DeepSeek V3.2-Exp non-thinking mode optimized for fast coding responses"
            }
            ModelId::DeepSeekReasoner => {
                "DeepSeek V3.2-Exp thinking mode with structured reasoning output"
            }
            // xAI models
            ModelId::XaiGrok2Latest => "Flagship xAI Grok model with long context and tool use",
            ModelId::XaiGrok2 => "Stable Grok 2 release tuned for general coding tasks",
            ModelId::XaiGrok2Mini => "Efficient Grok 2 variant optimized for latency",
            ModelId::XaiGrok2Reasoning => {
                "Grok 2 variant that surfaces structured reasoning traces"
            }
            ModelId::XaiGrok2Vision => "Multimodal Grok 2 model with image understanding",
            // OpenRouter models
            ModelId::OpenRouterGrokCodeFast1 => "Fast OpenRouter coding model powered by xAI Grok",
            ModelId::OpenRouterGrok4Fast => {
                "Reasoning-focused Grok 4 endpoint with transparent traces via OpenRouter"
            }
            ModelId::OpenRouterGrok4 => "Flagship Grok 4 endpoint exposed through OpenRouter",
            ModelId::OpenRouterQwen3Coder => {
                "Qwen3-based OpenRouter model tuned for IDE-style coding workflows"
            }
            ModelId::OpenRouterQwen3CoderPlus => {
                "Premium Qwen3 coding model with higher quality outputs and long context"
            }
            ModelId::OpenRouterQwen3CoderFlash => {
                "Latency-optimized Qwen3 coding model ideal for quick iterations"
            }
            ModelId::OpenRouterDeepSeekChatV31 => "Advanced DeepSeek model via OpenRouter",
            ModelId::OpenRouterDeepSeekR1 => {
                "DeepSeek R1 reasoning model with chain-of-thought access via OpenRouter"
            }
            ModelId::OpenRouterOpenAIGPT5 => "OpenAI GPT-5 model accessed through OpenRouter",
            ModelId::OpenRouterOpenAIGPT5Codex => {
                "OpenAI GPT-5 Codex coding model accessed through OpenRouter"
            }
            ModelId::OpenRouterOpenAIO4Mini => {
                "OpenAI o4 Mini reasoning model delivered through OpenRouter"
            }
            ModelId::OpenRouterOpenAIO3Mini => {
                "OpenAI o3 Mini efficiency model delivered through OpenRouter"
            }
            ModelId::OpenRouterAnthropicClaudeSonnet45 => {
                "Anthropic Claude Sonnet 4.5 model accessed through OpenRouter"
            }
            ModelId::OpenRouterAnthropicClaudeOpus41 => {
                "Anthropic Claude Opus 4.1 model accessed through OpenRouter"
            }
        }
    }

    /// Get all available models as a vector
    pub fn all_models() -> Vec<ModelId> {
        vec![
            // Gemini models
            ModelId::Gemini25FlashPreview,
            ModelId::Gemini25Flash,
            ModelId::Gemini25FlashLite,
            ModelId::Gemini25Pro,
            // OpenAI models
            ModelId::GPT5,
            ModelId::GPT5Codex,
            ModelId::GPT5Mini,
            ModelId::GPT5Nano,
            ModelId::CodexMiniLatest,
            // Anthropic models
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::ClaudeSonnet4,
            // DeepSeek models
            ModelId::DeepSeekChat,
            ModelId::DeepSeekReasoner,
            // xAI models
            ModelId::XaiGrok2Latest,
            ModelId::XaiGrok2,
            ModelId::XaiGrok2Mini,
            ModelId::XaiGrok2Reasoning,
            ModelId::XaiGrok2Vision,
            // OpenRouter models
            ModelId::OpenRouterGrokCodeFast1,
            ModelId::OpenRouterGrok4Fast,
            ModelId::OpenRouterGrok4,
            ModelId::OpenRouterQwen3Coder,
            ModelId::OpenRouterQwen3CoderPlus,
            ModelId::OpenRouterQwen3CoderFlash,
            ModelId::OpenRouterDeepSeekChatV31,
            ModelId::OpenRouterDeepSeekR1,
            ModelId::OpenRouterOpenAIGPT5,
            ModelId::OpenRouterOpenAIGPT5Codex,
            ModelId::OpenRouterOpenAIO4Mini,
            ModelId::OpenRouterOpenAIO3Mini,
            ModelId::OpenRouterAnthropicClaudeSonnet45,
            ModelId::OpenRouterAnthropicClaudeOpus41,
        ]
    }

    /// Get all models for a specific provider
    pub fn models_for_provider(provider: Provider) -> Vec<ModelId> {
        Self::all_models()
            .into_iter()
            .filter(|model| model.provider() == provider)
            .collect()
    }

    /// Get recommended fallback models in order of preference
    pub fn fallback_models() -> Vec<ModelId> {
        vec![
            ModelId::Gemini25FlashPreview,
            ModelId::Gemini25Pro,
            ModelId::GPT5,
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::DeepSeekReasoner,
            ModelId::XaiGrok2Latest,
            ModelId::OpenRouterGrokCodeFast1,
        ]
    }

    /// Get the default model for general use
    pub fn default() -> Self {
        ModelId::Gemini25FlashPreview
    }

    /// Get the default orchestrator model (more capable)
    pub fn default_orchestrator() -> Self {
        ModelId::Gemini25Pro
    }

    /// Get the default subagent model (fast and efficient)
    pub fn default_subagent() -> Self {
        ModelId::Gemini25FlashPreview
    }

    /// Get provider-specific defaults for orchestrator
    pub fn default_orchestrator_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25Pro,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus41,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::XAI => ModelId::XaiGrok2Latest,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
        }
    }

    /// Get provider-specific defaults for subagent
    pub fn default_subagent_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5Mini,
            Provider::Anthropic => ModelId::ClaudeSonnet45,
            Provider::DeepSeek => ModelId::DeepSeekChat,
            Provider::XAI => ModelId::XaiGrok2Mini,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
        }
    }

    /// Get provider-specific defaults for single agent
    pub fn default_single_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus41,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::XAI => ModelId::XaiGrok2Latest,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
        }
    }

    /// Check if this is a "flash" variant (optimized for speed)
    pub fn is_flash_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25FlashPreview | ModelId::Gemini25Flash | ModelId::Gemini25FlashLite
        )
    }

    /// Check if this is a "pro" variant (optimized for capability)
    pub fn is_pro_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25Pro
                | ModelId::GPT5
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus41
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok2Latest
        )
    }

    /// Check if this is an optimized/efficient variant
    pub fn is_efficient_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25FlashPreview
                | ModelId::Gemini25Flash
                | ModelId::Gemini25FlashLite
                | ModelId::GPT5Mini
                | ModelId::GPT5Nano
                | ModelId::OpenRouterGrokCodeFast1
                | ModelId::OpenRouterQwen3CoderFlash
                | ModelId::OpenRouterOpenAIO3Mini
                | ModelId::OpenRouterOpenAIO4Mini
                | ModelId::DeepSeekChat
                | ModelId::XaiGrok2Mini
        )
    }

    /// Check if this is a top-tier model
    pub fn is_top_tier(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25Pro
                | ModelId::GPT5
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus41
                | ModelId::ClaudeSonnet45
                | ModelId::ClaudeSonnet4
                | ModelId::DeepSeekReasoner
                | ModelId::OpenRouterGrok4Fast
                | ModelId::OpenRouterGrok4
                | ModelId::OpenRouterQwen3Coder
                | ModelId::OpenRouterQwen3CoderPlus
                | ModelId::OpenRouterDeepSeekR1
                | ModelId::OpenRouterOpenAIGPT5
                | ModelId::OpenRouterOpenAIGPT5Codex
                | ModelId::OpenRouterAnthropicClaudeSonnet45
                | ModelId::OpenRouterAnthropicClaudeOpus41
                | ModelId::XaiGrok2Latest
                | ModelId::XaiGrok2Reasoning
        )
    }

    /// Get the generation/version string for this model
    pub fn generation(&self) -> &'static str {
        match self {
            // Gemini generations
            ModelId::Gemini25FlashPreview
            | ModelId::Gemini25Flash
            | ModelId::Gemini25FlashLite
            | ModelId::Gemini25Pro => "2.5",
            // OpenAI generations
            ModelId::GPT5
            | ModelId::GPT5Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::CodexMiniLatest => "5",
            // Anthropic generations
            ModelId::ClaudeSonnet45 => "4.5",
            ModelId::ClaudeSonnet4 => "4",
            ModelId::ClaudeOpus41 => "4.1",
            // DeepSeek generations
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => "V3.2-Exp",
            // xAI generations
            ModelId::XaiGrok2Latest
            | ModelId::XaiGrok2
            | ModelId::XaiGrok2Mini
            | ModelId::XaiGrok2Reasoning
            | ModelId::XaiGrok2Vision => "2",
            // OpenRouter marketplace listings
            ModelId::OpenRouterGrokCodeFast1
            | ModelId::OpenRouterGrok4Fast
            | ModelId::OpenRouterGrok4
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash => "marketplace",
            ModelId::OpenRouterDeepSeekChatV31 => "2025-08-21",
            ModelId::OpenRouterDeepSeekR1 => "2025-01-20",
            ModelId::OpenRouterOpenAIGPT5 | ModelId::OpenRouterOpenAIGPT5Codex => "2025-09-20",
            ModelId::OpenRouterOpenAIO4Mini | ModelId::OpenRouterOpenAIO3Mini => "2025-07-18",
            ModelId::OpenRouterAnthropicClaudeOpus41 => "2025-08-05",
            ModelId::OpenRouterAnthropicClaudeSonnet45 => "2025-09-29",
        }
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for ModelId {
    type Err = ModelParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use crate::config::constants::models;
        match s {
            // Gemini models
            s if s == models::GEMINI_2_5_FLASH_PREVIEW => Ok(ModelId::Gemini25FlashPreview),
            s if s == models::GEMINI_2_5_FLASH => Ok(ModelId::Gemini25Flash),
            s if s == models::GEMINI_2_5_FLASH_LITE => Ok(ModelId::Gemini25FlashLite),
            s if s == models::GEMINI_2_5_PRO => Ok(ModelId::Gemini25Pro),
            // OpenAI models
            s if s == models::GPT_5 => Ok(ModelId::GPT5),
            s if s == models::GPT_5_CODEX => Ok(ModelId::GPT5Codex),
            s if s == models::GPT_5_MINI => Ok(ModelId::GPT5Mini),
            s if s == models::GPT_5_NANO => Ok(ModelId::GPT5Nano),
            s if s == models::CODEX_MINI_LATEST => Ok(ModelId::CodexMiniLatest),
            // Anthropic models
            s if s == models::CLAUDE_OPUS_4_1_20250805 => Ok(ModelId::ClaudeOpus41),
            s if s == models::CLAUDE_SONNET_4_5 => Ok(ModelId::ClaudeSonnet45),
            s if s == models::CLAUDE_SONNET_4_20250514 => Ok(ModelId::ClaudeSonnet4),
            // DeepSeek models
            s if s == models::DEEPSEEK_CHAT => Ok(ModelId::DeepSeekChat),
            s if s == models::DEEPSEEK_REASONER => Ok(ModelId::DeepSeekReasoner),
            // xAI models
            s if s == models::xai::GROK_2_LATEST => Ok(ModelId::XaiGrok2Latest),
            s if s == models::xai::GROK_2 => Ok(ModelId::XaiGrok2),
            s if s == models::xai::GROK_2_MINI => Ok(ModelId::XaiGrok2Mini),
            s if s == models::xai::GROK_2_REASONING => Ok(ModelId::XaiGrok2Reasoning),
            s if s == models::xai::GROK_2_VISION => Ok(ModelId::XaiGrok2Vision),
            // OpenRouter models
            s if s == models::OPENROUTER_X_AI_GROK_CODE_FAST_1 => {
                Ok(ModelId::OpenRouterGrokCodeFast1)
            }
            s if s == models::OPENROUTER_X_AI_GROK_4_FAST => Ok(ModelId::OpenRouterGrok4Fast),
            s if s == models::OPENROUTER_X_AI_GROK_4 => Ok(ModelId::OpenRouterGrok4),
            s if s == models::OPENROUTER_QWEN3_CODER => Ok(ModelId::OpenRouterQwen3Coder),
            s if s == models::OPENROUTER_QWEN3_CODER_PLUS => Ok(ModelId::OpenRouterQwen3CoderPlus),
            s if s == models::OPENROUTER_QWEN3_CODER_FLASH => {
                Ok(ModelId::OpenRouterQwen3CoderFlash)
            }
            s if s == models::OPENROUTER_DEEPSEEK_CHAT_V3_1 => {
                Ok(ModelId::OpenRouterDeepSeekChatV31)
            }
            s if s == models::OPENROUTER_DEEPSEEK_R1 => Ok(ModelId::OpenRouterDeepSeekR1),
            s if s == models::OPENROUTER_OPENAI_GPT_5 => Ok(ModelId::OpenRouterOpenAIGPT5),
            s if s == models::OPENROUTER_OPENAI_GPT_5_CODEX => {
                Ok(ModelId::OpenRouterOpenAIGPT5Codex)
            }
            s if s == models::OPENROUTER_OPENAI_O4_MINI => Ok(ModelId::OpenRouterOpenAIO4Mini),
            s if s == models::OPENROUTER_OPENAI_O3_MINI => Ok(ModelId::OpenRouterOpenAIO3Mini),
            s if s == models::OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5 => {
                Ok(ModelId::OpenRouterAnthropicClaudeSonnet45)
            }
            s if s == models::OPENROUTER_ANTHROPIC_CLAUDE_OPUS_4_1 => {
                Ok(ModelId::OpenRouterAnthropicClaudeOpus41)
            }
            _ => Err(ModelParseError::InvalidModel(s.to_string())),
        }
    }
}

/// Error type for model parsing failures
#[derive(Debug, Clone, PartialEq)]
pub enum ModelParseError {
    InvalidModel(String),
    InvalidProvider(String),
}

impl fmt::Display for ModelParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelParseError::InvalidModel(model) => {
                write!(
                    f,
                    "Invalid model identifier: '{}'. Supported models: {}",
                    model,
                    ModelId::all_models()
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ModelParseError::InvalidProvider(provider) => {
                write!(
                    f,
                    "Invalid provider: '{}'. Supported providers: {}",
                    provider,
                    Provider::all_providers()
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ModelParseError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::models;

    #[test]
    fn test_model_string_conversion() {
        // Gemini models
        assert_eq!(
            ModelId::Gemini25FlashPreview.as_str(),
            models::GEMINI_2_5_FLASH_PREVIEW
        );
        assert_eq!(ModelId::Gemini25Flash.as_str(), models::GEMINI_2_5_FLASH);
        assert_eq!(
            ModelId::Gemini25FlashLite.as_str(),
            models::GEMINI_2_5_FLASH_LITE
        );
        assert_eq!(ModelId::Gemini25Pro.as_str(), models::GEMINI_2_5_PRO);
        // OpenAI models
        assert_eq!(ModelId::GPT5.as_str(), models::GPT_5);
        assert_eq!(ModelId::GPT5Codex.as_str(), models::GPT_5_CODEX);
        assert_eq!(ModelId::GPT5Mini.as_str(), models::GPT_5_MINI);
        assert_eq!(ModelId::GPT5Nano.as_str(), models::GPT_5_NANO);
        assert_eq!(ModelId::CodexMiniLatest.as_str(), models::CODEX_MINI_LATEST);
        // Anthropic models
        assert_eq!(ModelId::ClaudeSonnet45.as_str(), models::CLAUDE_SONNET_4_5);
        assert_eq!(
            ModelId::ClaudeSonnet4.as_str(),
            models::CLAUDE_SONNET_4_20250514
        );
        assert_eq!(
            ModelId::ClaudeOpus41.as_str(),
            models::CLAUDE_OPUS_4_1_20250805
        );
        // DeepSeek models
        assert_eq!(ModelId::DeepSeekChat.as_str(), models::DEEPSEEK_CHAT);
        assert_eq!(
            ModelId::DeepSeekReasoner.as_str(),
            models::DEEPSEEK_REASONER
        );
        // xAI models
        assert_eq!(ModelId::XaiGrok2Latest.as_str(), models::xai::GROK_2_LATEST);
        assert_eq!(ModelId::XaiGrok2.as_str(), models::xai::GROK_2);
        assert_eq!(ModelId::XaiGrok2Mini.as_str(), models::xai::GROK_2_MINI);
        assert_eq!(
            ModelId::XaiGrok2Reasoning.as_str(),
            models::xai::GROK_2_REASONING
        );
        assert_eq!(ModelId::XaiGrok2Vision.as_str(), models::xai::GROK_2_VISION);
        // OpenRouter models
        assert_eq!(
            ModelId::OpenRouterGrokCodeFast1.as_str(),
            models::OPENROUTER_X_AI_GROK_CODE_FAST_1
        );
        assert_eq!(
            ModelId::OpenRouterGrok4Fast.as_str(),
            models::OPENROUTER_X_AI_GROK_4_FAST
        );
        assert_eq!(
            ModelId::OpenRouterGrok4.as_str(),
            models::OPENROUTER_X_AI_GROK_4
        );
        assert_eq!(
            ModelId::OpenRouterQwen3Coder.as_str(),
            models::OPENROUTER_QWEN3_CODER
        );
        assert_eq!(
            ModelId::OpenRouterQwen3CoderPlus.as_str(),
            models::OPENROUTER_QWEN3_CODER_PLUS
        );
        assert_eq!(
            ModelId::OpenRouterQwen3CoderFlash.as_str(),
            models::OPENROUTER_QWEN3_CODER_FLASH
        );
        assert_eq!(
            ModelId::OpenRouterDeepSeekChatV31.as_str(),
            models::OPENROUTER_DEEPSEEK_CHAT_V3_1
        );
        assert_eq!(
            ModelId::OpenRouterDeepSeekR1.as_str(),
            models::OPENROUTER_DEEPSEEK_R1
        );
        assert_eq!(
            ModelId::OpenRouterOpenAIGPT5.as_str(),
            models::OPENROUTER_OPENAI_GPT_5
        );
        assert_eq!(
            ModelId::OpenRouterOpenAIGPT5Codex.as_str(),
            models::OPENROUTER_OPENAI_GPT_5_CODEX
        );
        assert_eq!(
            ModelId::OpenRouterOpenAIO4Mini.as_str(),
            models::OPENROUTER_OPENAI_O4_MINI
        );
        assert_eq!(
            ModelId::OpenRouterOpenAIO3Mini.as_str(),
            models::OPENROUTER_OPENAI_O3_MINI
        );
        assert_eq!(
            ModelId::OpenRouterAnthropicClaudeSonnet45.as_str(),
            models::OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5
        );
        assert_eq!(
            ModelId::OpenRouterAnthropicClaudeOpus41.as_str(),
            models::OPENROUTER_ANTHROPIC_CLAUDE_OPUS_4_1
        );
    }

    #[test]
    fn test_model_from_string() {
        // Gemini models
        assert_eq!(
            models::GEMINI_2_5_FLASH_PREVIEW.parse::<ModelId>().unwrap(),
            ModelId::Gemini25FlashPreview
        );
        assert_eq!(
            models::GEMINI_2_5_FLASH.parse::<ModelId>().unwrap(),
            ModelId::Gemini25Flash
        );
        assert_eq!(
            models::GEMINI_2_5_FLASH_LITE.parse::<ModelId>().unwrap(),
            ModelId::Gemini25FlashLite
        );
        assert_eq!(
            models::GEMINI_2_5_PRO.parse::<ModelId>().unwrap(),
            ModelId::Gemini25Pro
        );
        // OpenAI models
        assert_eq!(models::GPT_5.parse::<ModelId>().unwrap(), ModelId::GPT5);
        assert_eq!(
            models::GPT_5_CODEX.parse::<ModelId>().unwrap(),
            ModelId::GPT5Codex
        );
        assert_eq!(
            models::GPT_5_MINI.parse::<ModelId>().unwrap(),
            ModelId::GPT5Mini
        );
        assert_eq!(
            models::GPT_5_NANO.parse::<ModelId>().unwrap(),
            ModelId::GPT5Nano
        );
        assert_eq!(
            models::CODEX_MINI_LATEST.parse::<ModelId>().unwrap(),
            ModelId::CodexMiniLatest
        );
        // Anthropic models
        assert_eq!(
            models::CLAUDE_SONNET_4_5.parse::<ModelId>().unwrap(),
            ModelId::ClaudeSonnet45
        );
        assert_eq!(
            models::CLAUDE_SONNET_4_20250514.parse::<ModelId>().unwrap(),
            ModelId::ClaudeSonnet4
        );
        assert_eq!(
            models::CLAUDE_OPUS_4_1_20250805.parse::<ModelId>().unwrap(),
            ModelId::ClaudeOpus41
        );
        // DeepSeek models
        assert_eq!(
            models::DEEPSEEK_CHAT.parse::<ModelId>().unwrap(),
            ModelId::DeepSeekChat
        );
        assert_eq!(
            models::DEEPSEEK_REASONER.parse::<ModelId>().unwrap(),
            ModelId::DeepSeekReasoner
        );
        // xAI models
        assert_eq!(
            models::xai::GROK_2_LATEST.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok2Latest
        );
        assert_eq!(
            models::xai::GROK_2.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok2
        );
        assert_eq!(
            models::xai::GROK_2_MINI.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok2Mini
        );
        assert_eq!(
            models::xai::GROK_2_REASONING.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok2Reasoning
        );
        assert_eq!(
            models::xai::GROK_2_VISION.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok2Vision
        );
        // OpenRouter models
        assert_eq!(
            models::OPENROUTER_X_AI_GROK_CODE_FAST_1
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterGrokCodeFast1
        );
        assert_eq!(
            models::OPENROUTER_X_AI_GROK_4_FAST
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterGrok4Fast
        );
        assert_eq!(
            models::OPENROUTER_X_AI_GROK_4.parse::<ModelId>().unwrap(),
            ModelId::OpenRouterGrok4
        );
        assert_eq!(
            models::OPENROUTER_QWEN3_CODER.parse::<ModelId>().unwrap(),
            ModelId::OpenRouterQwen3Coder
        );
        assert_eq!(
            models::OPENROUTER_QWEN3_CODER_PLUS
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterQwen3CoderPlus
        );
        assert_eq!(
            models::OPENROUTER_QWEN3_CODER_FLASH
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterQwen3CoderFlash
        );
        assert_eq!(
            models::OPENROUTER_DEEPSEEK_CHAT_V3_1
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterDeepSeekChatV31
        );
        assert_eq!(
            models::OPENROUTER_DEEPSEEK_R1.parse::<ModelId>().unwrap(),
            ModelId::OpenRouterDeepSeekR1
        );
        assert_eq!(
            models::OPENROUTER_OPENAI_GPT_5.parse::<ModelId>().unwrap(),
            ModelId::OpenRouterOpenAIGPT5
        );
        assert_eq!(
            models::OPENROUTER_OPENAI_GPT_5_CODEX
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterOpenAIGPT5Codex
        );
        assert_eq!(
            models::OPENROUTER_OPENAI_O4_MINI
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterOpenAIO4Mini
        );
        assert_eq!(
            models::OPENROUTER_OPENAI_O3_MINI
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterOpenAIO3Mini
        );
        assert_eq!(
            models::OPENROUTER_ANTHROPIC_CLAUDE_SONNET_4_5
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterAnthropicClaudeSonnet45
        );
        assert_eq!(
            models::OPENROUTER_ANTHROPIC_CLAUDE_OPUS_4_1
                .parse::<ModelId>()
                .unwrap(),
            ModelId::OpenRouterAnthropicClaudeOpus41
        );
        // Invalid model
        assert!("invalid-model".parse::<ModelId>().is_err());
    }

    #[test]
    fn test_provider_parsing() {
        assert_eq!("gemini".parse::<Provider>().unwrap(), Provider::Gemini);
        assert_eq!("openai".parse::<Provider>().unwrap(), Provider::OpenAI);
        assert_eq!(
            "anthropic".parse::<Provider>().unwrap(),
            Provider::Anthropic
        );
        assert_eq!("deepseek".parse::<Provider>().unwrap(), Provider::DeepSeek);
        assert_eq!(
            "openrouter".parse::<Provider>().unwrap(),
            Provider::OpenRouter
        );
        assert_eq!("xai".parse::<Provider>().unwrap(), Provider::XAI);
        assert!("invalid-provider".parse::<Provider>().is_err());
    }

    #[test]
    fn test_model_providers() {
        assert_eq!(ModelId::Gemini25FlashPreview.provider(), Provider::Gemini);
        assert_eq!(ModelId::GPT5.provider(), Provider::OpenAI);
        assert_eq!(ModelId::GPT5Codex.provider(), Provider::OpenAI);
        assert_eq!(ModelId::ClaudeSonnet45.provider(), Provider::Anthropic);
        assert_eq!(ModelId::ClaudeSonnet4.provider(), Provider::Anthropic);
        assert_eq!(ModelId::DeepSeekChat.provider(), Provider::DeepSeek);
        assert_eq!(ModelId::XaiGrok2Latest.provider(), Provider::XAI);
        assert_eq!(
            ModelId::OpenRouterGrokCodeFast1.provider(),
            Provider::OpenRouter
        );
        assert_eq!(
            ModelId::OpenRouterAnthropicClaudeSonnet45.provider(),
            Provider::OpenRouter
        );
    }

    #[test]
    fn test_provider_defaults() {
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::Gemini),
            ModelId::Gemini25Pro
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::OpenAI),
            ModelId::GPT5
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::Anthropic),
            ModelId::ClaudeSonnet4
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::DeepSeek),
            ModelId::DeepSeekReasoner
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::OpenRouter),
            ModelId::OpenRouterGrokCodeFast1
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::XAI),
            ModelId::XaiGrok2Latest
        );

        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::Gemini),
            ModelId::Gemini25FlashPreview
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::OpenAI),
            ModelId::GPT5Mini
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::Anthropic),
            ModelId::ClaudeSonnet45
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::DeepSeek),
            ModelId::DeepSeekChat
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::OpenRouter),
            ModelId::OpenRouterGrokCodeFast1
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::XAI),
            ModelId::XaiGrok2Mini
        );

        assert_eq!(
            ModelId::default_single_for_provider(Provider::DeepSeek),
            ModelId::DeepSeekReasoner
        );
    }

    #[test]
    fn test_model_defaults() {
        assert_eq!(ModelId::default(), ModelId::Gemini25FlashPreview);
        assert_eq!(ModelId::default_orchestrator(), ModelId::Gemini25Pro);
        assert_eq!(ModelId::default_subagent(), ModelId::Gemini25FlashPreview);
    }

    #[test]
    fn test_model_variants() {
        // Flash variants
        assert!(ModelId::Gemini25FlashPreview.is_flash_variant());
        assert!(ModelId::Gemini25Flash.is_flash_variant());
        assert!(ModelId::Gemini25FlashLite.is_flash_variant());
        assert!(!ModelId::GPT5.is_flash_variant());

        // Pro variants
        assert!(ModelId::Gemini25Pro.is_pro_variant());
        assert!(ModelId::GPT5.is_pro_variant());
        assert!(ModelId::GPT5Codex.is_pro_variant());
        assert!(ModelId::DeepSeekReasoner.is_pro_variant());
        assert!(!ModelId::Gemini25FlashPreview.is_pro_variant());

        // Efficient variants
        assert!(ModelId::Gemini25FlashPreview.is_efficient_variant());
        assert!(ModelId::Gemini25Flash.is_efficient_variant());
        assert!(ModelId::Gemini25FlashLite.is_efficient_variant());
        assert!(ModelId::GPT5Mini.is_efficient_variant());
        assert!(ModelId::OpenRouterGrokCodeFast1.is_efficient_variant());
        assert!(ModelId::OpenRouterQwen3CoderFlash.is_efficient_variant());
        assert!(ModelId::OpenRouterOpenAIO3Mini.is_efficient_variant());
        assert!(ModelId::OpenRouterOpenAIO4Mini.is_efficient_variant());
        assert!(ModelId::XaiGrok2Mini.is_efficient_variant());
        assert!(ModelId::DeepSeekChat.is_efficient_variant());
        assert!(!ModelId::GPT5.is_efficient_variant());

        // Top tier models
        assert!(ModelId::Gemini25Pro.is_top_tier());
        assert!(ModelId::GPT5.is_top_tier());
        assert!(ModelId::GPT5Codex.is_top_tier());
        assert!(ModelId::ClaudeSonnet45.is_top_tier());
        assert!(ModelId::ClaudeSonnet4.is_top_tier());
        assert!(ModelId::OpenRouterGrok4Fast.is_top_tier());
        assert!(ModelId::OpenRouterGrok4.is_top_tier());
        assert!(ModelId::OpenRouterQwen3Coder.is_top_tier());
        assert!(ModelId::OpenRouterQwen3CoderPlus.is_top_tier());
        assert!(ModelId::OpenRouterDeepSeekR1.is_top_tier());
        assert!(ModelId::OpenRouterOpenAIGPT5.is_top_tier());
        assert!(ModelId::OpenRouterOpenAIGPT5Codex.is_top_tier());
        assert!(ModelId::OpenRouterAnthropicClaudeSonnet45.is_top_tier());
        assert!(ModelId::OpenRouterAnthropicClaudeOpus41.is_top_tier());
        assert!(ModelId::XaiGrok2Latest.is_top_tier());
        assert!(ModelId::XaiGrok2Reasoning.is_top_tier());
        assert!(ModelId::DeepSeekReasoner.is_top_tier());
        assert!(!ModelId::Gemini25FlashPreview.is_top_tier());
    }

    #[test]
    fn test_model_generation() {
        // Gemini generations
        assert_eq!(ModelId::Gemini25FlashPreview.generation(), "2.5");
        assert_eq!(ModelId::Gemini25Flash.generation(), "2.5");
        assert_eq!(ModelId::Gemini25FlashLite.generation(), "2.5");
        assert_eq!(ModelId::Gemini25Pro.generation(), "2.5");

        // OpenAI generations
        assert_eq!(ModelId::GPT5.generation(), "5");
        assert_eq!(ModelId::GPT5Codex.generation(), "5");
        assert_eq!(ModelId::GPT5Mini.generation(), "5");
        assert_eq!(ModelId::GPT5Nano.generation(), "5");
        assert_eq!(ModelId::CodexMiniLatest.generation(), "5");

        // Anthropic generations
        assert_eq!(ModelId::ClaudeSonnet45.generation(), "4.5");
        assert_eq!(ModelId::ClaudeSonnet4.generation(), "4");
        assert_eq!(ModelId::ClaudeOpus41.generation(), "4.1");

        // DeepSeek generations
        assert_eq!(ModelId::DeepSeekChat.generation(), "V3.2-Exp");
        assert_eq!(ModelId::DeepSeekReasoner.generation(), "V3.2-Exp");

        // xAI generations
        assert_eq!(ModelId::XaiGrok2Latest.generation(), "2");
        assert_eq!(ModelId::XaiGrok2.generation(), "2");
        assert_eq!(ModelId::XaiGrok2Mini.generation(), "2");
        assert_eq!(ModelId::XaiGrok2Reasoning.generation(), "2");
        assert_eq!(ModelId::XaiGrok2Vision.generation(), "2");

        // OpenRouter marketplace entries
        assert_eq!(ModelId::OpenRouterGrokCodeFast1.generation(), "marketplace");
        assert_eq!(ModelId::OpenRouterGrok4Fast.generation(), "marketplace");
        assert_eq!(ModelId::OpenRouterGrok4.generation(), "marketplace");
        assert_eq!(ModelId::OpenRouterQwen3Coder.generation(), "marketplace");
        assert_eq!(
            ModelId::OpenRouterQwen3CoderPlus.generation(),
            "marketplace"
        );
        assert_eq!(
            ModelId::OpenRouterQwen3CoderFlash.generation(),
            "marketplace"
        );

        // New OpenRouter models
        assert_eq!(
            ModelId::OpenRouterDeepSeekChatV31.generation(),
            "2025-08-21"
        );
        assert_eq!(ModelId::OpenRouterDeepSeekR1.generation(), "2025-01-20");
        assert_eq!(ModelId::OpenRouterOpenAIGPT5.generation(), "2025-09-20");
        assert_eq!(
            ModelId::OpenRouterOpenAIGPT5Codex.generation(),
            "2025-09-20"
        );
        assert_eq!(ModelId::OpenRouterOpenAIO4Mini.generation(), "2025-07-18");
        assert_eq!(ModelId::OpenRouterOpenAIO3Mini.generation(), "2025-07-18");
        assert_eq!(
            ModelId::OpenRouterAnthropicClaudeOpus41.generation(),
            "2025-08-05"
        );
        assert_eq!(
            ModelId::OpenRouterAnthropicClaudeSonnet45.generation(),
            "2025-09-29"
        );
    }

    #[test]
    fn test_models_for_provider() {
        let gemini_models = ModelId::models_for_provider(Provider::Gemini);
        assert!(gemini_models.contains(&ModelId::Gemini25Pro));
        assert!(!gemini_models.contains(&ModelId::GPT5));

        let openai_models = ModelId::models_for_provider(Provider::OpenAI);
        assert!(openai_models.contains(&ModelId::GPT5));
        assert!(openai_models.contains(&ModelId::GPT5Codex));
        assert!(!openai_models.contains(&ModelId::Gemini25Pro));

        let anthropic_models = ModelId::models_for_provider(Provider::Anthropic);
        assert!(anthropic_models.contains(&ModelId::ClaudeSonnet45));
        assert!(anthropic_models.contains(&ModelId::ClaudeSonnet4));
        assert!(!anthropic_models.contains(&ModelId::GPT5));

        let deepseek_models = ModelId::models_for_provider(Provider::DeepSeek);
        assert!(deepseek_models.contains(&ModelId::DeepSeekChat));
        assert!(deepseek_models.contains(&ModelId::DeepSeekReasoner));

        let openrouter_models = ModelId::models_for_provider(Provider::OpenRouter);
        assert!(openrouter_models.contains(&ModelId::OpenRouterGrokCodeFast1));
        assert!(openrouter_models.contains(&ModelId::OpenRouterGrok4Fast));
        assert!(openrouter_models.contains(&ModelId::OpenRouterGrok4));
        assert!(openrouter_models.contains(&ModelId::OpenRouterQwen3Coder));
        assert!(openrouter_models.contains(&ModelId::OpenRouterQwen3CoderPlus));
        assert!(openrouter_models.contains(&ModelId::OpenRouterQwen3CoderFlash));
        assert!(openrouter_models.contains(&ModelId::OpenRouterDeepSeekChatV31));
        assert!(openrouter_models.contains(&ModelId::OpenRouterDeepSeekR1));
        assert!(openrouter_models.contains(&ModelId::OpenRouterOpenAIGPT5));
        assert!(openrouter_models.contains(&ModelId::OpenRouterOpenAIGPT5Codex));
        assert!(openrouter_models.contains(&ModelId::OpenRouterOpenAIO4Mini));
        assert!(openrouter_models.contains(&ModelId::OpenRouterOpenAIO3Mini));
        assert!(openrouter_models.contains(&ModelId::OpenRouterAnthropicClaudeSonnet45));
        assert!(openrouter_models.contains(&ModelId::OpenRouterAnthropicClaudeOpus41));

        let xai_models = ModelId::models_for_provider(Provider::XAI);
        assert!(xai_models.contains(&ModelId::XaiGrok2Latest));
        assert!(xai_models.contains(&ModelId::XaiGrok2));
        assert!(xai_models.contains(&ModelId::XaiGrok2Mini));
        assert!(xai_models.contains(&ModelId::XaiGrok2Reasoning));
        assert!(xai_models.contains(&ModelId::XaiGrok2Vision));
    }

    #[test]
    fn test_fallback_models() {
        let fallbacks = ModelId::fallback_models();
        assert!(!fallbacks.is_empty());
        assert!(fallbacks.contains(&ModelId::Gemini25Pro));
        assert!(fallbacks.contains(&ModelId::GPT5));
        assert!(fallbacks.contains(&ModelId::ClaudeOpus41));
        assert!(fallbacks.contains(&ModelId::ClaudeSonnet45));
        assert!(fallbacks.contains(&ModelId::DeepSeekReasoner));
        assert!(fallbacks.contains(&ModelId::XaiGrok2Latest));
        assert!(fallbacks.contains(&ModelId::OpenRouterGrokCodeFast1));
    }
}
