use serde::{Deserialize, Serialize};

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
    /// Gemini 3 Pro Preview - Preview of next-generation Gemini model
    Gemini3ProPreview,
    /// Gemini 3 Flash Preview - Our most intelligent model built for speed, combining frontier intelligence with superior search and grounding
    Gemini3FlashPreview,

    // OpenAI models
    /// GPT-5 - Latest most capable OpenAI model (2025-08-07)
    GPT5,
    /// GPT-5.2 - Latest flagship general-purpose OpenAI model (2025-12-11)
    GPT52,
    /// GPT-5.2 Codex - Code-focused GPT-5.2 variant optimized for agentic coding
    GPT52Codex,
    /// GPT-5 Codex - Code-focused GPT-5 variant using the Responses API
    GPT5Codex,
    /// GPT-5 Mini - Latest efficient OpenAI model (2025-08-07)
    GPT5Mini,
    /// GPT-5 Nano - Latest most cost-effective OpenAI model (2025-08-07)
    GPT5Nano,
    /// GPT-5.1 - Enhanced latest most capable OpenAI model with improved reasoning (2025-11-14)
    GPT51,
    /// GPT-5.1 Codex - Code-focused GPT-5.1 variant using the Responses API
    GPT51Codex,
    /// GPT-5.1 Codex Max - Maximum context code-focused GPT-5.1 variant
    GPT51CodexMax,
    /// GPT-5.1 Mini - Enhanced efficient OpenAI model with improved capabilities (2025-11-14)
    GPT51Mini,
    /// Codex Mini Latest - Latest Codex model for code generation (2025-05-16)
    CodexMiniLatest,
    /// GPT-OSS 20B - OpenAI's open-source 20B parameter model using harmony
    OpenAIGptOss20b,
    /// GPT-OSS 120B - OpenAI's open-source 120B parameter model using harmony
    OpenAIGptOss120b,

    // Anthropic models
    /// Claude Opus 4.1 - Specialized reasoning model (2025-08-05)
    ClaudeOpus41,
    /// Claude Opus 4.5 - Flagship model with exceptional intelligence (2025-11-01)
    ClaudeOpus45,
    /// Claude Sonnet 4.5 - Balanced flagship model for coding (2025-11-15)
    ClaudeSonnet45,
    /// Claude Haiku 4.5 - Fastest model with near-frontier intelligence (2025-11-15)
    ClaudeHaiku45,
    /// Claude Opus 4 - Previous flagship model (2025-05-14)
    ClaudeOpus4,
    /// Claude Sonnet 4 - Previous balanced model (2025-05-14)
    ClaudeSonnet4,
    /// Claude Sonnet 3.7 - Latest Claude 3 Sonnet (2025-02-19)
    ClaudeSonnet37,
    /// Claude Haiku 3.5 - Latest Claude 3 Haiku (2024-10-22)
    ClaudeHaiku35,

    // DeepSeek models
    /// DeepSeek V3.2 Chat - Fast non-thinking mode
    DeepSeekChat,
    /// DeepSeek V3.2 Reasoner - Thinking mode with structured reasoning output
    DeepSeekReasoner,

    // Hugging Face Inference Providers
    /// DeepSeek V3.2 via Hugging Face router
    HuggingFaceDeepseekV32,
    /// OpenAI GPT-OSS 20B via Hugging Face router
    HuggingFaceOpenAIGptOss20b,
    /// OpenAI GPT-OSS 120B via Hugging Face router
    HuggingFaceOpenAIGptOss120b,
    /// Z.AI GLM-4.7 via Hugging Face router
    HuggingFaceGlm47,
    /// Z.AI GLM-4.7 via Novita on Hugging Face router
    HuggingFaceGlm47Novita,
    /// Z.AI GLM-4.7-Flash via Novita on Hugging Face router
    HuggingFaceGlm47FlashNovita,
    /// MoonshotAI Kimi K2 Thinking via Hugging Face router
    HuggingFaceKimiK2Thinking,
    /// MiniMax M2.1 via Novita on Hugging Face router - Enhanced reasoning
    HuggingFaceMinimaxM21Novita,
    /// DeepSeek V3.2 via Novita on Hugging Face router
    HuggingFaceDeepseekV32Novita,
    /// Xiaomi MiMo-V2-Flash via Novita on Hugging Face router
    HuggingFaceXiaomiMimoV2FlashNovita,

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
    /// GLM-4.7 - Latest flagship GLM reasoning model
    ZaiGlm47,
    /// GLM-4.7 (Deep Thinking) - Latest flagship GLM reasoning model with forced deep thinking
    ZaiGlm47DeepThinking,
    /// GLM-4.7 Flash - Lightweight GLM-4.7 model optimized for agentic coding
    ZaiGlm47Flash,
    /// GLM-4.6 - Latest flagship GLM reasoning model
    ZaiGlm46,
    /// GLM-4.6 (Deep Thinking) - Latest flagship GLM reasoning model with forced deep thinking
    ZaiGlm46DeepThinking,
    /// GLM-4.5 - Balanced GLM release for general tasks
    ZaiGlm45,
    /// GLM-4.5 (Deep Thinking) - Balanced GLM reasoning model with forced deep thinking
    ZaiGlm45DeepThinking,
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

    // Ollama models
    /// GPT-OSS 20B - Open-weight GPT-OSS 20B model served via Ollama locally
    OllamaGptOss20b,
    /// GPT-OSS 20B Cloud - Cloud-hosted GPT-OSS 20B served via Ollama Cloud
    OllamaGptOss20bCloud,
    /// GPT-OSS 120B Cloud - Cloud-hosted GPT-OSS 120B served via Ollama Cloud
    OllamaGptOss120bCloud,
    /// Qwen3 1.7B - Qwen3 1.7B model served via Ollama
    OllamaQwen317b,
    /// DeepSeek V3.2 Cloud - DeepSeek V3.2 reasoning deployment via Ollama Cloud
    OllamaDeepseekV32Cloud,
    /// Qwen3 Next 80B Cloud - Next-generation Qwen3 80B via Ollama Cloud
    OllamaQwen3Next80bCloud,
    /// Mistral Large 3 675B Cloud - Mistral Large 3 reasoning model via Ollama Cloud
    OllamaMistralLarge3675bCloud,
    /// Kimi K2 Thinking Cloud - MoonshotAI Kimi K2 thinking model via Ollama Cloud
    OllamaKimiK2ThinkingCloud,

    /// Qwen3 Coder 480B Cloud - Large Qwen3 coding specialist via Ollama Cloud
    OllamaQwen3Coder480bCloud,
    /// GLM 4.6 Cloud - GLM 4.6 reasoning model via Ollama Cloud
    OllamaGlm46Cloud,
    /// Gemini 3 Pro Preview Latest Cloud - Google Gemini 3 Pro Preview via Ollama Cloud
    OllamaGemini3ProPreviewLatestCloud,
    /// Devstral 2 123B Cloud - Mistral Devstral 2 123B model via Ollama Cloud
    OllamaDevstral2123bCloud,
    /// MiniMax-M2 Cloud - MiniMax reasoning model via Ollama Cloud
    OllamaMinimaxM2Cloud,
    /// GLM-4.7 Cloud - Cloud-hosted GLM-4.7 model served via Ollama Cloud
    OllamaGlm47Cloud,
    /// MiniMax-M2.1 Cloud - Cloud-hosted MiniMax-M2.1 model served via Ollama Cloud
    OllamaMinimaxM21Cloud,
    /// Gemini 3 Flash Preview Cloud - Google Gemini 3 Flash Preview via Ollama Cloud
    OllamaGemini3FlashPreviewCloud,
    /// Nemotron-3-Nano 30B Cloud - NVIDIA Nemotron-3-Nano 30B via Ollama Cloud
    OllamaNemotron3Nano30bCloud,

    // LM Studio models
    /// Meta Llama 3 8B Instruct served locally via LM Studio
    LmStudioMetaLlama38BInstruct,
    /// Meta Llama 3.1 8B Instruct served locally via LM Studio
    LmStudioMetaLlama318BInstruct,
    /// Qwen2.5 7B Instruct served locally via LM Studio
    LmStudioQwen257BInstruct,
    /// Gemma 2 2B IT served locally via LM Studio
    LmStudioGemma22BIt,
    /// Gemma 2 9B IT served locally via LM Studio
    LmStudioGemma29BIt,
    /// Phi-3.1 Mini 4K Instruct served locally via LM Studio
    LmStudioPhi31Mini4kInstruct,

    // MiniMax models
    /// MiniMax-M2.1 - Latest MiniMax model with enhanced code understanding and reasoning
    MinimaxM21,
    /// MiniMax-M2.1-lightning - Fast version of MiniMax-M2.1
    MinimaxM21Lightning,
    /// MiniMax-M2 - MiniMax reasoning-focused model via Anthropic-compatible API
    MinimaxM2,

    // OpenRouter models
    /// Grok Code Fast 1 - Fast OpenRouter coding model powered by xAI Grok
    OpenRouterGrokCodeFast1,
    /// Grok 4 Fast - Reasoning-focused Grok endpoint with transparent traces
    OpenRouterGrok4Fast,
    /// Grok 4.1 Fast - Enhanced Grok 4.1 fast inference with improved reasoning
    OpenRouterGrok41Fast,
    /// Grok 4 - Flagship Grok 4 endpoint exposed through OpenRouter
    OpenRouterGrok4,
    /// GLM 4.6 - Z.AI GLM 4.6 long-context reasoning model
    OpenRouterZaiGlm46,
    /// Kimi K2 0905 - MoonshotAI Kimi K2 0905 MoE release optimised for coding agents
    OpenRouterMoonshotaiKimiK20905,
    /// Kimi K2 Thinking - MoonshotAI reasoning-tier Kimi K2 release optimized for long-horizon agents
    OpenRouterMoonshotaiKimiK2Thinking,
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
    /// Qwen3 Coder - Qwen3-based coding model tuned for IDE workflows
    OpenRouterQwen3Coder,
    /// Qwen3 Coder Plus - Premium Qwen3 coding model with long context
    OpenRouterQwen3CoderPlus,
    /// Qwen3 Coder Flash - Latency optimised Qwen3 coding model
    OpenRouterQwen3CoderFlash,
    /// Qwen3 Coder 30B A3B Instruct - Large Mixture-of-Experts coding deployment
    OpenRouterQwen3Coder30bA3bInstruct,
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
    /// OpenAI gpt-oss-20b - Open-weight 20B deployment via OpenRouter
    OpenRouterOpenAIGptOss20b,
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
    /// Amazon Nova 2 Lite - Amazon Nova 2 Lite model via OpenRouter
    OpenRouterAmazonNova2LiteV1,
    /// Mistral Large 3 2512 - Mistral Large 3 2512 model via OpenRouter
    OpenRouterMistralaiMistralLarge2512,
    /// DeepSeek V3.1 Nex N1 - Nex AGI DeepSeek V3.1 Nex N1 model via OpenRouter
    OpenRouterNexAgiDeepseekV31NexN1,
    /// OpenAI GPT-5.1 - OpenAI GPT-5.1 model accessed through OpenRouter
    OpenRouterOpenAIGpt51,
    /// OpenAI GPT-5.1-Codex - OpenRouter listing for GPT-5.1 Codex
    OpenRouterOpenAIGpt51Codex,
    /// OpenAI GPT-5.1-Codex-Max - OpenRouter listing for GPT-5.1 Codex Max
    OpenRouterOpenAIGpt51CodexMax,
    /// OpenAI GPT-5.1-Codex-Mini - OpenRouter listing for GPT-5.1 Codex Mini
    OpenRouterOpenAIGpt51CodexMini,
    /// OpenAI GPT-5.1 Chat - Chat optimised GPT-5.1 endpoint without tool use
    OpenRouterOpenAIGpt51Chat,
    /// OpenAI GPT-5.2 - OpenAI GPT-5.2 model accessed through OpenRouter
    OpenRouterOpenAIGpt52,
    /// OpenAI GPT-5.2 Chat - Chat optimised GPT-5.2 endpoint without tool use
    OpenRouterOpenAIGpt52Chat,
    /// OpenAI GPT-5.2-Codex - OpenRouter listing for GPT-5.2 Codex
    OpenRouterOpenAIGpt52Codex,
    /// OpenAI GPT-5.2 Pro - Professional tier GPT-5.2 model accessed through OpenRouter
    OpenRouterOpenAIGpt52Pro,
    /// OpenAI o1-pro - OpenAI o1-pro advanced reasoning model via OpenRouter
    OpenRouterOpenAIO1Pro,
    /// GLM 4.6V - Z.AI GLM 4.6V enhanced vision model
    OpenRouterZaiGlm46V,
    /// GLM 4.7 - Z.AI GLM 4.7 next-generation reasoning model
    OpenRouterZaiGlm47,
    /// GLM 4.7 Flash - Z.AI GLM-4.7-Flash lightweight model via OpenRouter
    OpenRouterZaiGlm47Flash,
}
