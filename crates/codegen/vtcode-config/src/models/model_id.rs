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
    ModelCatalogEntry, ModelPricing, catalog_provider_keys, model_catalog_entry, supported_models_for_provider,
};

/// Centralized enum for all supported model identifiers
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelId {
    // Gemini models
    /// Gemini 3.6 Flash - Latest flash model with improved capabilities
    /// Gemini 3.7 Flash - Flash model with 1M context and tunable thinking levels
    /// Gemini 3.8 Flash - Most intelligent Flash for long-horizon SWE, agents, and enterprise workflows (1M context, 64k output, low/medium/high thinking)
    Gemini38Flash,

    // OpenAI models
    /// GPT-6 Astra - Most capable model for hardest end-to-end work with complex reasoning, coding, computer use, and research
    GPT6Astra,
    /// GPT-6 Sol - Cost-efficient high-end model in the GPT-6 series for demanding professional work
    GPT6Sol,
    /// GPT-6.1 Sol - Near-Astra performance for complex coding, computer use, and professional work at a lower cost
    GPT61Sol,
    /// GPT-6 Luna - Fast cost-efficient model in the GPT-6 series for high-volume latency-sensitive workloads
    GPT6Luna,
    /// GPT-5.6 Sol - Frontier model for complex professional work in the GPT-5.6 family
    GPT56Sol,
    /// GPT-5.6 Terra - GPT-5.6 model that balances intelligence and cost
    GPT56Terra,
    /// GPT-5.6 Luna - GPT-5.6 model optimized for cost-sensitive workloads
    GPT56Luna,
    /// GPT-OSS 20B - OpenAI's open-source 20B parameter model using harmony
    OpenAIGptOss20b,
    /// GPT-OSS 120B - OpenAI's open-source 120B parameter model using harmony
    OpenAIGptOss120b,

    // Anthropic models
    /// Claude Sonnet 5 - The best combination of speed and intelligence with adaptive thinking on by default
    #[default]
    ClaudeSonnet5,
    /// Claude Sonnet 5.5 - Latest Sonnet: best speed/intelligence balance, 1M context, 128K output, `between_tools` as the lowest thinking setting, default effort high
    ClaudeSonnet55,
    /// Claude Haiku 5.5 - High-volume latency-sensitive work: classification, routing, extraction, subagents; adaptive thinking, 1M context, 128K output, default effort medium
    ClaudeHaiku55,
    /// Claude Fable 5 - Anthropic's most capable widely released model for demanding reasoning and long-horizon agentic work
    ClaudeFable5,
    /// Claude Fable 5.1 - successor to Fable 5 for demanding reasoning and long-horizon agentic work, 1M context, adaptive thinking always on, cache reads at 1/4 cost
    ClaudeFable51,
    /// Claude Opus 5 - Anthropic's newest Opus-tier model with 1M context, thinking on by default
    ClaudeOpus5,
    /// Claude Opus 5.5 - Opus-tier successor for long-running agentic coding, adaptive thinking always on, 1M context, 128K output, default effort medium
    ClaudeOpus55,
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

    // DeepSeek models
    /// DeepSeek V4.1 Flash - Latest flash model with improved reasoning and efficiency
    DeepSeekFlash,

    // Official Meta AI models
    /// Meta Muse Spark 1.1 - Official Meta AI Standard-tier reasoning model
    MetaMuseSpark11,
    /// Meta Muse Spark 1.3 - Official Meta AI flagship Standard-tier reasoning model, tuned for agentic workflows
    MetaMuseSpark13,
    /// Meta Muse Spark 1.3 Contributor tier - opt-in variant with Meta's discounted data-contribution terms
    MetaMuseSpark13Contributor,

    // NVIDIA NIM models
    /// NVIDIA Nemotron 3 Ultra - NVIDIA's flagship agentic reasoning model via NIM
    NvidiaNemotron3Ultra550bA55b,
    /// NVIDIA Nemotron 3 Super - Efficient long-context agentic reasoning model via NIM
    NvidiaNemotron3Super120bA12b,
    /// NVIDIA Nemotron 3 Nano - Efficient reasoning and tool-use model via NIM
    NvidiaNemotron3Nano30bA3b,

    // Merge Gateway routes
    /// Merge Gateway's default route selected by Merge
    MergeGatewayDefaultRouting,
    /// Anthropic Claude Opus 5 through Merge Gateway
    MergeGatewayAnthropicClaudeOpus5,
    /// Anthropic Claude Opus 5.5 through Merge Gateway
    MergeGatewayAnthropicClaudeOpus55,
    /// Anthropic Claude Sonnet 5 through Merge Gateway
    MergeGatewayAnthropicClaudeSonnet5,
    /// Anthropic Claude Sonnet 5.5 through Merge Gateway
    MergeGatewayAnthropicClaudeSonnet55,
    /// Google Gemini 3.6 Flash through Merge Gateway
    /// Google Gemini 3.7 Flash through Merge Gateway
    /// DeepSeek V4.1 Flash through Merge Gateway
    MergeGatewayDeepseekFlash,
    /// xAI Grok 4.6 through Merge Gateway
    MergeGatewayXaiGrok46,
    /// xAI Grok 4.7 through Merge Gateway
    MergeGatewayXaiGrok47,
    /// MiniMax H3 through Merge Gateway
    MergeGatewayMinimaxH3,
    /// Moonshot Kimi K3 through Merge Gateway
    MergeGatewayMoonshotKimiK3,
    /// Thinking Machines Inkling through Merge Gateway
    MergeGatewayThinkingMachinesInkling,
    /// Z.AI GLM-5.3 Flash through Merge Gateway
    MergeGatewayZaiGlm53Flash,
    /// Z.AI GLM-5.3 FlashX through Merge Gateway
    MergeGatewayZaiGlm53Flashx,
    /// OpenAI GPT-5.6 Luna through Merge Gateway
    MergeGatewayOpenAIGpt56Luna,
    /// OpenAI GPT-5.6 Sol through Merge Gateway
    MergeGatewayOpenAIGpt56Sol,
    /// OpenAI GPT-5.6 Terra through Merge Gateway
    MergeGatewayOpenAIGpt56Terra,
    /// Google Gemini 3.8 Flash through Merge Gateway
    MergeGatewayGoogleGemini38Flash,
    /// Anthropic Claude Haiku 4.5 through Merge Gateway
    MergeGatewayAnthropicClaudeHaiku4520251001,
    /// Anthropic Claude Haiku 5.5 through Merge Gateway
    MergeGatewayAnthropicClaudeHaiku55,
    /// Anthropic Claude Fable 5.1 through Merge Gateway
    MergeGatewayAnthropicClaudeFable51,
    /// OpenAI GPT-6 Astra through Merge Gateway
    MergeGatewayOpenAIGpt6Astra,
    /// OpenAI GPT-6 Sol through Merge Gateway
    MergeGatewayOpenAIGpt6Sol,
    /// OpenAI GPT-6.1 Sol through Merge Gateway
    MergeGatewayOpenAIGpt61Sol,
    /// OpenAI GPT-6 Luna through Merge Gateway
    MergeGatewayOpenAIGpt6Luna,
    /// Xiaomi MiMo V2.6 Pro through Merge Gateway
    MergeGatewayXiaomimimoMimoV26Pro,
    /// Xiaomi MiMo V2.6 Flash through Merge Gateway
    MergeGatewayXiaomimimoMimoV26Flash,
    /// Mistral Large 4 through Merge Gateway
    MergeGatewayMistralLarge4,

    // Mistral AI models
    /// Mistral Large 3 - State-of-the-art open-weight general-purpose multimodal model
    MistralLarge3,
    /// Mistral Large 4 - Open-weight MoE flagship (49B active / 1.05T total) with 1M context (Public Preview)
    MistralLarge4,
    // Hugging Face models
    /// OpenAI GPT-OSS 20B via Hugging Face router
    HuggingFaceOpenAIGptOss20b,
    /// OpenAI GPT-OSS 120B via Hugging Face router
    HuggingFaceOpenAIGptOss120b,
    /// Z.AI GLM-5.2 via Novita inference provider on Hugging Face router
    /// Z.AI GLM-5.3 Flash via Together inference provider on Hugging Face router
    HuggingFaceGlm53FlashTogether,
    /// Z.AI GLM-5.3 via Together inference provider on Hugging Face router
    HuggingFaceGlm53Together,
    /// Kimi K3 via Together on Hugging Face router
    HuggingFaceKimiK3Together,
    /// MiniMax M3 via Novita on Hugging Face router
    HuggingFaceMinimaxM3Novita,

    // StepFun models
    /// Step 3.7 Flash - StepFun's flagship multimodal reasoning model with tool calling
    StepFun37Flash,
    /// Step 5 Preview - StepFun's frontier model for production-scale Agent applications with 1M context
    StepFun5Preview,

    // Evolink gateway models (namespaced as `evolink/<model>`)
    /// GPT-5.2 served through the Evolink gateway
    /// GPT-5.5 served through the Evolink gateway
    /// Gemini 3.1 Pro served through the Evolink gateway (OpenAI SDK format)
    EvolinkGemini31Pro,
    /// Gemini 3.5 Flash served through the Evolink gateway (OpenAI SDK format)
    /// MiniMax-M3 served through the Evolink gateway (OpenAI Chat Completions format)
    EvolinkMinimaxM3,
    /// Claude Haiku 4.5 served through the Evolink gateway (Anthropic Messages API)
    EvolinkClaudeHaiku45,

    /// GLM-5.3 - Z.ai flagship coding model with frontier long-horizon agentic performance
    ZaiGlm53,
    /// GLM-5.3 Flash - Z.ai efficient multimodal model with hybrid sparse+linear attention, 320B total / 18B active, 1M context, native vision
    ZaiGlm53Flash,
    /// GLM-5.3 FlashX - Z.ai high-speed Flash variant with faster inference (up to 200 tok/s), 320B total / 18B active, 1M context, native vision
    ZaiGlm53Flashx,

    // MiMo models
    /// MiMo V2.6 Pro - Xiaomi's flagship reasoning model with 1M context
    MiMoV26Pro,
    /// MiMo V2.6 Flash - Xiaomi's efficient high-volume model with 1M context
    MiMoV26Flash,
    /// MiMo V2.6 Pro UltraSpeed - Xiaomi's fastest flagship variant with 1M context
    MiMoV26ProUltraspeed,

    // Moonshot models
    /// Kimi K3 - Moonshot.ai's 2.8T parameter flagship with Delta Attention, native vision, 1M context
    MoonshotKimiK3,

    // OpenCode Zen models

    // OpenCode Go models (20 models - https://opencode.ai/docs/go)
    /// GLM-5.3 - Z.AI flagship for frontier long-horizon coding on OpenCode Go
    OpenCodeGoGlm53,
    /// GLM-5.2 - Z.AI flagship model included with OpenCode Go
    /// GPT-5.6 Luna - OpenAI cost-efficient frontier model on OpenCode Go
    OpenCodeGoGpt56Luna,
    /// Kimi K3 - Moonshot flagship 2.8T agentic model on OpenCode Go
    OpenCodeGoKimiK3,
    /// MiniMax M3 - Frontier multimodal coding model on OpenCode Go
    OpenCodeGoMinimaxM3,
    /// Muse Spark 1.2 Contributor - Meta long-context reasoning on OpenCode Go (limited regions)

    // Qwen models (non-Qwen3 only)

    // Ollama models
    /// GPT-OSS 20B - Open-weight GPT-OSS 20B model served via Ollama locally
    OllamaGptOss20b,
    /// GPT-OSS 20B Cloud - Cloud-hosted GPT-OSS 20B served via Ollama Cloud
    OllamaGptOss20bCloud,
    /// GPT-OSS 120B Cloud - Cloud-hosted GPT-OSS 120B served via Ollama Cloud
    OllamaGptOss120bCloud,
    /// MiniMax-M3 Cloud - Cloud-hosted MiniMax-M3 model served via Ollama Cloud
    OllamaMinimaxM3Cloud,
    /// GLM-5.2 Cloud - Cloud-hosted GLM-5.2 flagship model served via Ollama Cloud
    /// GLM-5.3 Cloud - Cloud-hosted GLM-5.3 flagship model served via Ollama Cloud
    OllamaGlm53Cloud,
    /// Kimi K3 Cloud - Moonshot Kimi K3 via Ollama Cloud
    OllamaKimiK3Cloud,
    /// Gemma 4 - Google Gemma 4 model served via Ollama
    OllamaGemma4,
    /// Laguna XS.2 - Poolside's 33B MoE model (3B activated) for agentic coding via Ollama

    // llama.cpp models
    /// Gemma 4 26B A4B - Desktop Gemma 4 MoE model served through llama.cpp
    LlamaCppGemma426bA4b,
    /// Gemma 4 E4B - Tiny-footprint Gemma 4 model served through llama.cpp
    LlamaCppGemma4E4b,
    /// GPT-OSS 20B - OpenAI open-weight model served through llama.cpp
    LlamaCppGptOss20b,

    // MiniMax models
    /// MiniMax-M3 - Frontier multimodal coding model with 1M context
    MinimaxM3,

    // OpenRouter models
    /// DeepSeek V4.1 Flash - Latest flash model via OpenRouter
    OpenRouterDeepSeekFlash,
    /// OpenAI gpt-oss-120b - Open-weight 120B reasoning model via OpenRouter
    OpenRouterOpenAIGptOss120b,
    /// OpenAI gpt-oss-120b:free - Open-weight 120B reasoning model free tier via OpenRouter
    OpenRouterOpenAIGptOss120bFree,
    /// OpenAI gpt-oss-20b - Open-weight 20B deployment via OpenRouter
    OpenRouterOpenAIGptOss20b,
    /// OpenAI GPT-6 Astra - OpenAI's flagship model for demanding end-to-end work via OpenRouter
    OpenRouterOpenAIGpt6Astra,
    /// GPT-6 Sol - Cost-efficient high-end model in the GPT-6 series via OpenRouter
    OpenRouterOpenAIGpt6Sol,
    /// GPT-6 Luna - Fast cost-efficient model in the GPT-6 series via OpenRouter
    OpenRouterOpenAIGpt6Luna,

    /// Meta Muse Glimmer 30B via OpenRouter
    OpenRouterMetaMuseGlimmer30b,
    /// Meta Muse Spark 1.2 via OpenRouter
    /// Meta Muse Spark 1.3 via OpenRouter
    OpenRouterMetaMuseSpark13,
    /// Gemini 3.7 Flash - Flash model with 1M context and tunable thinking levels via OpenRouter
    /// Gemini 3.8 Flash - Most intelligent Flash for long-horizon SWE/agents with 1M context via OpenRouter
    OpenRouterGoogleGemini38Flash,

    /// Claude Sonnet 5 - Anthropic Claude Sonnet 5 listing
    OpenRouterAnthropicClaudeSonnet5,
    /// Mistral Large 3 2512 - Mistral Large 3 2512 model via OpenRouter
    OpenRouterMistralaiMistralLarge2512,
    /// DeepSeek V3.1 Nex N1 - Nex AGI DeepSeek V3.1 Nex N1 model via OpenRouter
    OpenRouterNexAgiDeepseekV31NexN1,
    /// GLM-5.2 - Z.AI GLM-5.2 flagship model for long-horizon tasks via OpenRouter
    /// GLM-5.3 Flash - Z.AI efficient multimodal model with hybrid sparse+linear attention via OpenRouter
    OpenRouterZaiGlm53Flash,
    /// GLM-5.3 FlashX - Z.AI high-speed Flash variant with faster inference via OpenRouter
    OpenRouterZaiGlm53Flashx,
    /// Kimi K3 - Moonshot AI's 2.8T parameter flagship via OpenRouter
    OpenRouterMoonshotaiKimiK3,
    /// Grok 4.6 - xAI's flagship reasoning model with reasoning_effort support via OpenRouter
    OpenRouterXAiGrok46,
    /// Grok 4.7 - xAI's flagship reasoning model with reasoning_effort support via OpenRouter
    OpenRouterXAiGrok47,
    /// MiMo-V2.6-Pro - Xiaomi's flagship agentic model for complex software engineering via OpenRouter
    OpenRouterXiaomiMimoV26Pro,
    /// MiMo-V2.6-Flash - Xiaomi's efficient high-volume agentic model via OpenRouter
    OpenRouterXiaomiMimoV26Flash,
    /// MiMo-V2.6-Pro-UltraSpeed - Xiaomi's fastest flagship variant via OpenRouter
    OpenRouterXiaomiMimoV26ProUltraspeed,

    // Vercel AI Gateway models (namespaced as `vendor/model` on the gateway)
    /// Claude Sonnet 5 served through the Vercel AI Gateway
    VercelAnthropicClaudeSonnet5,
    /// Claude Opus 5 served through the Vercel AI Gateway
    VercelAnthropicClaudeOpus5,
    /// Claude Opus 5.5 served through the Vercel AI Gateway
    VercelAnthropicClaudeOpus55,
    /// Claude Haiku 4.5 served through the Vercel AI Gateway
    VercelAnthropicClaudeHaiku45,
    /// GPT-5.6 Sol served through the Vercel AI Gateway
    VercelOpenAiGpt56Sol,
    /// GPT-6 Astra served through the Vercel AI Gateway
    VercelOpenAiGpt6Astra,
    /// GPT-5.6 Luna served through the Vercel AI Gateway
    VercelOpenAiGpt56Luna,
    /// GPT-5.3 Codex served through the Vercel AI Gateway
    /// Gemini 3.1 Pro Preview served through the Vercel AI Gateway
    /// Gemini 3.8 Flash served through the Vercel AI Gateway
    VercelGoogleGemini38Flash,
    /// DeepSeek V4.1 Flash served through the Vercel AI Gateway
    VercelDeepseekFlash,
    /// Kimi K3 served through the Vercel AI Gateway
    VercelMoonshotaiKimiK3,
    /// MiniMax M3 served through the Vercel AI Gateway
    VercelMinimaxM3,
    /// Grok 4.7 served through the Vercel AI Gateway
    VercelSpacexaiGrok47,
    // xAI models
    /// Grok 4.6 - xAI's flagship reasoning model with reasoning_effort support (500k context)
    XaiGrok46,
    /// Grok 4.7 - xAI's flagship reasoning model with reasoning_effort support (500k context)
    XaiGrok47,

    /// User-defined model not in the hardcoded catalog.
    /// Carries the provider key string and model identifier string.
    Custom(String, String),
}
