use super::ModelId;

impl ModelId {
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
            ModelId::ClaudeOpus46 => {
                "Next-gen Anthropic flagship with extended and adaptive thinking support"
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
            ModelId::ZaiGlm4Plus | ModelId::ZaiGlm4PlusDeepThinking => {
                "Z.AI flagship model with top-tier capability in reasoning, writing, and tool-use"
            }
            ModelId::ZaiGlm47 | ModelId::ZaiGlm47DeepThinking => {
                "Latest Z.AI GLM flagship with enhanced reasoning, 200k context and coding strengths"
            }
            ModelId::ZaiGlm47Flash => {
                "Z.AI GLM-4.7-Flash 30B-class SOTA lightweight model - Completely free, high-speed, optimized for agentic coding with enhanced reasoning capabilities"
            }
            ModelId::ZaiGlm46 | ModelId::ZaiGlm46DeepThinking => {
                "Previous Z.AI GLM flagship with long-context reasoning and coding strengths"
            }
            ModelId::ZaiGlm46V | ModelId::ZaiGlm46VFlash | ModelId::ZaiGlm46VFlashX => {
                "Vision-capable GLM 4.6 release optimized for multimodal understanding"
            }
            ModelId::ZaiGlm45 | ModelId::ZaiGlm45DeepThinking => {
                "Balanced GLM 4.5 release for general assistant tasks"
            }
            ModelId::ZaiGlm45Air => "Efficient GLM 4.5 Air variant tuned for lower latency",
            ModelId::ZaiGlm45X => "Enhanced GLM 4.5 X variant with improved reasoning",
            ModelId::ZaiGlm45Airx => "Hybrid GLM 4.5 AirX variant blending efficiency with quality",
            ModelId::ZaiGlm45Flash => "Low-latency GLM 4.5 Flash optimized for responsiveness",
            ModelId::ZaiGlm45V => "Vision-capable GLM 4.5 release for multimodal tasks",
            ModelId::ZaiGlm432b0414128k => {
                "Legacy GLM 4 32B deployment offering extended 128K context window"
            }
            ModelId::MoonshotKimiK25 => {
                "Kimi K2.5 multimodal model supporting text + vision, thinking modes, tool calls, JSON mode, and long-context reasoning"
            }
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
                "Gemini 3 Pro Preview Latest offers advanced reasoning and long context capabilities."
            }
            ModelId::OllamaNemotron3Nano30bCloud => {
                "NVIDIA Nemotron-3-Nano 30B brings efficient excellence to code"
            }
            ModelId::OllamaQwen3Coder480bCloud => {
                "Cloud-hosted Qwen3 Coder 480B model accessed through Ollama Cloud for coding tasks"
            }
            ModelId::OllamaGlm46Cloud => {
                "Cloud-hosted GLM-4.6 model accessed through Ollama Cloud for reasoning and coding"
            }
            ModelId::OllamaMinimaxM2Cloud => {
                "Cloud-hosted MiniMax-M2 model accessed through Ollama Cloud for reasoning tasks"
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
            ModelId::MinimaxM21 => {
                "Latest MiniMax-M2.1 model with enhanced code understanding and reasoning"
            }
            ModelId::MinimaxM21Lightning => {
                "Fast version of MiniMax-M2.1 for rapid conversational tasks"
            }
            ModelId::MinimaxM2 => {
                "MiniMax-M2 via Anthropic-compatible API with reasoning and tool use"
            }
            ModelId::HuggingFaceDeepseekV32 => {
                "DeepSeek-V3.2 via Hugging Face router for advanced reasoning"
            }
            ModelId::HuggingFaceOpenAIGptOss20b => "OpenAI GPT-OSS 20B via Hugging Face router",
            ModelId::HuggingFaceOpenAIGptOss120b => "OpenAI GPT-OSS 120B via Hugging Face router",
            ModelId::HuggingFaceGlm47 => "Z.AI GLM-4.7 via Hugging Face router",
            ModelId::HuggingFaceGlm47FlashNovita => {
                "Z.AI GLM-4.7-Flash via Novita inference provider on HuggingFace router. Lightweight model optimized for agentic coding."
            }
            ModelId::HuggingFaceKimiK2Thinking => {
                "MoonshotAI Kimi K2 Thinking via Hugging Face router"
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
            ModelId::HuggingFaceQwen3CoderNextNovita => {
                "Qwen3-Coder-Next via Novita inference provider on HuggingFace router. Coding-optimized model with reasoning capabilities."
            }
            _ => unreachable!(),
        }
    }
}
