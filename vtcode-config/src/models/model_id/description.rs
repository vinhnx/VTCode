use std::borrow::Cow;

use super::ModelId;

impl ModelId {
    /// Get a description of the model's characteristics.
    ///
    /// Returns `Cow<'static, str>` because custom user-defined models
    /// carry runtime strings that may not be `'static`.
    pub fn description(&self) -> Cow<'static, str> {
        if let Some(meta) = self.openrouter_metadata() {
            return Cow::Borrowed(meta.description);
        }
        match self {
            // Gemini models
            ModelId::Gemini31ProPreview => Cow::Borrowed(
                "Latest Gemini 3.1 Pro flagship model with improved thinking, efficiency, and factual consistency",
            ),
            ModelId::Gemini31ProPreviewCustomTools => Cow::Borrowed(
                "Gemini 3.1 Pro variant optimized for agentic workflows using custom tools and bash",
            ),
            ModelId::Gemini35Flash => Cow::Borrowed(
                "High-efficiency frontier model for fast inference with excellent quality-to-speed balance",
            ),
            // OpenAI models
            ModelId::GPT55 => Cow::Borrowed(
                "Next-gen OpenAI model with frontier reasoning and long context (2026-04-23 dated release)",
            ),
            ModelId::GPT54 => Cow::Borrowed(
                "Mainline frontier GPT model for general-purpose work, coding, long context, and multi-step agents",
            ),
            ModelId::GPT54Pro => Cow::Borrowed(
                "Higher-compute GPT-5.4 variant for tougher problems with deeper reasoning",
            ),
            ModelId::GPT54Nano => {
                Cow::Borrowed("Lightweight GPT-5.4 variant optimized for speed and cost-efficiency")
            }
            ModelId::GPT54Mini => Cow::Borrowed(
                "Compact GPT-5.4 variant for cost-effective tasks with reduced reasoning overhead",
            ),
            ModelId::GPT53Codex => Cow::Borrowed(
                "GPT-5.3 variant optimized for agentic coding tasks with reasoning effort support (low, medium, high, xhigh)",
            ),
            ModelId::OpenAIGptOss20b => Cow::Borrowed(
                "OpenAI's open-source 20B parameter GPT-OSS model using harmony tokenization",
            ),
            ModelId::OpenAIGptOss120b => Cow::Borrowed(
                "OpenAI's open-source 120B parameter GPT-OSS model using harmony tokenization",
            ),
            // Anthropic models
            ModelId::ClaudeSonnet5 => Cow::Borrowed(
                "Anthropic's best combination of speed and intelligence with adaptive thinking on by default, 1M context, and new tokenizer",
            ),
            ModelId::ClaudeFable5 => Cow::Borrowed(
                "Anthropic's most capable widely released model, for the most demanding reasoning and long-horizon agentic work",
            ),
            ModelId::ClaudeMythos5 => Cow::Borrowed(
                "Shares Claude Fable 5's capabilities without safety classifiers. Limited availability through Project Glasswing",
            ),
            ModelId::ClaudeOpus48 => Cow::Borrowed(
                "Anthropic's most capable model for complex reasoning, long-horizon agentic coding, and high-autonomy work",
            ),
            ModelId::ClaudeSonnet46 => Cow::Borrowed(
                "Balanced flagship model for coding with budgeted thinking in VT Code's current Anthropic rollout",
            ),
            ModelId::ClaudeHaiku45 => Cow::Borrowed(
                "Latest efficient Anthropic model optimized for low-latency agent workflows",
            ),
            ModelId::CopilotAuto => Cow::Borrowed(
                "GitHub Copilot preview provider with automatic model selection via the official Copilot CLI",
            ),
            ModelId::CopilotGPT52Codex => Cow::Borrowed(
                "GitHub Copilot GPT-5.2 Codex option for agentic software engineering workflows",
            ),
            ModelId::CopilotGPT51CodexMax => Cow::Borrowed(
                "GitHub Copilot GPT-5.1 Codex Max option for longer-running engineering tasks",
            ),
            ModelId::CopilotGPT54 => Cow::Borrowed(
                "GitHub Copilot GPT-5.4 option for complex professional work and long context",
            ),
            ModelId::CopilotGPT54Mini => {
                Cow::Borrowed("GitHub Copilot GPT-5.4 Mini option for faster, lighter-weight tasks")
            }
            ModelId::CopilotClaudeSonnet46 => Cow::Borrowed(
                "GitHub Copilot Claude Sonnet 4.6 option for balanced coding and reasoning work",
            ),
            // DeepSeek models
            ModelId::DeepSeekV4Pro => Cow::Borrowed(
                "High-performance reasoning model with advanced thinking capabilities",
            ),
            ModelId::DeepSeekV4Flash => {
                Cow::Borrowed("Fast inference model for cost-effective reasoning")
            }
            // Mistral models
            ModelId::MistralLarge3 => Cow::Borrowed(
                "State-of-the-art open-weight general-purpose multimodal model with Mixture-of-Experts architecture",
            ),
            // MiMo models
            ModelId::MiMoV25Pro => Cow::Borrowed(
                "Xiaomi's flagship reasoning model with advanced capabilities (1M context)",
            ),
            ModelId::MiMoV25 => Cow::Borrowed(
                "Xiaomi's omni-modal model with full-modal understanding and 1M context",
            ),
            // Z.AI models
            ModelId::ZaiGlm52 => Cow::Borrowed(
                "Z.ai flagship model for long-horizon tasks with truly usable 1M-token context",
            ),
            ModelId::ZaiGlm51 => Cow::Borrowed(
                "Z.ai next-gen GLM-5.1 foundation model with improved reasoning and agent capabilities",
            ),
            // Ollama models
            ModelId::OllamaGptOss20b => Cow::Borrowed(
                "Local GPT-OSS 20B deployment served via Ollama with no external API dependency",
            ),
            ModelId::OllamaGptOss20bCloud => Cow::Borrowed(
                "Cloud-hosted GPT-OSS 20B accessed through Ollama Cloud for efficient reasoning tasks",
            ),
            ModelId::OllamaGptOss120bCloud => Cow::Borrowed(
                "Cloud-hosted GPT-OSS 120B accessed through Ollama Cloud for larger reasoning tasks",
            ),
            ModelId::OllamaDeepseekV4FlashCloud => Cow::Borrowed(
                "DeepSeek V4 Flash cloud deployment via Ollama with fast inference and efficient reasoning",
            ),
            ModelId::OllamaDeepseekV4ProCloud => Cow::Borrowed(
                "DeepSeek V4 Pro cloud deployment via Ollama with advanced thinking and strong reasoning",
            ),
            ModelId::OllamaGlm51Cloud => {
                Cow::Borrowed("Cloud-hosted GLM-5.1 model served via Ollama Cloud")
            }
            ModelId::OllamaGlm52Cloud => Cow::Borrowed(
                "Cloud-hosted GLM-5.2 flagship model for long-horizon tasks with 1M context via Ollama Cloud",
            ),
            ModelId::OllamaMinimaxM3Cloud => {
                Cow::Borrowed("Cloud-hosted MiniMax-M3 model served via Ollama Cloud")
            }
            ModelId::OllamaKimiK26Cloud => {
                Cow::Borrowed("Cloud-hosted Kimi K2.6 model served via Ollama Cloud")
            }
            ModelId::OllamaKimiK27CodeCloud => {
                Cow::Borrowed("Cloud-hosted Kimi K2.7 Code model served via Ollama Cloud")
            }
            ModelId::OllamaGemma4 => Cow::Borrowed(
                "Google Gemma 4 model designed for frontier-level reasoning, agentic workflows, coding, and multimodal understanding (128K context).",
            ),
            ModelId::OllamaLagunaXs2 => Cow::Borrowed(
                "Poolside's 33B MoE model with 3B activated parameters, optimized for agentic coding with sliding window attention and native reasoning support (128K context)",
            ),
            ModelId::LlamaCppGemma426bA4b => Cow::Borrowed(
                "Gemma 4 desktop MoE model served through llama.cpp with strong reasoning and fast local inference",
            ),
            ModelId::LlamaCppGemma4E4b => Cow::Borrowed(
                "Tiny-footprint Gemma 4 local model served through llama.cpp for phones and low-end laptops",
            ),
            ModelId::LlamaCppGptOss20b => Cow::Borrowed(
                "OpenAI's open-weight GPT-OSS 20B model served locally through llama.cpp",
            ),
            ModelId::LlamaCppStep35Flash => Cow::Borrowed(
                "StepFun's efficient reasoning model served locally through llama.cpp",
            ),
            ModelId::OllamaMinimaxM27Cloud => Cow::Borrowed(
                "Cloud-hosted MiniMax-M2.7 model accessed through Ollama Cloud for reasoning tasks",
            ),
            ModelId::MinimaxM3 => {
                Cow::Borrowed("Frontier multimodal coding model with 1M context window")
            }
            ModelId::MinimaxM27 => Cow::Borrowed(
                "Beginning the journey of recursive self-improvement with 204.8K context and strong reasoning/coding performance",
            ),
            // Poolside models
            ModelId::PoolsideLagunaM1 => Cow::Borrowed(
                "Poolside's flagship MoE coding agent model with 128K context, optimized for multi-step agentic tasks, tool use, and validation",
            ),
            ModelId::PoolsideLagunaXs2 => Cow::Borrowed(
                "Poolside's efficient MoE coding agent model with 128K context, optimized for fast agentic coding with lower resource requirements",
            ),
            ModelId::MoonshotKimiK27Code => Cow::Borrowed(
                "Kimi K2.7 Code - Moonshot.ai's most capable coding model with long-horizon coding breakthrough, 256K context, and strong reasoning",
            ),
            ModelId::MoonshotKimiK26 => Cow::Borrowed(
                "Kimi K2.6 - Moonshot.ai's 1T MoE flagship with 32B active parameters, MLA attention, and MoonViT vision",
            ),
            ModelId::OpenCodeZenGPT54 => Cow::Borrowed(
                "OpenCode Zen flagship GPT-5.4 route using OpenCode's curated pay-as-you-go gateway",
            ),
            ModelId::OpenCodeZenGPT54Mini => Cow::Borrowed(
                "Lower-cost OpenCode Zen GPT-5.4 Mini option for faster and cheaper tasks",
            ),
            ModelId::OpenCodeZenClaudeSonnet46 => Cow::Borrowed(
                "Claude Sonnet 4.6 served through OpenCode Zen's curated Anthropic endpoint",
            ),
            ModelId::OpenCodeZenGlm51 => Cow::Borrowed(
                "GLM-5.1 served through OpenCode Zen for lower-cost reasoning and coding work",
            ),
            ModelId::OpenCodeGoGlm52 => Cow::Borrowed(
                "GLM-5.2 included with the OpenCode Go subscription for flagship open-model coding",
            ),
            ModelId::OpenCodeGoGlm51 => Cow::Borrowed(
                "GLM-5.1 included with the OpenCode Go subscription for open-model coding workflows",
            ),
            ModelId::OpenCodeGoKimiK27Code => Cow::Borrowed(
                "Kimi K2.7 Code included with the OpenCode Go subscription for long-horizon coding",
            ),
            ModelId::OpenCodeGoKimiK26 => Cow::Borrowed(
                "Kimi K2.6 included with the OpenCode Go subscription for general agentic coding",
            ),
            ModelId::OpenCodeGoMimoV25 => Cow::Borrowed(
                "MiMo-V2.5 included with the OpenCode Go subscription for high-volume coding workloads",
            ),
            ModelId::OpenCodeGoMimoV25Pro => Cow::Borrowed(
                "MiMo-V2.5-Pro included with the OpenCode Go subscription for complex agentic tasks",
            ),
            ModelId::OpenCodeGoMinimaxM3 => Cow::Borrowed(
                "MiniMax-M3 included with the OpenCode Go subscription for frontier agentic coding",
            ),
            ModelId::OpenCodeGoMinimaxM27 => Cow::Borrowed(
                "MiniMax-M2.7 included with the OpenCode Go subscription for stronger agentic coding",
            ),
            ModelId::OpenCodeGoQwen37Max => Cow::Borrowed(
                "Qwen3.7 Max included with the OpenCode Go subscription for the highest capability tier",
            ),
            ModelId::OpenCodeGoQwen37Plus => Cow::Borrowed(
                "Qwen3.7 Plus included with the OpenCode Go subscription as a balanced coding model",
            ),
            ModelId::OpenCodeGoQwen36Plus => Cow::Borrowed(
                "Qwen3.6 Plus included with the OpenCode Go subscription for cost-effective coding",
            ),
            ModelId::OpenCodeGoDeepseekV4Pro => Cow::Borrowed(
                "DeepSeek V4 Pro included with the OpenCode Go subscription for high-quality reasoning",
            ),
            ModelId::OpenCodeGoDeepseekV4Flash => Cow::Borrowed(
                "DeepSeek V4 Flash included with the OpenCode Go subscription for fast, low-cost coding",
            ),
            ModelId::HuggingFaceOpenAIGptOss20b => {
                Cow::Borrowed("OpenAI GPT-OSS 20B via Hugging Face router")
            }
            ModelId::HuggingFaceOpenAIGptOss120b => {
                Cow::Borrowed("OpenAI GPT-OSS 120B via Hugging Face router")
            }
            ModelId::HuggingFaceGlm51ZaiOrg => Cow::Borrowed(
                "Z.ai GLM-5.1 model via zai-org inference provider on HuggingFace router.",
            ),
            ModelId::HuggingFaceGlm52Novita => Cow::Borrowed(
                "Z.ai GLM-5.2 flagship model via Novita inference provider on HuggingFace router. 1M context for long-horizon tasks.",
            ),
            ModelId::HuggingFaceKimiK26Novita => {
                Cow::Borrowed("Kimi K2.6 via Novita inference provider on HuggingFace router.")
            }
            ModelId::HuggingFaceDeepseekV4FlashNovita => Cow::Borrowed(
                "DeepSeek V4 Flash via Novita inference provider on HuggingFace router. Fast inference model for cost-effective reasoning (1M context, 158B params).",
            ),
            ModelId::HuggingFaceDeepseekV4ProTogether => Cow::Borrowed(
                "DeepSeek V4 Pro via Together inference provider on HuggingFace router. High-performance reasoning model with advanced thinking capabilities (1M context, 1.6T params).",
            ),
            ModelId::HuggingFaceStep35Flash => Cow::Borrowed(
                "Step 3.5 Flash flagship model via HuggingFace router (featherless-ai provider). Supports streaming and fast inference.",
            ),
            ModelId::HuggingFaceGlm51Deepinfra => Cow::Borrowed(
                "Z.ai GLM-5.1 model via DeepInfra inference provider on HuggingFace router.",
            ),
            ModelId::HuggingFaceMinimaxM27Novita => Cow::Borrowed(
                "MiniMax-M2.7 model via Novita inference provider on HuggingFace router. Recursive self-improvement with enhanced reasoning.",
            ),
            ModelId::HuggingFaceMinimaxM3Novita => Cow::Borrowed(
                "MiniMax-M3 model via Novita inference provider on HuggingFace router. Frontier multimodal coding model with 1M context window.",
            ),
            ModelId::HuggingFaceDeepseekV4ProNovita => Cow::Borrowed(
                "DeepSeek V4 Pro via Novita inference provider on HuggingFace router. High-performance reasoning model with advanced thinking capabilities (1M context, 1.6T params).",
            ),
            ModelId::StepFun37Flash => Cow::Borrowed(
                "StepFun's flagship multimodal reasoning model with 256K context, native image/video input, and tool calling.",
            ),
            // Evolink gateway models
            ModelId::EvolinkGpt52 => Cow::Borrowed(
                "GPT-5.2 served through the Evolink OpenAI-compatible gateway (direct.evolink.ai).",
            ),
            ModelId::EvolinkGpt55 => Cow::Borrowed(
                "GPT-5.5 flagship model served through the Evolink OpenAI-compatible gateway (direct.evolink.ai).",
            ),
            ModelId::EvolinkDeepseekV4Pro => Cow::Borrowed(
                "DeepSeek V4 Pro reasoning model served through the Evolink gateway (direct.evolink.ai).",
            ),
            ModelId::EvolinkDeepseekV4Flash => Cow::Borrowed(
                "DeepSeek V4 Flash fast inference model served through the Evolink gateway (direct.evolink.ai).",
            ),
            ModelId::EvolinkDoubaoSeed20Pro => Cow::Borrowed(
                "Doubao Seed 2.0 Pro served through the Evolink gateway (direct.evolink.ai).",
            ),
            ModelId::EvolinkGemini31Pro => Cow::Borrowed(
                "Gemini 3.1 Pro served through the Evolink gateway via OpenAI SDK format (direct.evolink.ai).",
            ),
            ModelId::EvolinkGemini35Flash => Cow::Borrowed(
                "Gemini 3.5 Flash served through the Evolink gateway via OpenAI SDK format (direct.evolink.ai).",
            ),
            ModelId::EvolinkMinimaxM3 => Cow::Borrowed(
                "MiniMax-M3 frontier multimodal model served through the Evolink gateway (direct.evolink.ai).",
            ),
            ModelId::EvolinkClaudeSonnet46 => Cow::Borrowed(
                "Claude Sonnet 4.6 served through the Evolink gateway via Anthropic Messages API.",
            ),
            ModelId::EvolinkClaudeOpus48 => Cow::Borrowed(
                "Claude Opus 4.8 served through the Evolink gateway via Anthropic Messages API.",
            ),
            ModelId::EvolinkClaudeHaiku45 => Cow::Borrowed(
                "Claude Haiku 4.5 served through the Evolink gateway via Anthropic Messages API.",
            ),
            // Qwen models
            ModelId::QwenDeepSeekV4Flash => Cow::Borrowed(
                "DeepSeek V4 Flash fast inference model served through Qwen Cloud API (1M context)",
            ),
            ModelId::QwenDeepSeekV4Pro => Cow::Borrowed(
                "DeepSeek V4 Pro high-performance reasoning model served through Qwen Cloud API (1M context)",
            ),
            ModelId::QwenGlm51 => Cow::Borrowed(
                "Z.AI GLM-5.1 next-gen foundation model served through Qwen Cloud API",
            ),
            ModelId::OpenRouterMoonshotaiKimiK26 => Cow::Borrowed(
                "Kimi K2.6 multimodal agentic model for long-horizon coding and design via OpenRouter",
            ),
            ModelId::OpenRouterMoonshotaiKimiK27Code => Cow::Borrowed(
                "Kimi K2.7 Code most capable coding model with long-horizon coding breakthrough via OpenRouter",
            ),
            ModelId::OpenRouterZaiGlm51 => {
                Cow::Borrowed("Z.AI GLM-5.1 next-gen foundation model via OpenRouter")
            }
            ModelId::OpenRouterZaiGlm52 => Cow::Borrowed(
                "Z.AI GLM-5.2 flagship model for long-horizon tasks with 1M context via OpenRouter",
            ),
            ModelId::OpenRouterOpenAIGpt55 => {
                Cow::Borrowed("OpenAI GPT-5.5 model accessed through OpenRouter")
            }
            ModelId::Custom(_, _) => Cow::Borrowed("User-defined model"),
            model => Cow::Borrowed(
                model
                    .openrouter_metadata()
                    .expect("generated OpenRouter model should have metadata")
                    .description,
            ),
        }
    }
}
