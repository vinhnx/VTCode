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
            ModelId::Gemini35Flash => {
                "High-efficiency frontier model for fast inference with excellent quality-to-speed balance"
            }
            // OpenAI models
            ModelId::GPT55 => {
                "Next-gen OpenAI model with frontier reasoning and long context (2026-04-23 dated release)"
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
            ModelId::OpenAIGptOss20b => {
                "OpenAI's open-source 20B parameter GPT-OSS model using harmony tokenization"
            }
            ModelId::OpenAIGptOss120b => {
                "OpenAI's open-source 120B parameter GPT-OSS model using harmony tokenization"
            }
            // Anthropic models
            ModelId::ClaudeOpus48 => {
                "Anthropic's most capable model for complex reasoning, long-horizon agentic coding, and high-autonomy work"
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
                "High-performance reasoning model with advanced thinking capabilities"
            }
            ModelId::DeepSeekV4Flash => "Fast inference model for cost-effective reasoning",
            // Mistral models
            ModelId::MistralLarge3 => {
                "State-of-the-art open-weight general-purpose multimodal model with Mixture-of-Experts architecture"
            }
            // MiMo models
            ModelId::MiMoV25Pro => {
                "Xiaomi's flagship reasoning model with advanced capabilities (1M context)"
            }
            ModelId::MiMoV25 => {
                "Xiaomi's omni-modal model with full-modal understanding and 1M context"
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
            ModelId::OllamaMinimaxM3Cloud => {
                "Cloud-hosted MiniMax-M3 model served via Ollama Cloud"
            }
            ModelId::OllamaGemini3FlashPreviewCloud => {
                "Gemini 3 Flash offers frontier intelligence built for speed at a fraction of the cost."
            }
            ModelId::OllamaKimiK26Cloud => "Cloud-hosted Kimi K2.6 model served via Ollama Cloud",
            ModelId::OllamaNemotron3SuperCloud => {
                "NVIDIA Nemotron\u{2122} is a family of open models delivering leading efficiency and accuracy for building specialized AI agents. Nemotron-3-Super (120B) is optimized for collaborative agents and high-volume workloads."
            }
            ModelId::OllamaNemotron3UltraCloud => {
                "NVIDIA Nemotron 3 Ultra (550B) is built for high-throughput reasoning and long-running agent workflows with 256K context via Ollama Cloud."
            }
            ModelId::OllamaGemma4 => {
                "Google Gemma 4 model designed for frontier-level reasoning, agentic workflows, coding, and multimodal understanding (128K context)."
            }
            ModelId::OllamaLagunaXs2 => {
                "Poolside's 33B MoE model with 3B activated parameters, optimized for agentic coding with sliding window attention and native reasoning support (128K context)"
            }
            ModelId::LlamaCppQwen3627b => {
                "Dense Qwen 3.6 local model served through llama.cpp with strong reasoning and coding ability"
            }
            ModelId::LlamaCppQwen3635bA3b => {
                "Qwen 3.6 MoE local model served through llama.cpp for higher reasoning quality with lower active compute"
            }
            ModelId::LlamaCppGemma426bA4b => {
                "Gemma 4 desktop MoE model served through llama.cpp with strong reasoning and fast local inference"
            }
            ModelId::LlamaCppGemma4E4b => {
                "Tiny-footprint Gemma 4 local model served through llama.cpp for phones and low-end laptops"
            }
            ModelId::LlamaCppGptOss20b => {
                "OpenAI's open-weight GPT-OSS 20B model served locally through llama.cpp"
            }
            ModelId::LlamaCppStep35Flash => {
                "StepFun's efficient reasoning model served locally through llama.cpp"
            }
            ModelId::OllamaMinimaxM2Cloud => {
                "Cloud-hosted MiniMax-M2 model accessed through Ollama Cloud for reasoning tasks"
            }
            ModelId::OllamaMinimaxM27Cloud => {
                "Cloud-hosted MiniMax-M2.7 model accessed through Ollama Cloud for reasoning tasks"
            }
            ModelId::MinimaxM3 => "Frontier multimodal coding model with 1M context window",
            ModelId::MinimaxM27 => {
                "Beginning the journey of recursive self-improvement with 204.8K context and strong reasoning/coding performance"
            }
            ModelId::MinimaxM25 => {
                "Latest MiniMax-M2.5 model with further improvements in reasoning and coding"
            }
            // Poolside models
            ModelId::PoolsideLagunaM1 => {
                "Poolside's flagship MoE coding agent model with 128K context, optimized for multi-step agentic tasks, tool use, and validation"
            }
            ModelId::PoolsideLagunaXs2 => {
                "Poolside's efficient MoE coding agent model with 128K context, optimized for fast agentic coding with lower resource requirements"
            }
            ModelId::MoonshotKimiK26 => {
                "Kimi K2.6 - Moonshot.ai's latest 1T MoE flagship with 32B active parameters, MLA attention, and MoonViT vision"
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
            ModelId::OpenCodeGoGlm51 => {
                "GLM-5.1 included with the OpenCode Go subscription for open-model coding workflows"
            }
            ModelId::OpenCodeGoMinimaxM25 => {
                "MiniMax-M2.5 included with the OpenCode Go subscription"
            }
            ModelId::OpenCodeGoMinimaxM27 => {
                "MiniMax-M2.7 included with the OpenCode Go subscription for stronger agentic coding"
            }
            ModelId::HuggingFaceOpenAIGptOss20b => "OpenAI GPT-OSS 20B via Hugging Face router",
            ModelId::HuggingFaceOpenAIGptOss120b => "OpenAI GPT-OSS 120B via Hugging Face router",
            ModelId::HuggingFaceMinimaxM25Novita => {
                "MiniMax-M2.5 model via Novita inference provider on HuggingFace router. Enhanced reasoning capabilities."
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
            ModelId::HuggingFaceDeepseekV4FlashNovita => {
                "DeepSeek V4 Flash via Novita inference provider on HuggingFace router. Fast inference model for cost-effective reasoning (1M context, 158B params)."
            }
            ModelId::HuggingFaceDeepseekV4ProTogether => {
                "DeepSeek V4 Pro via Together inference provider on HuggingFace router. High-performance reasoning model with advanced thinking capabilities (1M context, 1.6T params)."
            }
            ModelId::HuggingFaceStep35Flash => {
                "Step 3.5 Flash flagship model via HuggingFace router (featherless-ai provider). Supports streaming and fast inference."
            }
            ModelId::HuggingFaceGlm51Deepinfra => {
                "Z.ai GLM-5.1 model via DeepInfra inference provider on HuggingFace router."
            }
            ModelId::HuggingFaceMinimaxM27Novita => {
                "MiniMax-M2.7 model via Novita inference provider on HuggingFace router. Recursive self-improvement with enhanced reasoning."
            }
            ModelId::HuggingFaceDeepseekV4ProNovita => {
                "DeepSeek V4 Pro via Novita inference provider on HuggingFace router. High-performance reasoning model with advanced thinking capabilities (1M context, 1.6T params)."
            }
            ModelId::HuggingFaceNvidiaNemotron3Ultra550bA55bNvfp4Together => {
                "NVIDIA Nemotron 3 Ultra 550B-A55B-NVFP4 via Together inference provider on HuggingFace router."
            }
            ModelId::StepFun37Flash => {
                "StepFun's flagship multimodal reasoning model with 256K context, native image/video input, and tool calling."
            }
            // Evolink gateway models
            ModelId::EvolinkGpt52 => {
                "GPT-5.2 served through the Evolink OpenAI-compatible gateway (direct.evolink.ai)."
            }
            ModelId::EvolinkGpt55 => {
                "GPT-5.5 flagship model served through the Evolink OpenAI-compatible gateway (direct.evolink.ai)."
            }
            ModelId::EvolinkDeepseekV4Pro => {
                "DeepSeek V4 Pro reasoning model served through the Evolink gateway (direct.evolink.ai)."
            }
            ModelId::EvolinkDeepseekV4Flash => {
                "DeepSeek V4 Flash fast inference model served through the Evolink gateway (direct.evolink.ai)."
            }
            ModelId::EvolinkDoubaoSeed20Pro => {
                "Doubao Seed 2.0 Pro served through the Evolink gateway (direct.evolink.ai)."
            }
            ModelId::EvolinkGemini31Pro => {
                "Gemini 3.1 Pro served through the Evolink gateway via OpenAI SDK format (direct.evolink.ai)."
            }
            ModelId::EvolinkGemini35Flash => {
                "Gemini 3.5 Flash served through the Evolink gateway via OpenAI SDK format (direct.evolink.ai)."
            }
            ModelId::EvolinkMinimaxM3 => {
                "MiniMax-M3 frontier multimodal model served through the Evolink gateway (direct.evolink.ai)."
            }
            ModelId::EvolinkClaudeSonnet46 => {
                "Claude Sonnet 4.6 served through the Evolink gateway via Anthropic Messages API."
            }
            ModelId::EvolinkClaudeOpus48 => {
                "Claude Opus 4.8 served through the Evolink gateway via Anthropic Messages API."
            }
            ModelId::EvolinkClaudeHaiku45 => {
                "Claude Haiku 4.5 served through the Evolink gateway via Anthropic Messages API."
            }
            // Qwen models
            ModelId::Qwen37Max => {
                "Alibaba Cloud's flagship reasoning model with 131K context, advanced thinking, and strong tool-use capabilities"
            }
            ModelId::Qwen36Flash => {
                "Alibaba Cloud's fast inference model with 1M context window, optimized for speed and cost-efficiency"
            }
            ModelId::Qwen36Plus => {
                "Alibaba Cloud's balanced model with 131K context, strong reasoning and coding performance"
            }
            ModelId::QwenDeepSeekV4Flash => {
                "DeepSeek V4 Flash fast inference model served through Qwen Cloud API (1M context)"
            }
            ModelId::QwenDeepSeekV4Pro => {
                "DeepSeek V4 Pro high-performance reasoning model served through Qwen Cloud API (1M context)"
            }
            ModelId::QwenGlm51 => {
                "Z.AI GLM-5.1 next-gen foundation model served through Qwen Cloud API"
            }
            ModelId::OpenRouterMinimaxM25 => "MiniMax-M2.5 flagship model via OpenRouter",
            ModelId::OpenRouterQwen3CoderNext => {
                "Next-generation Qwen3 coding model optimized for agentic workflows via OpenRouter"
            }
            ModelId::OpenRouterMoonshotaiKimiK26 => {
                "Kimi K2.6 multimodal agentic model for long-horizon coding and design via OpenRouter"
            }
            ModelId::OpenRouterZaiGlm51 => "Z.AI GLM-5.1 next-gen foundation model via OpenRouter",
            ModelId::OpenRouterOpenAIGpt55 => "OpenAI GPT-5.5 model accessed through OpenRouter",
            _ => unreachable!(),
        }
    }
}
