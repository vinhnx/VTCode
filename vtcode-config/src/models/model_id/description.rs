use super::ModelId;

impl ModelId {
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
            ModelId::Gemini3FlashPreview => {
                "Our most intelligent model built for speed, combining frontier intelligence with superior search and grounding"
            }
            // OpenAI models
            ModelId::GPT5 => "Latest most capable OpenAI model with advanced reasoning",
            ModelId::GPT52 => {
                "Latest flagship OpenAI model with improved reasoning, xhigh effort, and built-in compaction support"
            }
            ModelId::GPT5Mini => "Latest efficient OpenAI model, great for most tasks",
            ModelId::GPT5Nano => "Latest most cost-effective OpenAI model",
            ModelId::GPT53Codex => {
                "GPT-5.3 variant optimized for agentic coding tasks with reasoning effort support (low, medium, high, xhigh)"
            }
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
            ModelId::ClaudeOpus45 => {
                "Latest flagship Anthropic model with exceptional reasoning capabilities"
            }
            ModelId::ClaudeOpus41 => {
                "Latest flagship Anthropic model with exceptional reasoning capabilities"
            }
            ModelId::ClaudeSonnet45 => "Latest balanced Anthropic model for general tasks",
            ModelId::ClaudeHaiku45 => {
                "Latest efficient Anthropic model optimized for low-latency agent workflows"
            }
            ModelId::ClaudeSonnet4 => {
                "Previous balanced Anthropic model maintained for compatibility"
            }
            // DeepSeek models
            ModelId::DeepSeekChat => {
                "DeepSeek V3.2 - Fast, efficient chat model for immediate responses"
            }
            ModelId::DeepSeekReasoner => {
                "DeepSeek V3.2 - Thinking mode with integrated tool-use and reasoning capability"
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
                "Z.ai flagship GLM-5 foundation model engineered for complex systems design and long-horizon agent workflows"
            }
            // Ollama models
            ModelId::OllamaGptOss20b => {
                "Local GPT-OSS 20B deployment served via Ollama with no external API dependency"
            }
            ModelId::OllamaGptOss20bCloud => {
                "Cloud-hosted GPT-OSS 20B accessed through Ollama Cloud for efficient reasoning tasks"
            }
            ModelId::OllamaGptOss120bCloud => {
                "Cloud-hosted GPT-OSS 120B accessed through Ollama Cloud for larger reasoning tasks"
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
            ModelId::OllamaDevstral2123bCloud => {
                "Mistral Devstral 2 123B cloud deployment via Ollama for advanced coding workflows"
            }
            ModelId::OllamaNemotron3Nano30bCloud => {
                "NVIDIA Nemotron-3-Nano 30B brings efficient excellence to code"
            }
            ModelId::OllamaQwen3Coder480bCloud => {
                "Cloud-hosted Qwen3 Coder 480B model accessed through Ollama Cloud for coding tasks"
            }
            ModelId::OllamaMinimaxM2Cloud => {
                "Cloud-hosted MiniMax-M2 model accessed through Ollama Cloud for reasoning tasks"
            }
            ModelId::MinimaxM25 => {
                "Latest MiniMax-M2.5 model with further improvements in reasoning and coding"
            }
            ModelId::MinimaxM2 => {
                "MiniMax-M2 via Anthropic-compatible API with reasoning and tool use"
            }
            ModelId::MoonshotMinimaxM25 => "MiniMax-M2.5 served via Moonshot API",
            ModelId::MoonshotQwen3CoderNext => "Qwen3 Coder Next model served via Moonshot API",
            ModelId::HuggingFaceDeepseekV32 => {
                "DeepSeek-V3.2 via Hugging Face router for advanced reasoning"
            }
            ModelId::HuggingFaceOpenAIGptOss20b => "OpenAI GPT-OSS 20B via Hugging Face router",
            ModelId::HuggingFaceOpenAIGptOss120b => "OpenAI GPT-OSS 120B via Hugging Face router",
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
                "Z.ai GLM-5 flagship model via Novita inference provider on HuggingFace router."
            }
            ModelId::HuggingFaceQwen3CoderNextNovita => {
                "Qwen3-Coder-Next via Novita inference provider on HuggingFace router. Coding-optimized model with reasoning capabilities."
            }
            ModelId::OpenRouterMinimaxM25 => "MiniMax-M2.5 flagship model via OpenRouter",
            ModelId::OpenRouterQwen3CoderNext => {
                "Next-generation Qwen3 coding model optimized for agentic workflows via OpenRouter"
            }
            _ => unreachable!(),
        }
    }
}
