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
            ModelId::Gemini31ProPreview => "Gemini 3.1 Pro Preview",
            ModelId::Gemini31ProPreviewCustomTools => "Gemini 3.1 Pro Preview (Custom Tools)",
            ModelId::Gemini3ProPreview => "Gemini 3 Pro Preview",
            ModelId::Gemini3FlashPreview => "Gemini 3 Flash",
            // OpenAI models
            ModelId::GPT5 => "GPT-5",
            ModelId::GPT52 => "GPT-5.2",
            ModelId::GPT52Codex => "GPT-5.2 Codex",
            ModelId::GPT53Codex => "GPT-5.3 Codex",
            ModelId::GPT5Mini => "GPT-5 Mini",
            ModelId::GPT5Nano => "GPT-5 Nano",
            ModelId::OpenAIGptOss20b => "GPT-OSS 20B",
            ModelId::OpenAIGptOss120b => "GPT-OSS 120B",
            // Anthropic models
            ModelId::ClaudeOpus46 => "Claude Opus 4.6",
            ModelId::ClaudeSonnet46 => "Claude Sonnet 4.6",
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
            ModelId::HuggingFaceMinimaxM25Novita => "MiniMax-M2.5 (Novita)",
            ModelId::HuggingFaceDeepseekV32Novita => "DeepSeek V3.2 (Novita)",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "MiMo-V2-Flash (Novita)",
            ModelId::HuggingFaceGlm5Novita => "GLM-5 (Novita)",
            ModelId::HuggingFaceQwen3CoderNextNovita => "Qwen3-Coder-Next (Novita)",
            // xAI models
            ModelId::XaiGrok4 => "Grok-4",
            ModelId::XaiGrok4Mini => "Grok-4 Mini",
            ModelId::XaiGrok4Code => "Grok-4 Code",
            ModelId::XaiGrok4CodeLatest => "Grok-4 Code Latest",
            ModelId::XaiGrok4Vision => "Grok-4 Vision",
            // Z.AI models
            ModelId::ZaiGlm5 => "GLM-5",
            // Ollama models
            ModelId::OllamaGptOss20b => "GPT-OSS 20B (local)",
            ModelId::OllamaGptOss20bCloud => "GPT-OSS 20B (cloud)",
            ModelId::OllamaGptOss120bCloud => "GPT-OSS 120B (cloud)",
            ModelId::OllamaQwen317b => "Qwen3 1.7B (local)",
            ModelId::OllamaDeepseekV32Cloud => "DeepSeek V3.2 (cloud)",
            ModelId::OllamaQwen3Next80bCloud => "Qwen3 Next 80B (cloud)",
            ModelId::OllamaMistralLarge3675bCloud => "Mistral Large 3 675B (cloud)",
            ModelId::OllamaQwen3Coder480bCloud => "Qwen3 Coder 480B (cloud)",
            ModelId::OllamaGemini3ProPreviewLatestCloud => "Gemini 3 Pro Preview (cloud)",
            ModelId::OllamaDevstral2123bCloud => "Devstral 2 123B (cloud)",
            ModelId::OllamaMinimaxM2Cloud => "MiniMax-M2 (cloud)",
            ModelId::OllamaNemotron3Nano30bCloud => "Nemotron-3-Nano 30B (cloud)",
            ModelId::OllamaGlm5Cloud => "GLM-5 (cloud)",
            ModelId::OllamaMinimaxM25Cloud => "MiniMax-M2.5 (cloud)",
            ModelId::OllamaGemini3FlashPreviewCloud => "Gemini 3 Flash Preview (cloud)",
            ModelId::LmStudioMetaLlama38BInstruct => "Meta Llama 3 8B (LM Studio)",
            ModelId::LmStudioMetaLlama318BInstruct => "Meta Llama 3.1 8B (LM Studio)",
            ModelId::LmStudioQwen257BInstruct => "Qwen2.5 7B (LM Studio)",
            ModelId::LmStudioGemma22BIt => "Gemma 2 2B (LM Studio)",
            ModelId::LmStudioGemma29BIt => "Gemma 2 9B (LM Studio)",
            ModelId::LmStudioPhi31Mini4kInstruct => "Phi-3.1 Mini 4K (LM Studio)",
            // MiniMax models
            ModelId::MinimaxM25 => "MiniMax-M2.5",
            ModelId::MinimaxM2 => "MiniMax-M2",
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
            ModelId::Gemini31ProPreview => {
                "Latest Gemini 3.1 Pro flagship model with improved thinking, efficiency, and factual consistency"
            }
            ModelId::Gemini31ProPreviewCustomTools => {
                "Gemini 3.1 Pro variant optimized for agentic workflows using custom tools and bash"
            }
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
            ModelId::GPT53Codex => {
                "GPT-5.3 variant optimized for agentic coding tasks with xhigh reasoning effort support"
            }
            ModelId::GPT5Mini => "Latest efficient OpenAI model, great for most tasks",
            ModelId::GPT5Nano => "Latest most cost-effective OpenAI model",
            ModelId::OpenAIGptOss20b => {
                "OpenAI's open-source 20B parameter GPT-OSS model using harmony tokenization"
            }
            ModelId::OpenAIGptOss120b => {
                "OpenAI's open-source 120B parameter GPT-OSS model using harmony tokenization"
            }
            // Anthropic models
            ModelId::ClaudeOpus46 => {
                "Next-gen Anthropic flagship with extended and adaptive thinking support"
            }
            ModelId::ClaudeSonnet46 => {
                "Balanced flagship model for coding with extended and adaptive thinking support"
            }
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
            ModelId::HuggingFaceMinimaxM25Novita => {
                "MiniMax-M2.5 model via Novita inference provider on HuggingFace router. Enhanced reasoning capabilities."
            }
            ModelId::HuggingFaceDeepseekV32Novita => {
                "DeepSeek-V3.2 via Novita inference provider on HuggingFace router."
            }
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => {
                "Xiaomi MiMo-V2-Flash via Novita on HuggingFace router."
            }
            ModelId::HuggingFaceGlm5Novita => {
                "Z.AI GLM-5 via Novita inference provider on HuggingFace router. Flagship foundation model for complex systems."
            }
            ModelId::HuggingFaceQwen3CoderNextNovita => {
                "Qwen3-Coder-Next via Novita inference provider on HuggingFace router. Coding-optimized model with reasoning capabilities."
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
            ModelId::ZaiGlm5 => {
                "Z.ai's flagship open-source foundation model engineered for complex systems design and long-horizon agent workflows. Built for expert developers, it delivers production-grade performance on large-scale programming tasks, rivaling leading closed-source models. With advanced agentic planning, deep backend reasoning, and iterative self-correction, GLM-5 moves beyond code generation to full-system construction and autonomous execution."
            }
            // Ollama models
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
            ModelId::OllamaGlm5Cloud => "Cloud-hosted GLM-5 model served via Ollama Cloud",
            ModelId::OllamaMinimaxM25Cloud => {
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
            ModelId::MinimaxM25 => {
                "Latest MiniMax-M2.5 model with enhanced code understanding and reasoning"
            }
            ModelId::MinimaxM2 => {
                "MiniMax-M2 via Anthropic-compatible API with reasoning and tool use"
            }
            // OpenRouter models - fallback for any OpenRouter model without metadata
            ModelId::OpenRouterGrokCodeFast1
            | ModelId::OpenRouterGrok4Fast
            | ModelId::OpenRouterGrok41Fast
            | ModelId::OpenRouterGrok4
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
            | ModelId::OpenRouterQwen35Plus0215
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash
            | ModelId::OpenRouterQwen3Coder30bA3bInstruct
            | ModelId::OpenRouterQwen3CoderNext
            | ModelId::OpenRouterDeepSeekV32Exp
            | ModelId::OpenRouterDeepSeekChatV31
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGptOss120b
            | ModelId::OpenRouterOpenAIGptOss120bFree
            | ModelId::OpenRouterOpenAIGptOss20b
            | ModelId::OpenRouterOpenAIGpt5
            | ModelId::OpenRouterOpenAIGpt5Chat
            | ModelId::OpenRouterGoogleGemini31ProPreview
            | ModelId::OpenRouterAnthropicClaudeSonnet45
            | ModelId::OpenRouterAnthropicClaudeSonnet46
            | ModelId::OpenRouterAnthropicClaudeHaiku45
            | ModelId::OpenRouterAnthropicClaudeOpus41
            | ModelId::OpenRouterDeepseekChat
            | ModelId::OpenRouterDeepSeekV32
            | ModelId::OpenRouterDeepseekReasoner
            | ModelId::OpenRouterDeepSeekV32Speciale
            | ModelId::OpenRouterAmazonNova2LiteV1
            | ModelId::OpenRouterMistralaiMistralLarge2512
            | ModelId::OpenRouterNexAgiDeepseekV31NexN1
            | ModelId::OpenRouterOpenAIGpt52
            | ModelId::OpenRouterOpenAIGpt52Chat
            | ModelId::OpenRouterOpenAIGpt52Codex
            | ModelId::OpenRouterOpenAIGpt52Pro
            | ModelId::OpenRouterOpenAIO1Pro
            | ModelId::OpenRouterStepfunStep35FlashFree
            | ModelId::OpenRouterMoonshotaiKimiK20905
            | ModelId::OpenRouterMoonshotaiKimiK2Thinking
            | ModelId::OpenRouterMoonshotaiKimiK25
            | ModelId::OpenRouterZaiGlm5 => unreachable!(),
        }
    }

    /// Return the OpenRouter vendor slug when this identifier maps to a marketplace listing
    pub fn openrouter_vendor(&self) -> Option<&'static str> {
        self.openrouter_metadata().map(|meta| meta.vendor)
    }
    pub fn all_models() -> Vec<ModelId> {
        let mut models = vec![
            // Gemini models
            ModelId::Gemini31ProPreview,
            ModelId::Gemini31ProPreviewCustomTools,
            ModelId::Gemini3ProPreview,
            ModelId::Gemini3FlashPreview,
            // OpenAI models
            ModelId::GPT5,
            ModelId::GPT52,
            ModelId::GPT52Codex,
            ModelId::GPT53Codex,
            ModelId::GPT5Mini,
            ModelId::GPT5Nano,
            ModelId::OpenAIGptOss20b,
            ModelId::OpenAIGptOss120b,
            // Anthropic models
            ModelId::ClaudeOpus46,
            ModelId::ClaudeSonnet46,
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
            ModelId::HuggingFaceMinimaxM25Novita,
            ModelId::HuggingFaceDeepseekV32Novita,
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita,
            ModelId::HuggingFaceGlm5Novita,
            ModelId::HuggingFaceQwen3CoderNextNovita,
            // xAI models
            ModelId::XaiGrok4,
            ModelId::XaiGrok4Mini,
            ModelId::XaiGrok4Code,
            ModelId::XaiGrok4CodeLatest,
            ModelId::XaiGrok4Vision,
            // Z.AI models
            ModelId::ZaiGlm5,
            // Ollama models
            ModelId::OllamaGptOss20b,
            ModelId::OllamaGptOss20bCloud,
            ModelId::OllamaGptOss120bCloud,
            ModelId::OllamaQwen317b,
            ModelId::OllamaDeepseekV32Cloud,
            ModelId::OllamaQwen3Next80bCloud,
            ModelId::OllamaMistralLarge3675bCloud,
            ModelId::OllamaQwen3Coder480bCloud,
            ModelId::OllamaGlm5Cloud,
            ModelId::OllamaGemini3ProPreviewLatestCloud,
            ModelId::OllamaGemini3FlashPreviewCloud,
            ModelId::OllamaDevstral2123bCloud,
            ModelId::OllamaMinimaxM2Cloud,
            ModelId::OllamaMinimaxM25Cloud,
            ModelId::OllamaNemotron3Nano30bCloud,
            // LM Studio models
            ModelId::LmStudioMetaLlama38BInstruct,
            ModelId::LmStudioMetaLlama318BInstruct,
            ModelId::LmStudioQwen257BInstruct,
            ModelId::LmStudioGemma22BIt,
            ModelId::LmStudioGemma29BIt,
            ModelId::LmStudioPhi31Mini4kInstruct,
            // MiniMax models
            ModelId::MinimaxM25,
            ModelId::MinimaxM2,
        ];
        models.extend(Self::openrouter_models());
        models
    }
}
