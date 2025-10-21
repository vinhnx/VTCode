//! Model configuration and identification module
//!
//! This module provides a centralized enum for model identifiers and their configurations,
//! replacing hardcoded model strings throughout the codebase for better maintainability.
//! Read the model list in `docs/models.json`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy)]
pub struct OpenRouterMetadata {
    id: &'static str,
    vendor: &'static str,
    display: &'static str,
    description: &'static str,
    efficient: bool,
    top_tier: bool,
    generation: &'static str,
    reasoning: bool,
    tool_call: bool,
}

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
    /// xAI Grok models
    XAI,
    /// Z.AI GLM models
    ZAI,
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
            Provider::XAI => "XAI_API_KEY",
            Provider::ZAI => "ZAI_API_KEY",
        }
    }

    /// Get all supported providers
    pub fn all_providers() -> Vec<Provider> {
        vec![
            Provider::OpenAI,
            Provider::Anthropic,
            Provider::Gemini,
            Provider::DeepSeek,
            Provider::OpenRouter,
            Provider::Ollama,
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
            Provider::OpenRouter => "OpenRouter",
            Provider::Ollama => "Ollama",
            Provider::Moonshot => "Moonshot",
            Provider::XAI => "xAI",
            Provider::ZAI => "Z.AI",
        }
    }

    /// Determine if the provider supports configurable reasoning effort for the model
    pub fn supports_reasoning_effort(&self, model: &str) -> bool {
        use crate::constants::models;

        match self {
            Provider::Gemini => model == models::google::GEMINI_2_5_PRO,
            Provider::OpenAI => models::openai::REASONING_MODELS.contains(&model),
            Provider::Anthropic => models::anthropic::REASONING_MODELS.contains(&model),
            Provider::DeepSeek => model == models::deepseek::DEEPSEEK_REASONER,
            Provider::OpenRouter => {
                if let Ok(model_id) = ModelId::from_str(model) {
                    return model_id.is_reasoning_variant();
                }
                models::openrouter::REASONING_MODELS.contains(&model)
            }
            Provider::Ollama => false,
            Provider::Moonshot => false,
            Provider::XAI => model == models::xai::GROK_4 || model == models::xai::GROK_4_CODE,
            Provider::ZAI => model == models::zai::GLM_4_6,
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
            Provider::XAI => write!(f, "xai"),
            Provider::ZAI => write!(f, "zai"),
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
            "xai" => Ok(Provider::XAI),
            "zai" => Ok(Provider::ZAI),
            _ => Err(ModelParseError::InvalidProvider(s.to_string())),
        }
    }
}

/// Centralized enum for all supported model identifiers
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelId {
    // Gemini models
    /// Gemini 2.5 Flash Preview - Latest fast model with advanced capabilities
    #[default]
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
    /// GPT-OSS 20B - OpenAI's open-source 20B parameter model using harmony
    OpenAIGptOss20b,
    /// GPT-OSS 120B - OpenAI's open-source 120B parameter model using harmony
    OpenAIGptOss120b,

    // Anthropic models
    /// Claude Opus 4.1 - Latest most capable Anthropic model (2025-08-05)
    ClaudeOpus41,
    /// Claude Sonnet 4.5 - Latest balanced Anthropic model (2025-10-15)
    ClaudeSonnet45,
    /// Claude Haiku 4.5 - Latest efficient Anthropic model (2025-10-15)
    ClaudeHaiku45,
    /// Claude Sonnet 4 - Previous balanced Anthropic model (2025-05-14)
    ClaudeSonnet4,

    // DeepSeek models
    /// DeepSeek V3.2-Exp Chat - Non-thinking mode
    DeepSeekChat,
    /// DeepSeek V3.2-Exp Reasoner - Thinking mode with deliberate reasoning output
    DeepSeekReasoner,

    // xAI models
    /// Grok-4 - Flagship xAI model with advanced reasoning
    XaiGrok4,
    /// Grok-4 Mini - Efficient xAI model variant
    XaiGrok4Mini,
    /// Grok-4 Code - Code-focused Grok deployment
    XaiGrok4Code,
    /// Grok-4 Code Latest - Latest Grok code model with enhanced reasoning tools
    XaiGrok4CodeLatest,
    /// Grok-4 Vision - Multimodal Grok model
    XaiGrok4Vision,

    // Z.AI models
    /// GLM-4.6 - Latest flagship GLM reasoning model
    ZaiGlm46,
    /// GLM-4.5 - Balanced GLM release for general tasks
    ZaiGlm45,
    /// GLM-4.5-Air - Efficient GLM variant
    ZaiGlm45Air,
    /// GLM-4.5-X - Enhanced capability GLM variant
    ZaiGlm45X,
    /// GLM-4.5-AirX - Hybrid efficient GLM variant
    ZaiGlm45Airx,
    /// GLM-4.5-Flash - Low-latency GLM variant
    ZaiGlm45Flash,
    /// GLM-4-32B-0414-128K - Legacy long-context GLM deployment
    ZaiGlm432b0414128k,

    // Moonshot.ai models
    /// Kimi K2 Turbo Preview - Recommended high-speed K2 deployment
    MoonshotKimiK2TurboPreview,
    /// Kimi K2 0905 Preview - Flagship 256K K2 release with enhanced coding agents
    MoonshotKimiK20905Preview,
    /// Kimi K2 0711 Preview - Long-context K2 release tuned for balanced workloads
    MoonshotKimiK20711Preview,
    /// Kimi Latest - Auto-tier alias that selects 8K/32K/128K variants automatically
    MoonshotKimiLatest,
    /// Kimi Latest 8K - Vision-enabled 8K tier with automatic context caching
    MoonshotKimiLatest8k,
    /// Kimi Latest 32K - Vision-enabled mid-tier with extended context
    MoonshotKimiLatest32k,
    /// Kimi Latest 128K - Vision-enabled flagship tier with maximum context
    MoonshotKimiLatest128k,

    // Ollama models
    /// Qwen3 1.7B - Qwen3 1.7B model served via Ollama
    OllamaQwen317b,

    // OpenRouter models
    /// Grok Code Fast 1 - Fast OpenRouter coding model powered by xAI Grok
    OpenRouterGrokCodeFast1,
    /// Grok 4 Fast - Reasoning-focused Grok endpoint with transparent traces
    OpenRouterGrok4Fast,
    /// Grok 4 - Flagship Grok 4 endpoint exposed through OpenRouter
    OpenRouterGrok4,
    /// GLM 4.6 - Z.AI GLM 4.6 long-context reasoning model
    OpenRouterZaiGlm46,
    /// Kimi K2 0905 - MoonshotAI Kimi K2 0905 MoE release optimised for coding agents
    OpenRouterMoonshotaiKimiK20905,
    /// Kimi K2 (free) - Community tier for MoonshotAI Kimi K2
    OpenRouterMoonshotaiKimiK2Free,
    /// Qwen3 Max - Flagship Qwen3 mixture for general reasoning
    OpenRouterQwen3Max,
    /// Qwen3 235B A22B - Mixture-of-experts Qwen3 235B general model
    OpenRouterQwen3235bA22b,
    /// Qwen3 235B A22B (free) - Community tier for Qwen3 235B A22B
    OpenRouterQwen3235bA22bFree,
    /// Qwen3 235B A22B Instruct 2507 - Instruction-tuned Qwen3 235B A22B
    OpenRouterQwen3235bA22b2507,
    /// Qwen3 235B A22B Thinking 2507 - Deliberative Qwen3 235B A22B reasoning release
    OpenRouterQwen3235bA22bThinking2507,
    /// Qwen3 32B - Dense 32B Qwen3 deployment
    OpenRouterQwen332b,
    /// Qwen3 30B A3B - Active-parameter 30B Qwen3 model
    OpenRouterQwen330bA3b,
    /// Qwen3 30B A3B (free) - Community tier for Qwen3 30B A3B
    OpenRouterQwen330bA3bFree,
    /// Qwen3 30B A3B Instruct 2507 - Instruction-tuned Qwen3 30B A3B
    OpenRouterQwen330bA3bInstruct2507,
    /// Qwen3 30B A3B Thinking 2507 - Deliberative Qwen3 30B A3B release
    OpenRouterQwen330bA3bThinking2507,
    /// Qwen3 14B - Lightweight Qwen3 14B model
    OpenRouterQwen314b,
    /// Qwen3 14B (free) - Community tier for Qwen3 14B
    OpenRouterQwen314bFree,
    /// Qwen3 8B - Compact Qwen3 8B deployment
    OpenRouterQwen38b,
    /// Qwen3 8B (free) - Community tier for Qwen3 8B
    OpenRouterQwen38bFree,
    /// Qwen3 4B (free) - Entry level Qwen3 4B deployment
    OpenRouterQwen34bFree,
    /// Qwen3 Next 80B A3B Instruct - Next-generation Qwen3 instruction model
    OpenRouterQwen3Next80bA3bInstruct,
    /// Qwen3 Next 80B A3B Thinking - Next-generation Qwen3 reasoning release
    OpenRouterQwen3Next80bA3bThinking,
    /// Qwen3 Coder - Qwen3-based coding model tuned for IDE workflows
    OpenRouterQwen3Coder,
    /// Qwen3 Coder (free) - Community tier for Qwen3 Coder
    OpenRouterQwen3CoderFree,
    /// Qwen3 Coder Plus - Premium Qwen3 coding model with long context
    OpenRouterQwen3CoderPlus,
    /// Qwen3 Coder Flash - Latency optimised Qwen3 coding model
    OpenRouterQwen3CoderFlash,
    /// Qwen3 Coder 30B A3B Instruct - Large Mixture-of-Experts coding deployment
    OpenRouterQwen3Coder30bA3bInstruct,
    /// DeepSeek V3.2 Exp - Experimental DeepSeek V3.2 listing
    OpenRouterDeepSeekV32Exp,
    /// DeepSeek Chat v3.1 - Advanced DeepSeek model via OpenRouter
    OpenRouterDeepSeekChatV31,
    /// DeepSeek R1 - DeepSeek R1 reasoning model with chain-of-thought
    OpenRouterDeepSeekR1,
    /// DeepSeek Chat v3.1 (free) - Community tier for DeepSeek Chat v3.1
    OpenRouterDeepSeekChatV31Free,
    /// Nemotron Nano 9B v2 (free) - NVIDIA Nemotron Nano 9B v2 community tier
    OpenRouterNvidiaNemotronNano9bV2Free,
    /// OpenAI gpt-oss-120b - Open-weight 120B reasoning model via OpenRouter
    OpenRouterOpenAIGptOss120b,
    /// OpenAI gpt-oss-20b - Open-weight 20B deployment via OpenRouter
    OpenRouterOpenAIGptOss20b,
    /// OpenAI gpt-oss-20b (free) - Community tier for OpenAI gpt-oss-20b
    OpenRouterOpenAIGptOss20bFree,
    /// OpenAI GPT-5 - OpenAI GPT-5 model accessed through OpenRouter
    OpenRouterOpenAIGpt5,
    /// OpenAI GPT-5 Codex - OpenRouter listing for GPT-5 Codex
    OpenRouterOpenAIGpt5Codex,
    /// OpenAI GPT-5 Chat - Chat optimised GPT-5 endpoint without tool use
    OpenRouterOpenAIGpt5Chat,
    /// OpenAI GPT-4o Search Preview - GPT-4o search preview endpoint via OpenRouter
    OpenRouterOpenAIGpt4oSearchPreview,
    /// OpenAI GPT-4o Mini Search Preview - GPT-4o mini search preview endpoint
    OpenRouterOpenAIGpt4oMiniSearchPreview,
    /// OpenAI ChatGPT-4o Latest - ChatGPT 4o latest listing via OpenRouter
    OpenRouterOpenAIChatgpt4oLatest,
    /// Claude Sonnet 4.5 - Anthropic Claude Sonnet 4.5 listing
    OpenRouterAnthropicClaudeSonnet45,
    /// Claude Haiku 4.5 - Anthropic Claude Haiku 4.5 listing
    OpenRouterAnthropicClaudeHaiku45,
    /// Claude Opus 4.1 - Anthropic Claude Opus 4.1 listing
    OpenRouterAnthropicClaudeOpus41,
}

pub mod openrouter_generated {
    include!(concat!(env!("OUT_DIR"), "/openrouter_metadata.rs"));
}

impl ModelId {
    fn openrouter_metadata(&self) -> Option<OpenRouterMetadata> {
        openrouter_generated::metadata_for(*self)
    }

    fn parse_openrouter_model(value: &str) -> Option<Self> {
        openrouter_generated::parse_model(value)
    }

    fn openrouter_vendor_groups() -> Vec<(&'static str, &'static [Self])> {
        openrouter_generated::vendor_groups()
            .iter()
            .map(|group| (group.vendor, group.models))
            .collect()
    }

    fn openrouter_models() -> Vec<Self> {
        Self::openrouter_vendor_groups()
            .into_iter()
            .flat_map(|(_, models)| models.iter().copied())
            .collect()
    }

    /// Convert the model identifier to its string representation
    /// used in API calls and configurations
    pub fn as_str(&self) -> &'static str {
        use crate::constants::models;
        if let Some(meta) = self.openrouter_metadata() {
            return meta.id;
        }
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
            ModelId::ClaudeHaiku45 => models::CLAUDE_HAIKU_4_5,
            ModelId::ClaudeSonnet4 => models::CLAUDE_SONNET_4_20250514,
            // DeepSeek models
            ModelId::DeepSeekChat => models::DEEPSEEK_CHAT,
            ModelId::DeepSeekReasoner => models::DEEPSEEK_REASONER,
            // xAI models
            ModelId::XaiGrok4 => models::xai::GROK_4,
            ModelId::XaiGrok4Mini => models::xai::GROK_4_MINI,
            ModelId::XaiGrok4Code => models::xai::GROK_4_CODE,
            ModelId::XaiGrok4CodeLatest => models::xai::GROK_4_CODE_LATEST,
            ModelId::XaiGrok4Vision => models::xai::GROK_4_VISION,
            // Z.AI models
            ModelId::ZaiGlm46 => models::zai::GLM_4_6,
            ModelId::ZaiGlm45 => models::zai::GLM_4_5,
            ModelId::ZaiGlm45Air => models::zai::GLM_4_5_AIR,
            ModelId::ZaiGlm45X => models::zai::GLM_4_5_X,
            ModelId::ZaiGlm45Airx => models::zai::GLM_4_5_AIRX,
            ModelId::ZaiGlm45Flash => models::zai::GLM_4_5_FLASH,
            ModelId::ZaiGlm432b0414128k => models::zai::GLM_4_32B_0414_128K,
            // Moonshot models
            ModelId::MoonshotKimiK2TurboPreview => models::MOONSHOT_KIMI_K2_TURBO_PREVIEW,
            ModelId::MoonshotKimiK20905Preview => models::MOONSHOT_KIMI_K2_0905_PREVIEW,
            ModelId::MoonshotKimiK20711Preview => models::MOONSHOT_KIMI_K2_0711_PREVIEW,
            ModelId::MoonshotKimiLatest => models::MOONSHOT_KIMI_LATEST,
            ModelId::MoonshotKimiLatest8k => models::MOONSHOT_KIMI_LATEST_8K,
            ModelId::MoonshotKimiLatest32k => models::MOONSHOT_KIMI_LATEST_32K,
            ModelId::MoonshotKimiLatest128k => models::MOONSHOT_KIMI_LATEST_128K,
            // Ollama models
            ModelId::OllamaQwen317b => models::ollama::QWEN3_1_7B,
            // OpenRouter models
            _ => unreachable!(),
        }
    }

    /// Get the provider for this model
    pub fn provider(&self) -> Provider {
        if self.openrouter_metadata().is_some() {
            return Provider::OpenRouter;
        }
        match self {
            ModelId::Gemini25FlashPreview
            | ModelId::Gemini25Flash
            | ModelId::Gemini25FlashLite
            | ModelId::Gemini25Pro => Provider::Gemini,
            ModelId::GPT5
            | ModelId::GPT5Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::CodexMiniLatest
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => Provider::OpenAI,
            ModelId::ClaudeOpus41
            | ModelId::ClaudeSonnet45
            | ModelId::ClaudeHaiku45
            | ModelId::ClaudeSonnet4 => Provider::Anthropic,
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => Provider::DeepSeek,
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => Provider::XAI,
            ModelId::ZaiGlm46
            | ModelId::ZaiGlm45
            | ModelId::ZaiGlm45Air
            | ModelId::ZaiGlm45X
            | ModelId::ZaiGlm45Airx
            | ModelId::ZaiGlm45Flash
            | ModelId::ZaiGlm432b0414128k => Provider::ZAI,
            ModelId::MoonshotKimiK2TurboPreview
            | ModelId::MoonshotKimiK20905Preview
            | ModelId::MoonshotKimiK20711Preview
            | ModelId::MoonshotKimiLatest
            | ModelId::MoonshotKimiLatest8k
            | ModelId::MoonshotKimiLatest32k
            | ModelId::MoonshotKimiLatest128k => Provider::Moonshot,
            ModelId::OllamaQwen317b => Provider::Ollama,
            _ => unreachable!(),
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }

    /// Get the display name for the model (human-readable)
    pub fn display_name(&self) -> &'static str {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.display;
        }
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
            ModelId::ClaudeHaiku45 => "Claude Haiku 4.5",
            ModelId::ClaudeSonnet4 => "Claude Sonnet 4",
            // DeepSeek models
            ModelId::DeepSeekChat => "DeepSeek V3.2-Exp (Chat)",
            ModelId::DeepSeekReasoner => "DeepSeek V3.2-Exp (Reasoner)",
            // xAI models
            ModelId::XaiGrok4 => "Grok-4",
            ModelId::XaiGrok4Mini => "Grok-4 Mini",
            ModelId::XaiGrok4Code => "Grok-4 Code",
            ModelId::XaiGrok4CodeLatest => "Grok-4 Code Latest",
            ModelId::XaiGrok4Vision => "Grok-4 Vision",
            // Z.AI models
            ModelId::ZaiGlm46 => "GLM 4.6",
            ModelId::ZaiGlm45 => "GLM 4.5",
            ModelId::ZaiGlm45Air => "GLM 4.5 Air",
            ModelId::ZaiGlm45X => "GLM 4.5 X",
            ModelId::ZaiGlm45Airx => "GLM 4.5 AirX",
            ModelId::ZaiGlm45Flash => "GLM 4.5 Flash",
            ModelId::ZaiGlm432b0414128k => "GLM 4 32B 0414 128K",
            // Moonshot models
            ModelId::MoonshotKimiK2TurboPreview => "Kimi K2 Turbo Preview",
            ModelId::MoonshotKimiK20905Preview => "Kimi K2 0905 Preview",
            ModelId::MoonshotKimiK20711Preview => "Kimi K2 0711 Preview",
            ModelId::MoonshotKimiLatest => "Kimi Latest (auto-tier)",
            ModelId::MoonshotKimiLatest8k => "Kimi Latest 8K",
            ModelId::MoonshotKimiLatest32k => "Kimi Latest 32K",
            ModelId::MoonshotKimiLatest128k => "Kimi Latest 128K",
            // Ollama models
            ModelId::OllamaQwen317b => "Qwen3 1.7B (local)",
            // OpenRouter models
            _ => unreachable!(),
        }
    }

    /// Get a description of the model's characteristics
    pub fn description(&self) -> &'static str {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.description;
        }
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
            ModelId::OpenAIGptOss20b => {
                "OpenAI's open-source 20B parameter GPT-OSS model using harmony tokenization"
            }
            ModelId::OpenAIGptOss120b => {
                "OpenAI's open-source 120B parameter GPT-OSS model using harmony tokenization"
            }
            // Anthropic models
            ModelId::ClaudeOpus41 => "Latest most capable Anthropic model with advanced reasoning",
            ModelId::ClaudeSonnet45 => "Latest balanced Anthropic model for general tasks",
            ModelId::ClaudeHaiku45 => {
                "Latest efficient Anthropic model optimized for low-latency agent workflows"
            }
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
            ModelId::XaiGrok4 => "Flagship Grok 4 model with long context and tool use",
            ModelId::XaiGrok4Mini => "Efficient Grok 4 Mini tuned for low latency",
            ModelId::XaiGrok4Code => "Code-specialized Grok 4 deployment with tool support",
            ModelId::XaiGrok4CodeLatest => {
                "Latest Grok 4 code model offering enhanced reasoning traces"
            }
            ModelId::XaiGrok4Vision => "Multimodal Grok 4 model with image understanding",
            // Z.AI models
            ModelId::ZaiGlm46 => {
                "Latest Z.AI GLM flagship with long-context reasoning and coding strengths"
            }
            ModelId::ZaiGlm45 => "Balanced GLM 4.5 release for general assistant tasks",
            ModelId::ZaiGlm45Air => "Efficient GLM 4.5 Air variant tuned for lower latency",
            ModelId::ZaiGlm45X => "Enhanced GLM 4.5 X variant with improved reasoning",
            ModelId::ZaiGlm45Airx => "Hybrid GLM 4.5 AirX variant blending efficiency with quality",
            ModelId::ZaiGlm45Flash => "Low-latency GLM 4.5 Flash optimized for responsiveness",
            ModelId::ZaiGlm432b0414128k => {
                "Legacy GLM 4 32B deployment offering extended 128K context window"
            }
            // Moonshot models
            ModelId::MoonshotKimiK2TurboPreview => {
                "Recommended high-speed Kimi K2 turbo variant with 256K context and 60+ tok/s output"
            }
            ModelId::MoonshotKimiK20905Preview => {
                "Latest Kimi K2 0905 flagship with enhanced agentic coding, 256K context, and richer tool support"
            }
            ModelId::MoonshotKimiK20711Preview => {
                "Kimi K2 0711 preview tuned for balanced cost and capability with 131K context"
            }
            ModelId::MoonshotKimiLatest => {
                "Auto-tier alias that selects the right Kimi Latest vision tier (8K/32K/128K) with context caching"
            }
            ModelId::MoonshotKimiLatest8k => {
                "Kimi Latest 8K vision tier for short tasks with automatic context caching"
            }
            ModelId::MoonshotKimiLatest32k => {
                "Kimi Latest 32K vision tier blending longer context with latest assistant features"
            }
            ModelId::MoonshotKimiLatest128k => {
                "Kimi Latest 128K flagship vision tier delivering maximum context and newest capabilities"
            }
            ModelId::OllamaQwen317b => {
                "Qwen3 1.7B served locally through Ollama without external API requirements"
            }
            _ => unreachable!(),
        }
    }

    /// Return the OpenRouter vendor slug when this identifier maps to a marketplace listing
    pub fn openrouter_vendor(&self) -> Option<&'static str> {
        self.openrouter_metadata().map(|meta| meta.vendor)
    }

    /// Get all available models as a vector
    pub fn all_models() -> Vec<ModelId> {
        let mut models = vec![
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
            ModelId::ClaudeHaiku45,
            ModelId::ClaudeSonnet4,
            // DeepSeek models
            ModelId::DeepSeekChat,
            ModelId::DeepSeekReasoner,
            // xAI models
            ModelId::XaiGrok4,
            ModelId::XaiGrok4Mini,
            ModelId::XaiGrok4Code,
            ModelId::XaiGrok4CodeLatest,
            ModelId::XaiGrok4Vision,
            // Z.AI models
            ModelId::ZaiGlm46,
            ModelId::ZaiGlm45,
            ModelId::ZaiGlm45Air,
            ModelId::ZaiGlm45X,
            ModelId::ZaiGlm45Airx,
            ModelId::ZaiGlm45Flash,
            ModelId::ZaiGlm432b0414128k,
            // Moonshot models
            ModelId::MoonshotKimiK2TurboPreview,
            ModelId::MoonshotKimiK20905Preview,
            ModelId::MoonshotKimiK20711Preview,
            ModelId::MoonshotKimiLatest,
            ModelId::MoonshotKimiLatest8k,
            ModelId::MoonshotKimiLatest32k,
            ModelId::MoonshotKimiLatest128k,
            // Ollama models
            ModelId::OllamaQwen317b,
        ];
        models.extend(Self::openrouter_models());
        models
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
            ModelId::OpenAIGptOss20b,
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::DeepSeekReasoner,
            ModelId::MoonshotKimiK20905Preview,
            ModelId::XaiGrok4,
            ModelId::ZaiGlm46,
            ModelId::OpenRouterGrokCodeFast1,
        ]
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
            Provider::Moonshot => ModelId::MoonshotKimiK20905Preview,
            Provider::XAI => ModelId::XaiGrok4,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaQwen317b,
            Provider::ZAI => ModelId::ZaiGlm46,
        }
    }

    /// Get provider-specific defaults for subagent
    pub fn default_subagent_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5Mini,
            Provider::Anthropic => ModelId::ClaudeSonnet45,
            Provider::DeepSeek => ModelId::DeepSeekChat,
            Provider::Moonshot => ModelId::MoonshotKimiK2TurboPreview,
            Provider::XAI => ModelId::XaiGrok4Code,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaQwen317b,
            Provider::ZAI => ModelId::ZaiGlm45Flash,
        }
    }

    /// Get provider-specific defaults for single agent
    pub fn default_single_for_provider(provider: Provider) -> Self {
        match provider {
            Provider::Gemini => ModelId::Gemini25FlashPreview,
            Provider::OpenAI => ModelId::GPT5,
            Provider::Anthropic => ModelId::ClaudeOpus41,
            Provider::DeepSeek => ModelId::DeepSeekReasoner,
            Provider::Moonshot => ModelId::MoonshotKimiK2TurboPreview,
            Provider::XAI => ModelId::XaiGrok4,
            Provider::OpenRouter => ModelId::OpenRouterGrokCodeFast1,
            Provider::Ollama => ModelId::OllamaQwen317b,
            Provider::ZAI => ModelId::ZaiGlm46,
        }
    }

    /// Check if this is a "flash" variant (optimized for speed)
    pub fn is_flash_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25FlashPreview
                | ModelId::Gemini25Flash
                | ModelId::Gemini25FlashLite
                | ModelId::ZaiGlm45Flash
                | ModelId::MoonshotKimiK2TurboPreview
                | ModelId::MoonshotKimiLatest8k
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
                | ModelId::XaiGrok4
                | ModelId::ZaiGlm46
                | ModelId::MoonshotKimiK20905Preview
                | ModelId::MoonshotKimiLatest128k
        )
    }

    /// Check if this is an optimized/efficient variant
    pub fn is_efficient_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.efficient;
        }
        matches!(
            self,
            ModelId::Gemini25FlashPreview
                | ModelId::Gemini25Flash
                | ModelId::Gemini25FlashLite
                | ModelId::GPT5Mini
                | ModelId::GPT5Nano
                | ModelId::ClaudeHaiku45
                | ModelId::DeepSeekChat
                | ModelId::XaiGrok4Code
                | ModelId::ZaiGlm45Air
                | ModelId::ZaiGlm45Airx
                | ModelId::ZaiGlm45Flash
                | ModelId::MoonshotKimiK2TurboPreview
                | ModelId::MoonshotKimiLatest8k
        )
    }

    /// Check if this is a top-tier model
    pub fn is_top_tier(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.top_tier;
        }
        matches!(
            self,
            ModelId::Gemini25Pro
                | ModelId::GPT5
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus41
                | ModelId::ClaudeSonnet45
                | ModelId::ClaudeSonnet4
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::XaiGrok4CodeLatest
                | ModelId::ZaiGlm46
                | ModelId::MoonshotKimiK20905Preview
                | ModelId::MoonshotKimiLatest128k
        )
    }

    /// Determine whether the model is a reasoning-capable variant
    pub fn is_reasoning_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.reasoning;
        }
        self.provider().supports_reasoning_effort(self.as_str())
    }

    /// Determine whether the model supports tool calls/function execution
    pub fn supports_tool_calls(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.tool_call;
        }
        true
    }

    /// Get the generation/version string for this model
    pub fn generation(&self) -> &'static str {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.generation;
        }
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
            ModelId::ClaudeSonnet45 | ModelId::ClaudeHaiku45 => "4.5",
            ModelId::ClaudeSonnet4 => "4",
            ModelId::ClaudeOpus41 => "4.1",
            // DeepSeek generations
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => "V3.2-Exp",
            // xAI generations
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => "4",
            // Z.AI generations
            ModelId::ZaiGlm46 => "4.6",
            ModelId::ZaiGlm45
            | ModelId::ZaiGlm45Air
            | ModelId::ZaiGlm45X
            | ModelId::ZaiGlm45Airx
            | ModelId::ZaiGlm45Flash => "4.5",
            ModelId::ZaiGlm432b0414128k => "4-32B",
            // Moonshot generations
            ModelId::MoonshotKimiK2TurboPreview
            | ModelId::MoonshotKimiK20905Preview
            | ModelId::MoonshotKimiK20711Preview => "k2",
            ModelId::MoonshotKimiLatest
            | ModelId::MoonshotKimiLatest8k
            | ModelId::MoonshotKimiLatest32k
            | ModelId::MoonshotKimiLatest128k => "latest",
            ModelId::OllamaQwen317b => "oss",
            _ => unreachable!(),
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
        use crate::constants::models;
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
            s if s == models::openai::GPT_OSS_20B => Ok(ModelId::OpenAIGptOss20b),
            s if s == models::openai::GPT_OSS_120B => Ok(ModelId::OpenAIGptOss120b),
            // Anthropic models
            s if s == models::CLAUDE_OPUS_4_1_20250805 => Ok(ModelId::ClaudeOpus41),
            s if s == models::CLAUDE_SONNET_4_5 => Ok(ModelId::ClaudeSonnet45),
            s if s == models::CLAUDE_HAIKU_4_5 => Ok(ModelId::ClaudeHaiku45),
            s if s == models::CLAUDE_SONNET_4_20250514 => Ok(ModelId::ClaudeSonnet4),
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
            s if s == models::zai::GLM_4_6 => Ok(ModelId::ZaiGlm46),
            s if s == models::zai::GLM_4_5 => Ok(ModelId::ZaiGlm45),
            s if s == models::zai::GLM_4_5_AIR => Ok(ModelId::ZaiGlm45Air),
            s if s == models::zai::GLM_4_5_X => Ok(ModelId::ZaiGlm45X),
            s if s == models::zai::GLM_4_5_AIRX => Ok(ModelId::ZaiGlm45Airx),
            s if s == models::zai::GLM_4_5_FLASH => Ok(ModelId::ZaiGlm45Flash),
            s if s == models::zai::GLM_4_32B_0414_128K => Ok(ModelId::ZaiGlm432b0414128k),
            // Moonshot models
            s if s == models::MOONSHOT_KIMI_K2_TURBO_PREVIEW => {
                Ok(ModelId::MoonshotKimiK2TurboPreview)
            }
            s if s == models::MOONSHOT_KIMI_K2_0905_PREVIEW => {
                Ok(ModelId::MoonshotKimiK20905Preview)
            }
            s if s == models::MOONSHOT_KIMI_K2_0711_PREVIEW => {
                Ok(ModelId::MoonshotKimiK20711Preview)
            }
            s if s == models::MOONSHOT_KIMI_LATEST => Ok(ModelId::MoonshotKimiLatest),
            s if s == models::MOONSHOT_KIMI_LATEST_8K => Ok(ModelId::MoonshotKimiLatest8k),
            s if s == models::MOONSHOT_KIMI_LATEST_32K => Ok(ModelId::MoonshotKimiLatest32k),
            s if s == models::MOONSHOT_KIMI_LATEST_128K => Ok(ModelId::MoonshotKimiLatest128k),
            s if s == models::ollama::QWEN3_1_7B => Ok(ModelId::OllamaQwen317b),
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
    use crate::constants::models;

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
        assert_eq!(ModelId::ClaudeHaiku45.as_str(), models::CLAUDE_HAIKU_4_5);
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
        assert_eq!(ModelId::XaiGrok4.as_str(), models::xai::GROK_4);
        assert_eq!(ModelId::XaiGrok4Mini.as_str(), models::xai::GROK_4_MINI);
        assert_eq!(ModelId::XaiGrok4Code.as_str(), models::xai::GROK_4_CODE);
        assert_eq!(
            ModelId::XaiGrok4CodeLatest.as_str(),
            models::xai::GROK_4_CODE_LATEST
        );
        assert_eq!(ModelId::XaiGrok4Vision.as_str(), models::xai::GROK_4_VISION);
        // Z.AI models
        assert_eq!(ModelId::ZaiGlm46.as_str(), models::zai::GLM_4_6);
        assert_eq!(ModelId::ZaiGlm45.as_str(), models::zai::GLM_4_5);
        assert_eq!(ModelId::ZaiGlm45Air.as_str(), models::zai::GLM_4_5_AIR);
        assert_eq!(ModelId::ZaiGlm45X.as_str(), models::zai::GLM_4_5_X);
        assert_eq!(ModelId::ZaiGlm45Airx.as_str(), models::zai::GLM_4_5_AIRX);
        assert_eq!(ModelId::ZaiGlm45Flash.as_str(), models::zai::GLM_4_5_FLASH);
        assert_eq!(
            ModelId::ZaiGlm432b0414128k.as_str(),
            models::zai::GLM_4_32B_0414_128K
        );
        for entry in openrouter_generated::ENTRIES {
            assert_eq!(entry.variant.as_str(), entry.id);
        }
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
        assert_eq!(
            models::openai::GPT_OSS_20B.parse::<ModelId>().unwrap(),
            ModelId::OpenAIGptOss20b
        );
        assert_eq!(
            models::openai::GPT_OSS_120B.parse::<ModelId>().unwrap(),
            ModelId::OpenAIGptOss120b
        );
        // Anthropic models
        assert_eq!(
            models::CLAUDE_SONNET_4_5.parse::<ModelId>().unwrap(),
            ModelId::ClaudeSonnet45
        );
        assert_eq!(
            models::CLAUDE_HAIKU_4_5.parse::<ModelId>().unwrap(),
            ModelId::ClaudeHaiku45
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
            models::xai::GROK_4.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok4
        );
        assert_eq!(
            models::xai::GROK_4_MINI.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok4Mini
        );
        assert_eq!(
            models::xai::GROK_4_CODE.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok4Code
        );
        assert_eq!(
            models::xai::GROK_4_CODE_LATEST.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok4CodeLatest
        );
        assert_eq!(
            models::xai::GROK_4_VISION.parse::<ModelId>().unwrap(),
            ModelId::XaiGrok4Vision
        );
        // Z.AI models
        assert_eq!(
            models::zai::GLM_4_6.parse::<ModelId>().unwrap(),
            ModelId::ZaiGlm46
        );
        assert_eq!(
            models::zai::GLM_4_5.parse::<ModelId>().unwrap(),
            ModelId::ZaiGlm45
        );
        assert_eq!(
            models::zai::GLM_4_5_AIR.parse::<ModelId>().unwrap(),
            ModelId::ZaiGlm45Air
        );
        assert_eq!(
            models::zai::GLM_4_5_X.parse::<ModelId>().unwrap(),
            ModelId::ZaiGlm45X
        );
        assert_eq!(
            models::zai::GLM_4_5_AIRX.parse::<ModelId>().unwrap(),
            ModelId::ZaiGlm45Airx
        );
        assert_eq!(
            models::zai::GLM_4_5_FLASH.parse::<ModelId>().unwrap(),
            ModelId::ZaiGlm45Flash
        );
        assert_eq!(
            models::zai::GLM_4_32B_0414_128K.parse::<ModelId>().unwrap(),
            ModelId::ZaiGlm432b0414128k
        );
        assert_eq!(
            models::MOONSHOT_KIMI_K2_TURBO_PREVIEW
                .parse::<ModelId>()
                .unwrap(),
            ModelId::MoonshotKimiK2TurboPreview
        );
        assert_eq!(
            models::MOONSHOT_KIMI_K2_0905_PREVIEW
                .parse::<ModelId>()
                .unwrap(),
            ModelId::MoonshotKimiK20905Preview
        );
        assert_eq!(
            models::MOONSHOT_KIMI_K2_0711_PREVIEW
                .parse::<ModelId>()
                .unwrap(),
            ModelId::MoonshotKimiK20711Preview
        );
        assert_eq!(
            models::MOONSHOT_KIMI_LATEST.parse::<ModelId>().unwrap(),
            ModelId::MoonshotKimiLatest
        );
        assert_eq!(
            models::MOONSHOT_KIMI_LATEST_8K.parse::<ModelId>().unwrap(),
            ModelId::MoonshotKimiLatest8k
        );
        assert_eq!(
            models::MOONSHOT_KIMI_LATEST_32K.parse::<ModelId>().unwrap(),
            ModelId::MoonshotKimiLatest32k
        );
        assert_eq!(
            models::MOONSHOT_KIMI_LATEST_128K
                .parse::<ModelId>()
                .unwrap(),
            ModelId::MoonshotKimiLatest128k
        );
        for entry in openrouter_generated::ENTRIES {
            assert_eq!(entry.id.parse::<ModelId>().unwrap(), entry.variant);
        }
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
        assert_eq!("zai".parse::<Provider>().unwrap(), Provider::ZAI);
        assert_eq!("moonshot".parse::<Provider>().unwrap(), Provider::Moonshot);
        assert!("invalid-provider".parse::<Provider>().is_err());
    }

    #[test]
    fn test_model_providers() {
        assert_eq!(ModelId::Gemini25FlashPreview.provider(), Provider::Gemini);
        assert_eq!(ModelId::GPT5.provider(), Provider::OpenAI);
        assert_eq!(ModelId::GPT5Codex.provider(), Provider::OpenAI);
        assert_eq!(ModelId::ClaudeSonnet45.provider(), Provider::Anthropic);
        assert_eq!(ModelId::ClaudeHaiku45.provider(), Provider::Anthropic);
        assert_eq!(ModelId::ClaudeSonnet4.provider(), Provider::Anthropic);
        assert_eq!(ModelId::DeepSeekChat.provider(), Provider::DeepSeek);
        assert_eq!(ModelId::XaiGrok4.provider(), Provider::XAI);
        assert_eq!(ModelId::ZaiGlm46.provider(), Provider::ZAI);
        assert_eq!(
            ModelId::MoonshotKimiK20905Preview.provider(),
            Provider::Moonshot
        );
        assert_eq!(ModelId::OllamaQwen317b.provider(), Provider::Ollama);
        assert_eq!(
            ModelId::OpenRouterGrokCodeFast1.provider(),
            Provider::OpenRouter
        );
        assert_eq!(
            ModelId::OpenRouterAnthropicClaudeSonnet45.provider(),
            Provider::OpenRouter
        );

        for entry in openrouter_generated::ENTRIES {
            assert_eq!(entry.variant.provider(), Provider::OpenRouter);
        }
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
            ModelId::XaiGrok4
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::Ollama),
            ModelId::OllamaQwen317b
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::ZAI),
            ModelId::ZaiGlm46
        );
        assert_eq!(
            ModelId::default_orchestrator_for_provider(Provider::Moonshot),
            ModelId::MoonshotKimiK20905Preview
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
            ModelId::XaiGrok4Code
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::Ollama),
            ModelId::OllamaQwen317b
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::ZAI),
            ModelId::ZaiGlm45Flash
        );
        assert_eq!(
            ModelId::default_subagent_for_provider(Provider::Moonshot),
            ModelId::MoonshotKimiK2TurboPreview
        );

        assert_eq!(
            ModelId::default_single_for_provider(Provider::DeepSeek),
            ModelId::DeepSeekReasoner
        );
        assert_eq!(
            ModelId::default_single_for_provider(Provider::Moonshot),
            ModelId::MoonshotKimiK2TurboPreview
        );
        assert_eq!(
            ModelId::default_single_for_provider(Provider::Ollama),
            ModelId::OllamaQwen317b
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
        assert!(ModelId::ZaiGlm45Flash.is_flash_variant());
        assert!(ModelId::MoonshotKimiK2TurboPreview.is_flash_variant());
        assert!(ModelId::MoonshotKimiLatest8k.is_flash_variant());

        // Pro variants
        assert!(ModelId::Gemini25Pro.is_pro_variant());
        assert!(ModelId::GPT5.is_pro_variant());
        assert!(ModelId::GPT5Codex.is_pro_variant());
        assert!(ModelId::DeepSeekReasoner.is_pro_variant());
        assert!(ModelId::ZaiGlm46.is_pro_variant());
        assert!(ModelId::MoonshotKimiK20905Preview.is_pro_variant());
        assert!(ModelId::MoonshotKimiLatest128k.is_pro_variant());
        assert!(!ModelId::Gemini25FlashPreview.is_pro_variant());

        // Efficient variants
        assert!(ModelId::Gemini25FlashPreview.is_efficient_variant());
        assert!(ModelId::Gemini25Flash.is_efficient_variant());
        assert!(ModelId::Gemini25FlashLite.is_efficient_variant());
        assert!(ModelId::GPT5Mini.is_efficient_variant());
        assert!(ModelId::ClaudeHaiku45.is_efficient_variant());
        assert!(ModelId::XaiGrok4Code.is_efficient_variant());
        assert!(ModelId::DeepSeekChat.is_efficient_variant());
        assert!(ModelId::ZaiGlm45Air.is_efficient_variant());
        assert!(ModelId::ZaiGlm45Airx.is_efficient_variant());
        assert!(ModelId::ZaiGlm45Flash.is_efficient_variant());
        assert!(ModelId::MoonshotKimiK2TurboPreview.is_efficient_variant());
        assert!(ModelId::MoonshotKimiLatest8k.is_efficient_variant());
        assert!(!ModelId::GPT5.is_efficient_variant());

        for entry in openrouter_generated::ENTRIES {
            assert_eq!(entry.variant.is_efficient_variant(), entry.efficient);
        }

        // Top tier models
        assert!(ModelId::Gemini25Pro.is_top_tier());
        assert!(ModelId::GPT5.is_top_tier());
        assert!(ModelId::GPT5Codex.is_top_tier());
        assert!(ModelId::ClaudeSonnet45.is_top_tier());
        assert!(ModelId::ClaudeSonnet4.is_top_tier());
        assert!(ModelId::XaiGrok4.is_top_tier());
        assert!(ModelId::XaiGrok4CodeLatest.is_top_tier());
        assert!(ModelId::DeepSeekReasoner.is_top_tier());
        assert!(ModelId::ZaiGlm46.is_top_tier());
        assert!(ModelId::MoonshotKimiK20905Preview.is_top_tier());
        assert!(ModelId::MoonshotKimiLatest128k.is_top_tier());
        assert!(!ModelId::Gemini25FlashPreview.is_top_tier());
        assert!(!ModelId::ClaudeHaiku45.is_top_tier());

        for entry in openrouter_generated::ENTRIES {
            assert_eq!(entry.variant.is_top_tier(), entry.top_tier);
        }
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
        assert_eq!(ModelId::ClaudeHaiku45.generation(), "4.5");
        assert_eq!(ModelId::ClaudeSonnet4.generation(), "4");
        assert_eq!(ModelId::ClaudeOpus41.generation(), "4.1");

        // DeepSeek generations
        assert_eq!(ModelId::DeepSeekChat.generation(), "V3.2-Exp");
        assert_eq!(ModelId::DeepSeekReasoner.generation(), "V3.2-Exp");

        // xAI generations
        assert_eq!(ModelId::XaiGrok4.generation(), "4");
        assert_eq!(ModelId::XaiGrok4Mini.generation(), "4");
        assert_eq!(ModelId::XaiGrok4Code.generation(), "4");
        assert_eq!(ModelId::XaiGrok4CodeLatest.generation(), "4");
        assert_eq!(ModelId::XaiGrok4Vision.generation(), "4");
        // Z.AI generations
        assert_eq!(ModelId::ZaiGlm46.generation(), "4.6");
        assert_eq!(ModelId::ZaiGlm45.generation(), "4.5");
        assert_eq!(ModelId::ZaiGlm45Air.generation(), "4.5");
        assert_eq!(ModelId::ZaiGlm45X.generation(), "4.5");
        assert_eq!(ModelId::ZaiGlm45Airx.generation(), "4.5");
        assert_eq!(ModelId::ZaiGlm45Flash.generation(), "4.5");
        assert_eq!(ModelId::ZaiGlm432b0414128k.generation(), "4-32B");
        assert_eq!(ModelId::MoonshotKimiK2TurboPreview.generation(), "k2");
        assert_eq!(ModelId::MoonshotKimiK20905Preview.generation(), "k2");
        assert_eq!(ModelId::MoonshotKimiK20711Preview.generation(), "k2");
        assert_eq!(ModelId::MoonshotKimiLatest.generation(), "latest");
        assert_eq!(ModelId::MoonshotKimiLatest8k.generation(), "latest");
        assert_eq!(ModelId::MoonshotKimiLatest32k.generation(), "latest");
        assert_eq!(ModelId::MoonshotKimiLatest128k.generation(), "latest");

        for entry in openrouter_generated::ENTRIES {
            assert_eq!(entry.variant.generation(), entry.generation);
        }
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
        assert!(anthropic_models.contains(&ModelId::ClaudeHaiku45));
        assert!(anthropic_models.contains(&ModelId::ClaudeSonnet4));
        assert!(!anthropic_models.contains(&ModelId::GPT5));

        let deepseek_models = ModelId::models_for_provider(Provider::DeepSeek);
        assert!(deepseek_models.contains(&ModelId::DeepSeekChat));
        assert!(deepseek_models.contains(&ModelId::DeepSeekReasoner));

        let openrouter_models = ModelId::models_for_provider(Provider::OpenRouter);
        for entry in openrouter_generated::ENTRIES {
            assert!(openrouter_models.contains(&entry.variant));
        }

        let xai_models = ModelId::models_for_provider(Provider::XAI);
        assert!(xai_models.contains(&ModelId::XaiGrok4));
        assert!(xai_models.contains(&ModelId::XaiGrok4Mini));
        assert!(xai_models.contains(&ModelId::XaiGrok4Code));
        assert!(xai_models.contains(&ModelId::XaiGrok4CodeLatest));
        assert!(xai_models.contains(&ModelId::XaiGrok4Vision));

        let zai_models = ModelId::models_for_provider(Provider::ZAI);
        assert!(zai_models.contains(&ModelId::ZaiGlm46));
        assert!(zai_models.contains(&ModelId::ZaiGlm45));
        assert!(zai_models.contains(&ModelId::ZaiGlm45Air));
        assert!(zai_models.contains(&ModelId::ZaiGlm45X));
        assert!(zai_models.contains(&ModelId::ZaiGlm45Airx));
        assert!(zai_models.contains(&ModelId::ZaiGlm45Flash));
        assert!(zai_models.contains(&ModelId::ZaiGlm432b0414128k));

        let moonshot_models = ModelId::models_for_provider(Provider::Moonshot);
        assert!(moonshot_models.contains(&ModelId::MoonshotKimiK2TurboPreview));
        assert!(moonshot_models.contains(&ModelId::MoonshotKimiK20905Preview));
        assert!(moonshot_models.contains(&ModelId::MoonshotKimiK20711Preview));
        assert!(moonshot_models.contains(&ModelId::MoonshotKimiLatest));
        assert!(moonshot_models.contains(&ModelId::MoonshotKimiLatest8k));
        assert!(moonshot_models.contains(&ModelId::MoonshotKimiLatest32k));
        assert!(moonshot_models.contains(&ModelId::MoonshotKimiLatest128k));
        assert_eq!(moonshot_models.len(), 7);

        let ollama_models = ModelId::models_for_provider(Provider::Ollama);
        assert!(ollama_models.contains(&ModelId::OllamaQwen317b));
        assert_eq!(ollama_models.len(), 1);
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
        assert!(fallbacks.contains(&ModelId::MoonshotKimiK20905Preview));
        assert!(fallbacks.contains(&ModelId::XaiGrok4));
        assert!(fallbacks.contains(&ModelId::ZaiGlm46));
        assert!(fallbacks.contains(&ModelId::OpenRouterGrokCodeFast1));
    }
}
