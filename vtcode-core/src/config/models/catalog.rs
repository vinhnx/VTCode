//! Model catalog helpers (display names, descriptions, listings).

use super::ModelId;

impl ModelId {
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
            ModelId::Gemini3ProPreview => "Gemini 3 Pro Preview",
            ModelId::Gemini3FlashPreview => "Gemini 3 Flash",
            // OpenAI models
            ModelId::GPT5 => "GPT-5",
            ModelId::GPT52 => "GPT-5.2",
            ModelId::GPT52Codex => "GPT-5.2 Codex",
            ModelId::GPT5Codex => "GPT-5 Codex",
            ModelId::GPT5Mini => "GPT-5 Mini",
            ModelId::GPT5Nano => "GPT-5 Nano",
            ModelId::GPT51 => "GPT-5.1",
            ModelId::GPT51Codex => "GPT-5.1 Codex",
            ModelId::GPT51CodexMax => "GPT-5.1 Codex Max",
            ModelId::GPT51Mini => "GPT-5.1 Mini",
            ModelId::CodexMiniLatest => "Codex Mini Latest",
            ModelId::OpenAIGptOss20b => "GPT-OSS 20B",
            ModelId::OpenAIGptOss120b => "GPT-OSS 120B",
            // Anthropic models
            ModelId::ClaudeOpus41 => "Claude Opus 4.1",
            ModelId::ClaudeOpus45 => "Claude Opus 4.5",
            ModelId::ClaudeSonnet45 => "Claude Sonnet 4.5",
            ModelId::ClaudeHaiku45 => "Claude Haiku 4.5",
            ModelId::ClaudeOpus4 => "Claude Opus 4",
            ModelId::ClaudeSonnet4 => "Claude Sonnet 4",
            ModelId::ClaudeSonnet37 => "Claude 3.7 Sonnet",
            ModelId::ClaudeHaiku35 => "Claude 3.5 Haiku",
            // DeepSeek models
            ModelId::DeepSeekChat => "DeepSeek V3.2 Chat",
            ModelId::DeepSeekReasoner => "DeepSeek V3.2 Reasoner",
            // Hugging Face Inference Providers
            ModelId::HuggingFaceDeepseekV32 => "DeepSeek V3.2 (HF)",
            ModelId::HuggingFaceOpenAIGptOss20b => "GPT-OSS 20B (HF)",
            ModelId::HuggingFaceOpenAIGptOss120b => "GPT-OSS 120B (HF)",
            ModelId::HuggingFaceGlm47 => "GLM-4.7 (HF)",
            ModelId::HuggingFaceGlm47Novita => "GLM-4.7 (Novita)",
            ModelId::HuggingFaceGlm47FlashNovita => "GLM-4.7-Flash (Novita)",
            ModelId::HuggingFaceKimiK2Thinking => "Kimi K2 Thinking (HF)",
            ModelId::HuggingFaceKimiK25Novita => "Kimi K2.5 (Novita)",
            ModelId::HuggingFaceMinimaxM21Novita => "MiniMax-M2.1 (Novita)",
            ModelId::HuggingFaceDeepseekV32Novita => "DeepSeek V3.2 (Novita)",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "MiMo-V2-Flash (Novita)",
            // xAI models
            ModelId::XaiGrok4 => "Grok-4",
            ModelId::XaiGrok4Mini => "Grok-4 Mini",
            ModelId::XaiGrok4Code => "Grok-4 Code",
            ModelId::XaiGrok4CodeLatest => "Grok-4 Code Latest",
            ModelId::XaiGrok4Vision => "Grok-4 Vision",
            // Z.AI models
            ModelId::ZaiGlm47 => "GLM 4.7",
            ModelId::ZaiGlm47DeepThinking => "GLM 4.7 (Deep Thinking)",
            ModelId::ZaiGlm47Flash => "GLM 4.7 Flash",
            ModelId::ZaiGlm46 => "GLM 4.6",
            ModelId::ZaiGlm46DeepThinking => "GLM 4.6 (Deep Thinking)",
            ModelId::ZaiGlm45 => "GLM 4.5",
            ModelId::ZaiGlm45DeepThinking => "GLM 4.5 (Deep Thinking)",
            ModelId::ZaiGlm45Air => "GLM 4.5 Air",
            ModelId::ZaiGlm45X => "GLM 4.5 X",
            ModelId::ZaiGlm45Airx => "GLM 4.5 AirX",
            ModelId::ZaiGlm45Flash => "GLM 4.5 Flash",
            ModelId::ZaiGlm432b0414128k => "GLM 4 32B 0414 128K",
            // Moonshot models
            ModelId::MoonshotKimiK25 => "Kimi K2.5",
            // Ollama models
            ModelId::OllamaGptOss20b => "GPT-OSS 20B (local)",
            ModelId::OllamaGptOss20bCloud => "GPT-OSS 20B (cloud)",
            ModelId::OllamaGptOss120bCloud => "GPT-OSS 120B (cloud)",
            ModelId::OllamaQwen317b => "Qwen3 1.7B (local)",
            ModelId::OllamaDeepseekV32Cloud => "DeepSeek V3.2 (cloud)",
            ModelId::OllamaQwen3Next80bCloud => "Qwen3 Next 80B (cloud)",
            ModelId::OllamaMistralLarge3675bCloud => "Mistral Large 3 675B (cloud)",
            ModelId::OllamaKimiK2ThinkingCloud => "Kimi K2 Thinking (cloud)",
            ModelId::OllamaKimiK25Cloud => "Kimi K2.5 (cloud)",

            ModelId::OllamaQwen3Coder480bCloud => "Qwen3 Coder 480B (cloud)",
            ModelId::OllamaGlm46Cloud => "GLM 4.6 (cloud)",
            ModelId::OllamaGemini3ProPreviewLatestCloud => "Gemini 3 Pro Preview (cloud)",
            ModelId::OllamaDevstral2123bCloud => "Devstral 2 123B (cloud)",
            ModelId::OllamaMinimaxM2Cloud => "MiniMax-M2 (cloud)",
            ModelId::OllamaNemotron3Nano30bCloud => "Nemotron-3-Nano 30B (cloud)",
            ModelId::OllamaGlm47Cloud => "GLM-4.7 (cloud)",
            ModelId::OllamaMinimaxM21Cloud => "MiniMax-M2.1 (cloud)",
            ModelId::OllamaGemini3FlashPreviewCloud => "Gemini 3 Flash Preview (cloud)",
            ModelId::LmStudioMetaLlama38BInstruct => "Meta Llama 3 8B (LM Studio)",
            ModelId::LmStudioMetaLlama318BInstruct => "Meta Llama 3.1 8B (LM Studio)",
            ModelId::LmStudioQwen257BInstruct => "Qwen2.5 7B (LM Studio)",
            ModelId::LmStudioGemma22BIt => "Gemma 2 2B (LM Studio)",
            ModelId::LmStudioGemma29BIt => "Gemma 2 9B (LM Studio)",
            ModelId::LmStudioPhi31Mini4kInstruct => "Phi-3.1 Mini 4K (LM Studio)",
            // MiniMax models
            ModelId::MinimaxM21 => "MiniMax-M2.1",
            ModelId::MinimaxM21Lightning => "MiniMax-M2.1-lightning",
            ModelId::MinimaxM2 => "MiniMax-M2",
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
            ModelId::Gemini3ProPreview => {
                "Preview of next-generation Gemini 3 Pro model with advanced reasoning and capabilities"
            }
            ModelId::Gemini3FlashPreview => {
                "Our most intelligent model built for speed, combining frontier intelligence with superior search and grounding"
            }
            // OpenAI models
            ModelId::GPT5 => "Latest most capable OpenAI model with advanced reasoning",
            ModelId::GPT52 => {
                "Latest flagship OpenAI model with improved reasoning, xhigh effort, and built-in compaction support"
            }
            ModelId::GPT52Codex => {
                "GPT-5.2 variant optimized for agentic coding tasks with reasoning effort support"
            }
            ModelId::GPT5Codex => {
                "Code-focused GPT-5 variant optimized for tool calling and structured outputs"
            }
            ModelId::GPT51 => {
                "Enhanced most capable OpenAI model with improved reasoning and capabilities"
            }
            ModelId::GPT51Codex => {
                "Code-focused GPT-5.1 variant optimized for tool calling and structured outputs"
            }
            ModelId::GPT51CodexMax => {
                "Maximum context code-focused GPT-5.1 variant optimized for large codebases"
            }
            ModelId::GPT51Mini => "Enhanced efficient OpenAI model with improved capabilities",
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
            ModelId::ClaudeOpus41 => "Specialized reasoning model for complex tasks",
            ModelId::ClaudeOpus45 => "Premium flagship model with exceptional intelligence",
            ModelId::ClaudeSonnet45 => "Balanced flagship model for coding and agentic workflows",
            ModelId::ClaudeHaiku45 => "Fastest model with near-frontier intelligence",
            ModelId::ClaudeOpus4 => "Previous generation premium flagship model",
            ModelId::ClaudeSonnet4 => "Standard balanced model for general tasks",
            ModelId::ClaudeSonnet37 => "Latest model in the Claude 3 family with extended thinking",
            ModelId::ClaudeHaiku35 => "Highly efficient model for high-volume tasks",
            // DeepSeek models
            ModelId::DeepSeekChat => {
                "DeepSeek V3.2 - Fast, efficient chat model for immediate responses"
            }
            ModelId::DeepSeekReasoner => {
                "DeepSeek V3.2 - Thinking mode with integrated tool-use and reasoning capability"
            }
            // Hugging Face models
            ModelId::HuggingFaceDeepseekV32 => {
                "DeepSeek V3.2 served via Hugging Face's OpenAI-compatible router"
            }
            ModelId::HuggingFaceOpenAIGptOss20b => {
                "OpenAI GPT-OSS 20B available through Hugging Face Inference Providers"
            }
            ModelId::HuggingFaceOpenAIGptOss120b => {
                "OpenAI GPT-OSS 120B available through Hugging Face Inference Providers"
            }
            ModelId::HuggingFaceGlm47 => {
                "Z.AI GLM-4.7 long-context reasoning model served via Hugging Face router"
            }
            ModelId::HuggingFaceGlm47Novita => {
                "Z.AI GLM-4.7 via Novita inference provider on HuggingFace router."
            }
            ModelId::HuggingFaceGlm47FlashNovita => {
                "Z.AI GLM-4.7-Flash via Novita inference provider on HuggingFace router. Lightweight model optimized for agentic coding."
            }
            ModelId::HuggingFaceKimiK2Thinking => {
                "MoonshotAI Kimi K2 Thinking routed through Hugging Face"
            }
            ModelId::HuggingFaceKimiK25Novita => {
                "MoonshotAI Kimi K2.5 via Novita inference provider on Hugging Face router"
            }
            ModelId::HuggingFaceMinimaxM21Novita => {
                "MiniMax-M2.1 model via Novita inference provider on HuggingFace router. Enhanced reasoning capabilities."
            }
            ModelId::HuggingFaceDeepseekV32Novita => {
                "DeepSeek-V3.2 via Novita inference provider on HuggingFace router."
            }
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => {
                "Xiaomi MiMo-V2-Flash via Novita on HuggingFace router."
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
            ModelId::ZaiGlm47 => {
                "Latest Z.AI GLM flagship reasoning model with improved capabilities and 200K context"
            }
            ModelId::ZaiGlm47DeepThinking => {
                "Latest Z.AI GLM flagship with forced Deep Thinking mode for complex reasoning tasks"
            }
            ModelId::ZaiGlm47Flash => {
                "Z.AI GLM-4.7-Flash 30B-class SOTA lightweight model - Completely free, high-speed, optimized for agentic coding with enhanced reasoning"
            }
            ModelId::ZaiGlm46 => {
                "Latest Z.AI GLM flagship with long-context reasoning and coding strengths"
            }
            ModelId::ZaiGlm46DeepThinking => {
                "Latest Z.AI GLM flagship with forced Deep Thinking mode for enhanced logical analysis"
            }
            ModelId::ZaiGlm45 => "Balanced GLM 4.5 release for general assistant tasks",
            ModelId::ZaiGlm45DeepThinking => {
                "Balanced GLM 4.5 model with forced Deep Thinking for multi-step problem solving"
            }
            ModelId::ZaiGlm45Air => "Efficient GLM 4.5 Air variant tuned for lower latency",
            ModelId::ZaiGlm45X => "Enhanced GLM 4.5 X variant with improved reasoning",
            ModelId::ZaiGlm45Airx => "Hybrid GLM 4.5 AirX variant blending efficiency with quality",
            ModelId::ZaiGlm45Flash => "Low-latency GLM 4.5 Flash optimized for responsiveness",
            ModelId::ZaiGlm432b0414128k => {
                "Legacy GLM 4 32B deployment offering extended 128K context window"
            }
            // Moonshot models
            ModelId::MoonshotKimiK25 => {
                "Kimi K2.5 multimodal model supporting text + vision, thinking modes, tool calls, JSON mode, and long-context reasoning"
            }
            ModelId::OllamaGptOss20b => {
                "Local GPT-OSS 20B deployment served via Ollama with no external API dependency"
            }
            ModelId::OllamaGptOss120bCloud => {
                "Cloud-hosted GPT-OSS 120B accessed through Ollama Cloud for larger reasoning tasks"
            }
            ModelId::OllamaGptOss20bCloud => {
                "Cloud-hosted GPT-OSS 20B accessed through Ollama Cloud for enhanced reasoning tasks"
            }
            ModelId::OllamaQwen317b => {
                "Qwen3 1.7B served locally through Ollama without external API requirements"
            }
            ModelId::OllamaDeepseekV32Cloud => {
                "DeepSeek V3.2 cloud deployment via Ollama with enhanced reasoning and instruction following"
            }
            ModelId::OllamaQwen3Next80bCloud => {
                "Qwen3 Next generation 80B model via Ollama Cloud with improved reasoning and long context"
            }
            ModelId::OllamaMistralLarge3675bCloud => {
                "Mistral Large 3 675B reasoning model via Ollama Cloud for complex problem-solving"
            }
            ModelId::OllamaKimiK2ThinkingCloud => {
                "MoonshotAI Kimi K2 thinking model via Ollama Cloud with explicit reasoning traces"
            }
            ModelId::OllamaKimiK25Cloud => {
                "MoonshotAI Kimi K2.5 versatile multimodal model via Ollama Cloud with reasoning support"
            }
            ModelId::OllamaGlm47Cloud => "Advancing the Coding Capability",
            ModelId::OllamaMinimaxM21Cloud => {
                "Exceptional multilingual capabilities to elevate code engineering"
            }
            ModelId::OllamaGemini3FlashPreviewCloud => {
                "Gemini 3 Flash offers frontier intelligence built for speed at a fraction of the cost."
            }

            ModelId::OllamaGemini3ProPreviewLatestCloud => {
                "Google Gemini 3 Pro Preview latest model via Ollama Cloud with multimodal capabilities"
            }
            ModelId::OllamaDevstral2123bCloud => {
                "Mistral Devstral 2 123B model via Ollama Cloud optimized for development and coding tasks"
            }

            ModelId::OllamaMinimaxM2Cloud => {
                "Cloud-hosted MiniMax-M2 accessed through Ollama Cloud with reasoning and tool use"
            }
            ModelId::OllamaQwen3Coder480bCloud => {
                "Qwen3 Coder 480B expert model provided by Ollama Cloud for complex code generation"
            }
            ModelId::OllamaGlm46Cloud => {
                "GLM 4.6 reasoning model offered by Ollama Cloud with extended context support"
            }
            ModelId::OllamaNemotron3Nano30bCloud => {
                "NVIDIA Nemotron-3-Nano 30B deployed via Ollama Cloud for efficient inference"
            }
            ModelId::LmStudioMetaLlama38BInstruct => {
                "Meta Llama 3 8B running through LM Studio's local OpenAI-compatible server"
            }
            ModelId::LmStudioMetaLlama318BInstruct => {
                "Meta Llama 3.1 8B running through LM Studio's local OpenAI-compatible server"
            }
            ModelId::LmStudioQwen257BInstruct => {
                "Qwen2.5 7B hosted in LM Studio for local experimentation and coding tasks"
            }
            ModelId::LmStudioGemma22BIt => {
                "Gemma 2 2B IT deployed via LM Studio for lightweight on-device assistance"
            }
            ModelId::LmStudioGemma29BIt => {
                "Gemma 2 9B IT served locally via LM Studio when you need additional capacity"
            }
            ModelId::LmStudioPhi31Mini4kInstruct => {
                "Phi-3.1 Mini 4K hosted in LM Studio for compact reasoning and experimentation"
            }
            // MiniMax models
            ModelId::MinimaxM21 => {
                "Latest MiniMax-M2.1 model with enhanced code understanding and reasoning"
            }
            ModelId::MinimaxM21Lightning => {
                "Fast version of MiniMax-M2.1 for rapid conversational tasks"
            }
            ModelId::MinimaxM2 => {
                "MiniMax-M2 via Anthropic-compatible API with reasoning and tool use"
            }
            // OpenRouter models - fallback for any OpenRouter model without metadata
            ModelId::OpenRouterGrokCodeFast1
            | ModelId::OpenRouterGrok4Fast
            | ModelId::OpenRouterGrok41Fast
            | ModelId::OpenRouterGrok4
            | ModelId::OpenRouterZaiGlm46
            | ModelId::OpenRouterQwen3Max
            | ModelId::OpenRouterQwen3235bA22b
            | ModelId::OpenRouterQwen3235bA22b2507
            | ModelId::OpenRouterQwen3235bA22bThinking2507
            | ModelId::OpenRouterQwen332b
            | ModelId::OpenRouterQwen330bA3b
            | ModelId::OpenRouterQwen330bA3bInstruct2507
            | ModelId::OpenRouterQwen330bA3bThinking2507
            | ModelId::OpenRouterQwen314b
            | ModelId::OpenRouterQwen38b
            | ModelId::OpenRouterQwen3Next80bA3bInstruct
            | ModelId::OpenRouterQwen3Next80bA3bThinking
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash
            | ModelId::OpenRouterQwen3Coder30bA3bInstruct
            | ModelId::OpenRouterDeepSeekV32Exp
            | ModelId::OpenRouterDeepSeekChatV31
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGptOss120b
            | ModelId::OpenRouterOpenAIGptOss120bFree
            | ModelId::OpenRouterOpenAIGptOss20b
            | ModelId::OpenRouterOpenAIGpt5
            | ModelId::OpenRouterOpenAIGpt5Codex
            | ModelId::OpenRouterOpenAIGpt5Chat
            | ModelId::OpenRouterOpenAIGpt4oSearchPreview
            | ModelId::OpenRouterOpenAIGpt4oMiniSearchPreview
            | ModelId::OpenRouterOpenAIChatgpt4oLatest
            | ModelId::OpenRouterAnthropicClaudeSonnet45
            | ModelId::OpenRouterAnthropicClaudeHaiku45
            | ModelId::OpenRouterAnthropicClaudeOpus41
            | ModelId::OpenRouterDeepseekChat
            | ModelId::OpenRouterDeepSeekV32
            | ModelId::OpenRouterDeepseekReasoner
            | ModelId::OpenRouterDeepSeekV32Speciale
            | ModelId::OpenRouterMoonshotaiKimiK20905
            | ModelId::OpenRouterMoonshotaiKimiK2Thinking
            | ModelId::OpenRouterAmazonNova2LiteV1
            | ModelId::OpenRouterMistralaiMistralLarge2512
            | ModelId::OpenRouterNexAgiDeepseekV31NexN1
            | ModelId::OpenRouterOpenAIGpt51
            | ModelId::OpenRouterOpenAIGpt51Codex
            | ModelId::OpenRouterOpenAIGpt51CodexMax
            | ModelId::OpenRouterOpenAIGpt51CodexMini
            | ModelId::OpenRouterOpenAIGpt51Chat
            | ModelId::OpenRouterOpenAIGpt52
            | ModelId::OpenRouterOpenAIGpt52Chat
            | ModelId::OpenRouterOpenAIGpt52Codex
            | ModelId::OpenRouterOpenAIGpt52Pro
            | ModelId::OpenRouterOpenAIO1Pro
            | ModelId::OpenRouterZaiGlm46V
            | ModelId::OpenRouterZaiGlm47
            | ModelId::OpenRouterZaiGlm47Flash
            | ModelId::OpenRouterMoonshotaiKimiK25 => {
                // Fallback description for OpenRouter models
                // In production, these should have metadata
                "Model available via OpenRouter marketplace"
            }
        }
    }

    /// Return the OpenRouter vendor slug when this identifier maps to a marketplace listing
    pub fn openrouter_vendor(&self) -> Option<&'static str> {
        self.openrouter_metadata().map(|meta| meta.vendor)
    }
    pub fn all_models() -> Vec<ModelId> {
        let mut models = vec![
            // Gemini models
            ModelId::Gemini25FlashPreview,
            ModelId::Gemini25Flash,
            ModelId::Gemini25FlashLite,
            ModelId::Gemini25Pro,
            ModelId::Gemini3ProPreview,
            ModelId::Gemini3FlashPreview,
            // OpenAI models
            ModelId::GPT5,
            ModelId::GPT52,
            ModelId::GPT52Codex,
            ModelId::GPT5Codex,
            ModelId::GPT5Mini,
            ModelId::GPT5Nano,
            ModelId::GPT51,
            ModelId::GPT51Codex,
            ModelId::GPT51CodexMax,
            ModelId::GPT51Mini,
            ModelId::CodexMiniLatest,
            // Anthropic models
            ModelId::ClaudeOpus45,
            ModelId::ClaudeOpus41,
            ModelId::ClaudeSonnet45,
            ModelId::ClaudeHaiku45,
            ModelId::ClaudeOpus4,
            ModelId::ClaudeSonnet4,
            ModelId::ClaudeSonnet37,
            ModelId::ClaudeHaiku35,
            // DeepSeek models
            ModelId::DeepSeekChat,
            ModelId::DeepSeekReasoner,
            // Hugging Face models
            ModelId::HuggingFaceDeepseekV32,
            ModelId::HuggingFaceOpenAIGptOss20b,
            ModelId::HuggingFaceOpenAIGptOss120b,
            ModelId::HuggingFaceGlm47,
            ModelId::HuggingFaceGlm47Novita,
            ModelId::HuggingFaceGlm47FlashNovita,
            ModelId::HuggingFaceKimiK2Thinking,
            ModelId::HuggingFaceKimiK25Novita,
            ModelId::HuggingFaceMinimaxM21Novita,
            ModelId::HuggingFaceDeepseekV32Novita,
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita,
            // xAI models
            ModelId::XaiGrok4,
            ModelId::XaiGrok4Mini,
            ModelId::XaiGrok4Code,
            ModelId::XaiGrok4CodeLatest,
            ModelId::XaiGrok4Vision,
            // Z.AI models
            ModelId::ZaiGlm47,
            ModelId::ZaiGlm47DeepThinking,
            ModelId::ZaiGlm47Flash,
            ModelId::ZaiGlm46,
            ModelId::ZaiGlm46DeepThinking,
            ModelId::ZaiGlm45,
            ModelId::ZaiGlm45DeepThinking,
            ModelId::ZaiGlm45Air,
            ModelId::ZaiGlm45X,
            ModelId::ZaiGlm45Airx,
            ModelId::ZaiGlm45Flash,
            ModelId::ZaiGlm432b0414128k,
            // Moonshot models
            ModelId::MoonshotKimiK25,
            // Ollama models
            ModelId::OllamaGptOss20b,
            ModelId::OllamaGptOss20bCloud,
            ModelId::OllamaGptOss120bCloud,
            ModelId::OllamaQwen317b,
            ModelId::OllamaDeepseekV32Cloud,
            ModelId::OllamaQwen3Next80bCloud,
            ModelId::OllamaMistralLarge3675bCloud,
            ModelId::OllamaKimiK2ThinkingCloud,
            ModelId::OllamaKimiK25Cloud,
            ModelId::OllamaQwen3Coder480bCloud,
            ModelId::OllamaGlm46Cloud,
            ModelId::OllamaGlm47Cloud,
            ModelId::OllamaGemini3ProPreviewLatestCloud,
            ModelId::OllamaGemini3FlashPreviewCloud,
            ModelId::OllamaDevstral2123bCloud,
            ModelId::OllamaMinimaxM2Cloud,
            ModelId::OllamaMinimaxM21Cloud,
            ModelId::OllamaNemotron3Nano30bCloud,
            // LM Studio models
            ModelId::LmStudioMetaLlama38BInstruct,
            ModelId::LmStudioMetaLlama318BInstruct,
            ModelId::LmStudioQwen257BInstruct,
            ModelId::LmStudioGemma22BIt,
            ModelId::LmStudioGemma29BIt,
            ModelId::LmStudioPhi31Mini4kInstruct,
            // MiniMax models
            ModelId::MinimaxM21,
            ModelId::MinimaxM21Lightning,
            ModelId::MinimaxM2,
        ];
        models.extend(Self::openrouter_models());
        models
    }
}
