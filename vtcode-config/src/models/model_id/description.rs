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
            ModelId::Gemini31FlashLitePreview => {
                "Most cost-efficient Gemini 3.1 model, offering fastest performance for high-frequency, lightweight tasks"
            }
            ModelId::Gemini3FlashPreview => {
                "Our most intelligent model built for speed, combining frontier intelligence with superior search and grounding"
            }
            // OpenAI models
            ModelId::GPT55 => {
                "Next-gen OpenAI model with frontier reasoning and long context (2026-04-23 dated release)"
            }
            ModelId::GPT5 => "Latest most capable OpenAI model with advanced reasoning",
            ModelId::GPT52 => {
                "Latest flagship OpenAI model with improved reasoning, xhigh effort, and built-in compaction support"
            }
            ModelId::GPT52Codex => {
                "GPT-5.2 Codex variant optimized for agentic coding tasks with xhigh reasoning support"
            }
            ModelId::GPT54 => {
                "Mainline frontier GPT model for general-purpose work, coding, long context, and multi-step agents"
            }
            ModelId::GPT54Pro => {
                "Higher-compute GPT-5.4 variant for tougher problems with deeper reasoning"
            }
            ModelId::GPT54Nano => {
                "Lightweight GPT-5.4 variant optimized for speed and cost-efficiency"
            }
            ModelId::GPT54Mini => {
                "Compact GPT-5.4 variant for cost-effective tasks with reduced reasoning overhead"
            }
            ModelId::GPT53Codex => {
                "GPT-5.3 variant optimized for agentic coding tasks with reasoning effort support (low, medium, high, xhigh)"
            }
            ModelId::GPT51Codex => {
                "GPT-5.1 variant optimized for agentic coding tasks and software engineering workflows"
            }
            ModelId::GPT51CodexMax => {
                "Higher-compute GPT-5.1 Codex variant optimized for longer-running engineering tasks"
            }
            ModelId::GPT5Mini => "Latest efficient OpenAI model, great for most tasks",
            ModelId::GPT5Nano => "Latest most cost-effective OpenAI model",
            ModelId::GPT5Codex => {
                "GPT-5 variant optimized for agentic coding tasks and software engineering workflows"
            }
            ModelId::OpenAIGptOss20b => {
                "OpenAI's open-source 20B parameter GPT-OSS model using harmony tokenization"
            }
            ModelId::OpenAIGptOss120b => {
                "OpenAI's open-source 120B parameter GPT-OSS model using harmony tokenization"
            }
            // Anthropic models
            ModelId::ClaudeOpus47 => {
                "Next-gen Anthropic flagship with adaptive thinking and task budget support"
            }
            ModelId::ClaudeOpus46 => {
                "Previous Anthropic flagship retained on VT Code's budgeted-thinking path for compatibility"
            }
            ModelId::ClaudeSonnet46 => {
                "Balanced flagship model for coding with budgeted thinking in VT Code's current Anthropic rollout"
            }
            ModelId::ClaudeHaiku45 => {
                "Latest efficient Anthropic model optimized for low-latency agent workflows"
            }
            ModelId::ClaudeMythosPreview => {
                "Invitation-only Anthropic research preview for defensive cybersecurity workflows with adaptive thinking"
            }
            ModelId::CopilotAuto => {
                "GitHub Copilot preview provider with automatic model selection via the official Copilot CLI"
            }
            ModelId::CopilotGPT52Codex => {
                "GitHub Copilot GPT-5.2 Codex option for agentic software engineering workflows"
            }
            ModelId::CopilotGPT51CodexMax => {
                "GitHub Copilot GPT-5.1 Codex Max option for longer-running engineering tasks"
            }
            ModelId::CopilotGPT54 => {
                "GitHub Copilot GPT-5.4 option for complex professional work and long context"
            }
            ModelId::CopilotGPT54Mini => {
                "GitHub Copilot GPT-5.4 Mini option for faster, lighter-weight tasks"
            }
            ModelId::CopilotClaudeSonnet46 => {
                "GitHub Copilot Claude Sonnet 4.6 option for balanced coding and reasoning work"
            }
            // DeepSeek models
            ModelId::DeepSeekV4Pro => {
                "DeepSeek V4 Pro - High-performance reasoning model with advanced thinking capabilities (1M context, 384K max output)"
            }
            ModelId::DeepSeekV4Flash => {
                "DeepSeek V4 Flash - Fast inference model for cost-effective reasoning tasks (1M context, 384K max output)"
            }
            // Z.AI models
            ModelId::ZaiGlm5 => {
                "Z.ai flagship GLM-5 foundation model engineered for complex systems design and long-horizon agent workflows"
            }
            ModelId::ZaiGlm51 => {
                "Z.ai next-gen GLM-5.1 foundation model with improved reasoning and agent capabilities"
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
            ModelId::OllamaQwen3CoderNext => {
                "Qwen3-Coder-Next served via Ollama Cloud with 256K context, strong coding/tool-use performance, and non-thinking mode responses"
            }
            ModelId::OllamaDeepseekV32Cloud => {
                "DeepSeek V3.2 cloud deployment via Ollama with enhanced reasoning and instruction following"
            }
            ModelId::OllamaDeepseekV4FlashCloud => {
                "DeepSeek V4 Flash cloud deployment via Ollama with fast inference and efficient reasoning"
            }
            ModelId::OllamaDeepseekV4ProCloud => {
                "DeepSeek V4 Pro cloud deployment via Ollama with advanced thinking and strong reasoning"
            }
            ModelId::OllamaQwen3Next80bCloud => {
                "Qwen3 Next generation 80B model via Ollama Cloud with improved reasoning and long context"
            }
            ModelId::OllamaGlm5Cloud => "Cloud-hosted GLM-5 model served via Ollama Cloud",
            ModelId::OllamaGlm51Cloud => "Cloud-hosted GLM-5.1 model served via Ollama Cloud",
            ModelId::OllamaMinimaxM25Cloud => {
                "Exceptional multilingual capabilities to elevate code engineering"
            }
            ModelId::OllamaGemini3FlashPreviewCloud => {
                "Gemini 3 Flash offers frontier intelligence built for speed at a fraction of the cost."
            }
            ModelId::OllamaKimiK26Cloud => "Cloud-hosted Kimi K2.6 model served via Ollama Cloud",
            ModelId::OllamaNemotron3SuperCloud => {
                "NVIDIA Nemotron™ is a family of open models delivering leading efficiency and accuracy for building specialized AI agents. Nemotron-3-Super (120B) is optimized for collaborative agents and high-volume workloads."
            }
            ModelId::OllamaMinimaxM2Cloud => {
                "Cloud-hosted MiniMax-M2 model accessed through Ollama Cloud for reasoning tasks"
            }
            ModelId::OllamaMinimaxM27Cloud => {
                "Cloud-hosted MiniMax-M2.7 model accessed through Ollama Cloud for reasoning tasks"
            }
            ModelId::MinimaxM27 => {
                "Beginning the journey of recursive self-improvement with 204.8K context and strong reasoning/coding performance"
            }
            ModelId::MinimaxM25 => {
                "Latest MiniMax-M2.5 model with further improvements in reasoning and coding"
            }
            ModelId::MoonshotKimiK26 => {
                "Kimi K2.6 - Moonshot.ai's latest 1T MoE flagship with 32B active parameters, MLA attention, and MoonViT vision"
            }
            ModelId::MoonshotKimiK25 => {
                "Kimi K2.5 - Moonshot.ai's previous flagship reasoning model"
            }
            ModelId::OpenCodeZenGPT54 => {
                "OpenCode Zen flagship GPT-5.4 route using OpenCode's curated pay-as-you-go gateway"
            }
            ModelId::OpenCodeZenGPT54Mini => {
                "Lower-cost OpenCode Zen GPT-5.4 Mini option for faster and cheaper tasks"
            }
            ModelId::OpenCodeZenClaudeSonnet46 => {
                "Claude Sonnet 4.6 served through OpenCode Zen's curated Anthropic endpoint"
            }
            ModelId::OpenCodeZenGlm51 => {
                "GLM-5.1 served through OpenCode Zen for lower-cost reasoning and coding work"
            }
            ModelId::OpenCodeZenKimiK25 => {
                "Kimi K2.5 served through OpenCode Zen's curated open-model gateway"
            }
            ModelId::OpenCodeGoGlm51 => {
                "GLM-5.1 included with the OpenCode Go subscription for open-model coding workflows"
            }
            ModelId::OpenCodeGoKimiK25 => "Kimi K2.5 included with the OpenCode Go subscription",
            ModelId::OpenCodeGoMinimaxM25 => {
                "MiniMax-M2.5 included with the OpenCode Go subscription"
            }
            ModelId::OpenCodeGoMinimaxM27 => {
                "MiniMax-M2.7 included with the OpenCode Go subscription for stronger agentic coding"
            }
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
            ModelId::HuggingFaceGlm51ZaiOrg => {
                "Z.ai GLM-5.1 model via zai-org inference provider on HuggingFace router."
            }
            ModelId::HuggingFaceQwen3CoderNextNovita => {
                "Qwen3-Coder-Next via Novita inference provider on HuggingFace router. Coding-optimized model with reasoning capabilities."
            }
            ModelId::HuggingFaceQwen35397BA17BTogether => {
                "Qwen3.5-397B-A17B via Together inference provider on HuggingFace router. Vision-language model with linear attention and sparse MoE, 1M context window."
            }
            ModelId::HuggingFaceKimiK26Novita => {
                "Kimi K2.6 via Novita inference provider on HuggingFace router."
            }
            ModelId::HuggingFaceStep35Flash => {
                "Step 3.5 Flash flagship model via HuggingFace router (featherless-ai provider). Supports streaming and fast inference."
            }
            ModelId::OpenRouterMinimaxM25 => "MiniMax-M2.5 flagship model via OpenRouter",
            ModelId::OpenRouterQwen3CoderNext => {
                "Next-generation Qwen3 coding model optimized for agentic workflows via OpenRouter"
            }
            ModelId::OpenRouterMoonshotaiKimiK26 => {
                "Kimi K2.6 multimodal agentic model for long-horizon coding and design via OpenRouter"
            }
            ModelId::OpenRouterOpenAIGpt55 => "OpenAI GPT-5.5 model accessed through OpenRouter",
            _ => unreachable!(),
        }
    }
}
