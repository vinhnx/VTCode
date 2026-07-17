use serde::{Deserialize, Serialize};

mod as_str;
mod capabilities;
mod collection;
mod defaults;
mod description;
mod display;
mod format;
mod openrouter;
mod parse;
mod provider;
mod table;

pub use capabilities::{
    ModelCatalogEntry, ModelPricing, catalog_provider_keys, model_catalog_entry,
    supported_models_for_provider,
};

/// Centralized enum for all supported model identifiers
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelId {
    // Gemini models
    /// Gemini 3.1 Pro Preview - Latest Gemini 3.1 Pro flagship
    Gemini31ProPreview,
    /// Gemini 3.1 Pro Preview Custom Tools - Optimized for custom tools & bash
    Gemini31ProPreviewCustomTools,
    /// Gemini 3.5 Flash - High-efficiency frontier model for fast inference
    Gemini35Flash,

    // OpenAI models
    /// GPT-5.6 Sol - Frontier model for complex professional work in the GPT-5.6 family
    GPT56Sol,
    /// GPT-5.6 Terra - GPT-5.6 model that balances intelligence and cost
    GPT56Terra,
    /// GPT-5.6 Luna - GPT-5.6 model optimized for cost-sensitive workloads
    GPT56Luna,
    /// GPT-5.5 - Next-gen OpenAI model dated release (2026-04-23)
    GPT55,
    /// GPT-5.4 - Mainline frontier GPT model for general-purpose and coding work
    GPT54,
    /// GPT-5.4 Pro - Higher-compute GPT-5.4 variant for difficult problems
    GPT54Pro,
    /// GPT-5.4 Nano - Lightweight GPT-5.4 variant optimized for speed and cost-efficiency
    GPT54Nano,
    /// GPT-5.4 Mini - Compact GPT-5.4 variant for cost-effective tasks with reduced reasoning overhead
    GPT54Mini,
    /// GPT-5.3 Codex - Code-focused GPT-5.3 variant optimized for agentic coding with reasoning effort support (low, medium, high, xhigh)
    GPT53Codex,
    /// GPT-OSS 20B - OpenAI's open-source 20B parameter model using harmony
    OpenAIGptOss20b,
    /// GPT-OSS 120B - OpenAI's open-source 120B parameter model using harmony
    OpenAIGptOss120b,

    // Anthropic models
    /// Claude Sonnet 5 - The best combination of speed and intelligence with adaptive thinking on by default
    #[default]
    ClaudeSonnet5,
    /// Claude Fable 5 - Anthropic's most capable widely released model for demanding reasoning and long-horizon agentic work
    ClaudeFable5,
    /// Claude Mythos 5 - Fable 5-class model without safety classifiers, limited availability through Project Glasswing
    ClaudeMythos5,
    /// Claude Opus 4.8 - Anthropic's most capable model for complex reasoning and agentic coding
    ClaudeOpus48,
    /// Claude Sonnet 4.6 - Balanced flagship Anthropic model in VT Code's conservative rollout
    ClaudeSonnet46,
    /// Claude Haiku 4.5 - Latest efficient Anthropic model (2025-10-15)
    ClaudeHaiku45,
    /// GitHub Copilot auto model selection
    CopilotAuto,
    /// GitHub Copilot GPT-5.2 Codex
    CopilotGPT52Codex,
    /// GitHub Copilot GPT-5.1 Codex Max
    CopilotGPT51CodexMax,
    /// GitHub Copilot GPT-5.4
    CopilotGPT54,
    /// GitHub Copilot GPT-5.4 Mini
    CopilotGPT54Mini,
    /// GitHub Copilot Claude Sonnet 4.6
    CopilotClaudeSonnet46,

    // DeepSeek models
    /// DeepSeek V4 Pro - High-performance reasoning model with advanced thinking
    DeepSeekV4Pro,
    /// DeepSeek V4 Flash - Fast inference model for cost-effective reasoning
    DeepSeekV4Flash,

    // Mistral AI models
    /// Mistral Large 3 - State-of-the-art open-weight general-purpose multimodal model
    MistralLarge3,
    // Hugging Face models
    /// OpenAI GPT-OSS 20B via Hugging Face router
    HuggingFaceOpenAIGptOss20b,
    /// OpenAI GPT-OSS 120B via Hugging Face router
    HuggingFaceOpenAIGptOss120b,
    /// Z.AI GLM-5.1 via zai-org provider on Hugging Face router
    HuggingFaceGlm51ZaiOrg,
    /// Z.AI GLM-5.2 via Novita inference provider on Hugging Face router
    HuggingFaceGlm52Novita,
    /// Kimi K2.6 via Novita on Hugging Face router
    HuggingFaceKimiK26Novita,
    /// DeepSeek V4 Flash via Novita on Hugging Face router
    HuggingFaceDeepseekV4FlashNovita,
    /// DeepSeek V4 Pro via Together on Hugging Face router
    HuggingFaceDeepseekV4ProTogether,
    /// Step 3.5 Flash via Hugging Face router
    HuggingFaceStep35Flash,
    /// Z.AI GLM-5.1 via DeepInfra inference provider on Hugging Face router
    HuggingFaceGlm51Deepinfra,
    /// MiniMax M2.7 via Novita on Hugging Face router
    HuggingFaceMinimaxM27Novita,
    /// MiniMax M3 via Novita on Hugging Face router
    HuggingFaceMinimaxM3Novita,
    /// DeepSeek V4 Pro via Novita on Hugging Face router
    HuggingFaceDeepseekV4ProNovita,

    // StepFun models
    /// Step 3.7 Flash - StepFun's flagship multimodal reasoning model with tool calling
    StepFun37Flash,

    // Evolink gateway models (namespaced as `evolink/<model>`)
    /// GPT-5.2 served through the Evolink gateway
    EvolinkGpt52,
    /// GPT-5.5 served through the Evolink gateway
    EvolinkGpt55,
    /// DeepSeek V4 Pro served through the Evolink gateway
    EvolinkDeepseekV4Pro,
    /// DeepSeek V4 Flash served through the Evolink gateway
    EvolinkDeepseekV4Flash,
    /// Doubao Seed 2.0 Pro served through the Evolink gateway
    EvolinkDoubaoSeed20Pro,
    /// Gemini 3.1 Pro served through the Evolink gateway (OpenAI SDK format)
    EvolinkGemini31Pro,
    /// Gemini 3.5 Flash served through the Evolink gateway (OpenAI SDK format)
    EvolinkGemini35Flash,
    /// MiniMax-M3 served through the Evolink gateway (OpenAI Chat Completions format)
    EvolinkMinimaxM3,
    /// Claude Sonnet 4.6 served through the Evolink gateway (Anthropic Messages API)
    EvolinkClaudeSonnet46,
    /// Claude Opus 4.8 served through the Evolink gateway (Anthropic Messages API)
    EvolinkClaudeOpus48,
    /// Claude Haiku 4.5 served through the Evolink gateway (Anthropic Messages API)
    EvolinkClaudeHaiku45,

    /// GLM-5.2 - Z.ai flagship model for long-horizon tasks with 1M context
    ZaiGlm52,
    /// GLM-5.1 - Next-gen Z.ai foundation model with improved reasoning
    ZaiGlm51,

    // MiMo models
    /// MiMo V2.5 Pro - Xiaomi's flagship reasoning model with 1M context
    MiMoV25Pro,
    /// MiMo V2.5 - Xiaomi's omni-modal model with full-modal understanding and 1M context
    MiMoV25,

    // Moonshot models
    /// Kimi K3 - Moonshot.ai's 2.8T parameter flagship with Delta Attention, native vision, 1M context
    MoonshotKimiK3,
    /// Kimi K2.7 Code - Moonshot.ai's most capable coding model with long-horizon coding breakthrough
    MoonshotKimiK27Code,
    /// Kimi K2.6 - Moonshot.ai's 1T MoE flagship (32B active, MLA, MoonViT vision)
    MoonshotKimiK26,

    // OpenCode Zen models
    /// GPT-5.4 - OpenCode Zen default flagship model
    OpenCodeZenGPT54,
    /// GPT-5.4 Mini - Lower-cost OpenCode Zen GPT option
    OpenCodeZenGPT54Mini,
    /// Claude Sonnet 4.6 - Anthropic-backed OpenCode Zen coding model
    OpenCodeZenClaudeSonnet46,
    /// GLM-5.1 - Z.AI model served through OpenCode Zen
    OpenCodeZenGlm51,

    // OpenCode Go models
    /// GLM-5.2 - Z.AI flagship model included with OpenCode Go
    OpenCodeGoGlm52,
    /// GLM-5.1 - Z.AI model included with OpenCode Go
    OpenCodeGoGlm51,
    /// Kimi K2.7 Code - Moonshot.ai's most capable coding model on OpenCode Go
    OpenCodeGoKimiK27Code,
    /// Kimi K2.6 - Moonshot.ai's 1T MoE model on OpenCode Go
    OpenCodeGoKimiK26,
    /// MiMo-V2.5 - Xiaomi's omnimodal model on OpenCode Go
    OpenCodeGoMimoV25,
    /// MiMo-V2.5-Pro - Xiaomi's flagship reasoning model on OpenCode Go
    OpenCodeGoMimoV25Pro,
    /// MiniMax M3 - Frontier multimodal coding model on OpenCode Go
    OpenCodeGoMinimaxM3,
    /// MiniMax M2.7 - Higher-tier OpenCode Go subscription model
    OpenCodeGoMinimaxM27,
    /// Qwen3.7 Max - Qwen flagship on OpenCode Go
    OpenCodeGoQwen37Max,
    /// Qwen3.7 Plus - Qwen balanced tier on OpenCode Go
    OpenCodeGoQwen37Plus,
    /// Qwen3.6 Plus - Qwen 3.6 tier on OpenCode Go
    OpenCodeGoQwen36Plus,
    /// DeepSeek V4 Pro - High-performance reasoning model on OpenCode Go
    OpenCodeGoDeepseekV4Pro,
    /// DeepSeek V4 Flash - Fast inference model on OpenCode Go
    OpenCodeGoDeepseekV4Flash,

    // Qwen models (non-Qwen3 only)
    /// DeepSeek V4 Flash via Qwen Cloud API
    QwenDeepSeekV4Flash,
    /// DeepSeek V4 Pro via Qwen Cloud API
    QwenDeepSeekV4Pro,
    /// GLM-5.1 via Qwen Cloud API
    QwenGlm51,

    // Ollama models
    /// GPT-OSS 20B - Open-weight GPT-OSS 20B model served via Ollama locally
    OllamaGptOss20b,
    /// GPT-OSS 20B Cloud - Cloud-hosted GPT-OSS 20B served via Ollama Cloud
    OllamaGptOss20bCloud,
    /// GPT-OSS 120B Cloud - Cloud-hosted GPT-OSS 120B served via Ollama Cloud
    OllamaGptOss120bCloud,
    /// DeepSeek V4 Flash Cloud - Fast inference DeepSeek V4 Flash model via Ollama Cloud
    OllamaDeepseekV4FlashCloud,
    /// DeepSeek V4 Pro Cloud - High-performance DeepSeek V4 Pro model via Ollama Cloud
    OllamaDeepseekV4ProCloud,
    /// MiniMax-M2.7 Cloud - Cloud-hosted MiniMax-M2.7 model served via Ollama Cloud
    OllamaMinimaxM27Cloud,
    /// MiniMax-M3 Cloud - Cloud-hosted MiniMax-M3 model served via Ollama Cloud
    OllamaMinimaxM3Cloud,
    /// GLM-5.1 Cloud - Cloud-hosted GLM-5.1 model served via Ollama Cloud
    OllamaGlm51Cloud,
    /// GLM-5.2 Cloud - Cloud-hosted GLM-5.2 flagship model served via Ollama Cloud
    OllamaGlm52Cloud,
    /// Kimi K2.6 Cloud - Moonshot Kimi K2.6 via Ollama Cloud
    OllamaKimiK26Cloud,
    /// Kimi K2.7 Code Cloud - Moonshot Kimi K2.7 Code via Ollama Cloud
    OllamaKimiK27CodeCloud,
    /// Gemma 4 - Google Gemma 4 model served via Ollama
    OllamaGemma4,
    /// Laguna XS.2 - Poolside's 33B MoE model (3B activated) for agentic coding via Ollama
    OllamaLagunaXs2,

    // llama.cpp models
    /// Gemma 4 26B A4B - Desktop Gemma 4 MoE model served through llama.cpp
    LlamaCppGemma426bA4b,
    /// Gemma 4 E4B - Tiny-footprint Gemma 4 model served through llama.cpp
    LlamaCppGemma4E4b,
    /// GPT-OSS 20B - OpenAI open-weight model served through llama.cpp
    LlamaCppGptOss20b,
    /// Step 3.5 Flash - StepFun local model served through llama.cpp
    LlamaCppStep35Flash,

    // MiniMax models
    /// MiniMax-M3 - Frontier multimodal coding model with 1M context
    MinimaxM3,
    /// MiniMax-M2.7 - Recursive self-improvement flagship with 204.8K context
    MinimaxM27,

    // OpenRouter models
    /// DeepSeek V4 Pro - High-performance reasoning model via OpenRouter
    OpenRouterDeepSeekV4Pro,
    /// DeepSeek V4 Flash - Fast inference model via OpenRouter
    OpenRouterDeepSeekV4Flash,
    /// DeepSeek R1 - DeepSeek R1 reasoning model with chain-of-thought
    OpenRouterDeepSeekR1,
    /// OpenAI gpt-oss-120b - Open-weight 120B reasoning model via OpenRouter
    OpenRouterOpenAIGptOss120b,
    /// OpenAI gpt-oss-120b:free - Open-weight 120B reasoning model free tier via OpenRouter
    OpenRouterOpenAIGptOss120bFree,
    /// OpenAI gpt-oss-20b - Open-weight 20B deployment via OpenRouter
    OpenRouterOpenAIGptOss20b,
    /// OpenAI GPT-5 - OpenAI GPT-5 model accessed through OpenRouter
    OpenRouterOpenAIGpt5,
    /// OpenAI GPT-5.5 - OpenAI GPT-5.5 model accessed through OpenRouter
    OpenRouterOpenAIGpt55,
    /// OpenAI GPT-5 Chat - Chat optimised GPT-5 endpoint without tool use
    OpenRouterOpenAIGpt5Chat,

    /// Gemini 3.1 Pro Preview - Google's latest Gemini 3.1 Pro model via OpenRouter
    OpenRouterGoogleGemini31ProPreview,

    /// Claude Sonnet 4.6 - Anthropic Claude Sonnet 4.6 listing
    OpenRouterAnthropicClaudeSonnet46,
    /// Claude Sonnet 5 - Anthropic Claude Sonnet 5 listing
    OpenRouterAnthropicClaudeSonnet5,
    /// Claude Haiku 4.5 - Anthropic Claude Haiku 4.5 listing
    OpenRouterAnthropicClaudeHaiku45,
    /// Mistral Large 3 2512 - Mistral Large 3 2512 model via OpenRouter
    OpenRouterMistralaiMistralLarge2512,
    /// DeepSeek V3.1 Nex N1 - Nex AGI DeepSeek V3.1 Nex N1 model via OpenRouter
    OpenRouterNexAgiDeepseekV31NexN1,
    /// Step 3.5 Flash (free) - StepFun's most capable open-source reasoning model via OpenRouter
    OpenRouterStepfunStep35FlashFree,
    /// GLM-5.1 - Z.AI GLM-5.1 next-gen foundation model via OpenRouter
    OpenRouterZaiGlm51,
    /// GLM-5.2 - Z.AI GLM-5.2 flagship model for long-horizon tasks via OpenRouter
    OpenRouterZaiGlm52,
    /// Kimi K3 - Moonshot AI's 2.8T parameter flagship via OpenRouter
    OpenRouterMoonshotaiKimiK3,
    /// Kimi K2.6 - Moonshot AI's next-generation multimodal model via OpenRouter
    OpenRouterMoonshotaiKimiK26,
    /// Kimi K2.7 Code - Moonshot AI's most capable coding model via OpenRouter
    OpenRouterMoonshotaiKimiK27Code,
    /// Hy3 Preview - Tencent's high-efficiency MoE model for agentic workflows via OpenRouter
    OpenRouterTencentHy3Preview,
    /// Grok Build 0.1 - xAI's fast coding model for agentic software engineering via OpenRouter
    OpenRouterXAiGrokBuild01,
    /// MiMo-V2.5 - Xiaomi's omnimodal agentic model for complex software engineering via OpenRouter
    OpenRouterXiaomiMimoV25,
    /// MiMo-V2.5-Pro - Xiaomi's flagship agentic model for complex software engineering via OpenRouter
    OpenRouterXiaomiMimoV25Pro,
    /// Laguna XS.2 (free) - Poolside's efficient free coding agent model via OpenRouter
    OpenRouterPoolsideLagunaXs2Free,
    /// Laguna M.1 (free) - Poolside's flagship free coding agent model via OpenRouter
    OpenRouterPoolsideLagunaM1Free,

    // Poolside models
    /// Laguna M.1 - Poolside's flagship MoE coding agent model
    PoolsideLagunaM1,
    /// Laguna XS.2 - Poolside's efficient MoE coding agent model
    PoolsideLagunaXs2,

    /// User-defined model not in the hardcoded catalog.
    /// Carries the provider key string and model identifier string.
    Custom(String, String),
}
