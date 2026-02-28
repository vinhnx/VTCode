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

/// Centralized enum for all supported model identifiers
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelId {
    // Gemini models
    /// Gemini 3.1 Pro Preview - Latest Gemini 3.1 Pro flagship
    Gemini31ProPreview,
    /// Gemini 3.1 Pro Preview Custom Tools - Optimized for custom tools & bash
    Gemini31ProPreviewCustomTools,
    /// Gemini 3 Flash Preview - Our most intelligent model built for speed, combining frontier intelligence with superior search and grounding
    #[default]
    Gemini3FlashPreview,

    // OpenAI models
    /// GPT-5 - Latest most capable OpenAI model (2025-08-07)
    GPT5,
    /// GPT-5.2 - Latest flagship general-purpose OpenAI model (2025-12-11)
    GPT52,
    /// GPT-5 Mini - Latest efficient OpenAI model (2025-08-07)
    GPT5Mini,
    /// GPT-5 Nano - Latest most cost-effective OpenAI model (2025-08-07)
    GPT5Nano,
    /// GPT-5.3 Codex - Code-focused GPT-5.3 variant optimized for agentic coding with reasoning effort support (low, medium, high, xhigh)
    GPT53Codex,
    /// GPT-OSS 20B - OpenAI's open-source 20B parameter model using harmony
    OpenAIGptOss20b,
    /// GPT-OSS 120B - OpenAI's open-source 120B parameter model using harmony
    OpenAIGptOss120b,

    // Anthropic models
    /// Claude Opus 4.6 - Next-gen flagship Anthropic model with extended thinking
    ClaudeOpus46,
    /// Claude Sonnet 4.6 - Balanced flagship Anthropic model with extended thinking
    ClaudeSonnet46,
    /// Claude Opus 4.5 - Latest flagship Anthropic model with exceptional reasoning (2025-11-01)
    ClaudeOpus45,
    /// Claude Opus 4.1 - Previous most capable Anthropic model (2025-08-05)
    ClaudeOpus41,
    /// Claude Sonnet 4.5 - Latest balanced Anthropic model (2025-10-15)
    ClaudeSonnet45,
    /// Claude Haiku 4.5 - Latest efficient Anthropic model (2025-10-15)
    ClaudeHaiku45,
    /// Claude Sonnet 4 - Previous balanced Anthropic model (2025-05-14)
    ClaudeSonnet4,

    // DeepSeek models
    /// DeepSeek V3.2 Chat - Fast non-thinking mode
    DeepSeekChat,
    /// DeepSeek V3.2 Reasoner - Thinking mode with structured reasoning output
    DeepSeekReasoner,
    // Hugging Face models
    /// DeepSeek V3.2 via Hugging Face router
    HuggingFaceDeepseekV32,
    /// OpenAI GPT-OSS 20B via Hugging Face router
    HuggingFaceOpenAIGptOss20b,
    /// OpenAI GPT-OSS 120B via Hugging Face router
    HuggingFaceOpenAIGptOss120b,
    /// DeepSeek V3.2 via Novita on Hugging Face router
    HuggingFaceDeepseekV32Novita,
    /// Xiaomi MiMo-V2-Flash via Novita on Hugging Face router
    HuggingFaceXiaomiMimoV2FlashNovita,
    /// MiniMax M2.5 via Novita on Hugging Face router
    HuggingFaceMinimaxM25Novita,
    /// Z.AI GLM-5 via Novita on Hugging Face router
    HuggingFaceGlm5Novita,
    /// Qwen3-Coder-Next via Novita inference provider on Hugging Face router
    HuggingFaceQwen3CoderNextNovita,
    /// Qwen3.5-397B-A17B via Together inference provider on Hugging Face router
    HuggingFaceQwen35397BA17BTogether,

    /// GLM-5 - Flagship Z.ai foundation model for complex systems
    ZaiGlm5,

    // Moonshot models
    /// MiniMax-M2.5 - MiniMax model served via Moonshot API
    MoonshotMinimaxM25,
    /// Qwen3-Coder-Next - Qwen3 Coder Next model served via Moonshot API
    MoonshotQwen3CoderNext,

    // Ollama models
    /// GPT-OSS 20B - Open-weight GPT-OSS 20B model served via Ollama locally
    OllamaGptOss20b,
    /// GPT-OSS 20B Cloud - Cloud-hosted GPT-OSS 20B served via Ollama Cloud
    OllamaGptOss20bCloud,
    /// GPT-OSS 120B Cloud - Cloud-hosted GPT-OSS 120B served via Ollama Cloud
    OllamaGptOss120bCloud,
    /// Qwen3 1.7B - Qwen3 1.7B model served via Ollama
    OllamaQwen317b,
    /// Qwen3 Coder Next - Coding-optimized Qwen3 Next model served via Ollama locally
    OllamaQwen3CoderNext,
    /// DeepSeek V3.2 Cloud - DeepSeek V3.2 reasoning deployment via Ollama Cloud
    OllamaDeepseekV32Cloud,
    /// Qwen3 Next 80B Cloud - Next-generation Qwen3 80B via Ollama Cloud
    OllamaQwen3Next80bCloud,
    /// Mistral Large 3 675B Cloud - Mistral Large 3 reasoning model via Ollama Cloud
    OllamaMistralLarge3675bCloud,
    /// Qwen3 Coder 480B Cloud - Cloud-hosted Qwen3 Coder model served via Ollama Cloud
    OllamaQwen3Coder480bCloud,
    /// Devstral 2 123B Cloud - Mistral Devstral 2 123B model via Ollama Cloud
    OllamaDevstral2123bCloud,
    /// MiniMax-M2 Cloud - Cloud-hosted MiniMax-M2 model served via Ollama Cloud
    OllamaMinimaxM2Cloud,
    /// GLM-5 Cloud - Cloud-hosted GLM-5 model served via Ollama Cloud
    OllamaGlm5Cloud,
    /// MiniMax-M2.5 Cloud - Cloud-hosted MiniMax-M2.5 model served via Ollama Cloud
    OllamaMinimaxM25Cloud,
    /// Gemini 3 Flash Preview Cloud - Google Gemini 3 Flash Preview via Ollama Cloud
    OllamaGemini3FlashPreviewCloud,
    /// Nemotron-3-Nano 30B Cloud - NVIDIA Nemotron-3-Nano 30B via Ollama Cloud
    OllamaNemotron3Nano30bCloud,

    // MiniMax models
    /// MiniMax-M2.5 - Latest MiniMax model with further improvements in reasoning and coding
    MinimaxM25,
    /// MiniMax-M2 - MiniMax reasoning-focused model
    MinimaxM2,

    // OpenRouter models
    /// Qwen3 Max - Flagship Qwen3 mixture for general reasoning
    OpenRouterQwen3Max,
    /// Qwen3 235B A22B - Mixture-of-experts Qwen3 235B general model
    OpenRouterQwen3235bA22b,
    /// Qwen3 235B A22B Instruct 2507 - Instruction-tuned Qwen3 235B A22B
    OpenRouterQwen3235bA22b2507,
    /// Qwen3 235B A22B Thinking 2507 - Deliberative Qwen3 235B A22B reasoning release
    OpenRouterQwen3235bA22bThinking2507,
    /// Qwen3 32B - Dense 32B Qwen3 deployment
    OpenRouterQwen332b,
    /// Qwen3 30B A3B - Active-parameter 30B Qwen3 model
    OpenRouterQwen330bA3b,
    /// Qwen3 30B A3B Instruct 2507 - Instruction-tuned Qwen3 30B A3B
    OpenRouterQwen330bA3bInstruct2507,
    /// Qwen3 30B A3B Thinking 2507 - Deliberative Qwen3 30B A3B release
    OpenRouterQwen330bA3bThinking2507,
    /// Qwen3 14B - Lightweight Qwen3 14B model
    OpenRouterQwen314b,
    /// Qwen3 8B - Compact Qwen3 8B deployment
    OpenRouterQwen38b,
    /// Qwen3 Next 80B A3B Instruct - Next-generation Qwen3 instruction model
    OpenRouterQwen3Next80bA3bInstruct,
    /// Qwen3 Next 80B A3B Thinking - Next-generation Qwen3 reasoning release
    OpenRouterQwen3Next80bA3bThinking,
    /// Qwen3.5-397B-A17B - Native vision-language model with linear attention and sparse MoE, 1M context window
    OpenRouterQwen35Plus0215,
    /// Qwen3 Coder - Qwen3-based coding model tuned for IDE workflows
    OpenRouterQwen3Coder,
    /// Qwen3 Coder Plus - Premium Qwen3 coding model with long context
    OpenRouterQwen3CoderPlus,
    /// Qwen3 Coder Flash - Latency optimised Qwen3 coding model
    OpenRouterQwen3CoderFlash,
    /// Qwen3 Coder 30B A3B Instruct - Large Mixture-of-Experts coding deployment
    OpenRouterQwen3Coder30bA3bInstruct,
    /// Qwen3 Coder Next - Next-generation Qwen3 coding model with enhanced reasoning
    OpenRouterQwen3CoderNext,
    /// DeepSeek V3.2 Chat - Official chat model via OpenRouter
    OpenRouterDeepseekChat,
    /// DeepSeek V3.2 - Standard model with thinking support via OpenRouter
    OpenRouterDeepSeekV32,
    /// DeepSeek V3.2 Reasoner - Thinking mode via OpenRouter
    OpenRouterDeepseekReasoner,
    /// DeepSeek V3.2 Speciale - Enhanced reasoning model (no tool-use)
    OpenRouterDeepSeekV32Speciale,
    /// DeepSeek V3.2 Exp - Experimental DeepSeek V3.2 listing
    OpenRouterDeepSeekV32Exp,
    /// DeepSeek Chat v3.1 - Advanced DeepSeek model via OpenRouter
    OpenRouterDeepSeekChatV31,
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
    /// OpenAI GPT-5 Chat - Chat optimised GPT-5 endpoint without tool use
    OpenRouterOpenAIGpt5Chat,

    /// Gemini 3.1 Pro Preview - Google's latest Gemini 3.1 Pro model via OpenRouter
    OpenRouterGoogleGemini31ProPreview,

    /// Claude Sonnet 4.5 - Anthropic Claude Sonnet 4.5 listing
    OpenRouterAnthropicClaudeSonnet45,
    /// Claude Sonnet 4.6 - Anthropic Claude Sonnet 4.6 listing
    OpenRouterAnthropicClaudeSonnet46,
    /// Claude Haiku 4.5 - Anthropic Claude Haiku 4.5 listing
    OpenRouterAnthropicClaudeHaiku45,
    /// Claude Opus 4.1 - Anthropic Claude Opus 4.1 listing
    OpenRouterAnthropicClaudeOpus41,
    /// Amazon Nova 2 Lite - Amazon Nova 2 Lite model via OpenRouter
    OpenRouterAmazonNova2LiteV1,
    /// Mistral Large 3 2512 - Mistral Large 3 2512 model via OpenRouter
    OpenRouterMistralaiMistralLarge2512,
    /// DeepSeek V3.1 Nex N1 - Nex AGI DeepSeek V3.1 Nex N1 model via OpenRouter
    OpenRouterNexAgiDeepseekV31NexN1,
    /// OpenAI o1-pro - OpenAI o1-pro advanced reasoning model via OpenRouter
    OpenRouterOpenAIO1Pro,
    /// Step 3.5 Flash (free) - StepFun's most capable open-source reasoning model via OpenRouter
    OpenRouterStepfunStep35FlashFree,
    /// GLM-5 - Z.AI GLM-5 flagship foundation model via OpenRouter
    OpenRouterZaiGlm5,
    /// MiniMax-M2.5 - MiniMax flagship model via OpenRouter
    OpenRouterMinimaxM25,
}
