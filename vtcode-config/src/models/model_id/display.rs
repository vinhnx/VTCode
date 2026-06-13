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
            ModelId::Gemini35Flash => "Gemini 3.5 Flash",
            // OpenAI models
            ModelId::GPT55 => "GPT-5.5",
            ModelId::GPT54 => "GPT-5.4",
            ModelId::GPT54Pro => "GPT-5.4 Pro",
            ModelId::GPT54Nano => "GPT-5.4 Nano",
            ModelId::GPT54Mini => "GPT-5.4 Mini",
            ModelId::GPT53Codex => "GPT-5.3 Codex",
            ModelId::OpenAIGptOss20b => "GPT-OSS 20B",
            ModelId::OpenAIGptOss120b => "GPT-OSS 120B",
            // Anthropic models
            ModelId::ClaudeOpus48 => "Claude Opus 4.8",
            ModelId::ClaudeSonnet46 => "Claude Sonnet 4.6",
            ModelId::ClaudeHaiku45 => "Claude Haiku 4.5",
            ModelId::CopilotAuto => "GitHub Copilot Auto",
            ModelId::CopilotGPT52Codex => "GitHub Copilot GPT-5.2 Codex",
            ModelId::CopilotGPT51CodexMax => "GitHub Copilot GPT-5.1 Codex Max",
            ModelId::CopilotGPT54 => "GitHub Copilot GPT-5.4",
            ModelId::CopilotGPT54Mini => "GitHub Copilot GPT-5.4 Mini",
            ModelId::CopilotClaudeSonnet46 => "GitHub Copilot Claude Sonnet 4.6",
            // DeepSeek models
            ModelId::DeepSeekV4Pro => "DeepSeek V4 Pro",
            ModelId::DeepSeekV4Flash => "DeepSeek V4 Flash",
            // Mistral models
            ModelId::MistralLarge3 => "Mistral Large 3",
            // MiMo models
            ModelId::MiMoV25Pro => "MiMo V2.5 Pro",
            ModelId::MiMoV25 => "MiMo V2.5",
            // Z.AI models
            ModelId::ZaiGlm51 => "GLM 5.1",
            // Qwen models
            ModelId::QwenDeepSeekV4Flash => "DeepSeek V4 Flash (Qwen)",
            ModelId::QwenDeepSeekV4Pro => "DeepSeek V4 Pro (Qwen)",
            ModelId::QwenGlm51 => "GLM-5.1 (Qwen)",
            // Ollama models
            ModelId::OllamaGptOss20b => "GPT-OSS 20B (local)",
            ModelId::OllamaGptOss20bCloud => "GPT-OSS 20B (cloud)",
            ModelId::OllamaGptOss120bCloud => "GPT-OSS 120B (cloud)",
            ModelId::OllamaDeepseekV4FlashCloud => "DeepSeek V4 Flash (cloud)",
            ModelId::OllamaDeepseekV4ProCloud => "DeepSeek V4 Pro (cloud)",
            ModelId::OllamaGlm51Cloud => "GLM-5.1 (cloud)",
            ModelId::OllamaMinimaxM27Cloud => "MiniMax-M2.7 (cloud)",
            ModelId::OllamaMinimaxM3Cloud => "MiniMax-M3 (cloud)",
            ModelId::OllamaKimiK26Cloud => "Kimi-K2.6 (cloud)",
            ModelId::OllamaKimiK27CodeCloud => "Kimi-K2.7-Code (cloud)",
            ModelId::OllamaNemotron3SuperCloud => "Nemotron-3-Super (cloud)",
            ModelId::OllamaGemma4 => "Gemma 4",
            ModelId::OllamaLagunaXs2 => "Laguna XS.2 (local)",
            ModelId::LlamaCppGemma426bA4b => "Gemma 4 26B A4B (llama.cpp)",
            ModelId::LlamaCppGemma4E4b => "Gemma 4 E4B (llama.cpp)",
            ModelId::LlamaCppGptOss20b => "GPT-OSS 20B (llama.cpp)",
            ModelId::LlamaCppStep35Flash => "Step 3.5 Flash (llama.cpp)",
            // Hugging Face models
            ModelId::HuggingFaceOpenAIGptOss20b => "GPT-OSS 20B (HF)",
            ModelId::HuggingFaceOpenAIGptOss120b => "GPT-OSS 120B (HF)",
            ModelId::HuggingFaceGlm51ZaiOrg => "GLM-5.1 (zai-org)",
            ModelId::HuggingFaceKimiK26Novita => "Kimi K2.6 (Novita)",
            ModelId::HuggingFaceDeepseekV4FlashNovita => "DeepSeek V4 Flash (Novita)",
            ModelId::HuggingFaceDeepseekV4ProTogether => "DeepSeek V4 Pro (Together)",
            ModelId::HuggingFaceStep35Flash => "Step 3.5 Flash (HF)",
            ModelId::HuggingFaceGlm51Deepinfra => "GLM-5.1 (DeepInfra)",
            ModelId::HuggingFaceMinimaxM27Novita => "MiniMax-M2.7 (Novita)",
            ModelId::HuggingFaceMinimaxM3Novita => "MiniMax-M3 (Novita)",
            ModelId::HuggingFaceDeepseekV4ProNovita => "DeepSeek V4 Pro (Novita)",
            ModelId::StepFun37Flash => "Step 3.7 Flash",
            ModelId::EvolinkGpt52 => "GPT-5.2 (Evolink)",
            ModelId::EvolinkGpt55 => "GPT-5.5 (Evolink)",
            ModelId::EvolinkDeepseekV4Pro => "DeepSeek V4 Pro (Evolink)",
            ModelId::EvolinkDeepseekV4Flash => "DeepSeek V4 Flash (Evolink)",
            ModelId::EvolinkDoubaoSeed20Pro => "Doubao Seed 2.0 Pro (Evolink)",
            ModelId::EvolinkGemini31Pro => "Gemini 3.1 Pro (Evolink)",
            ModelId::EvolinkGemini35Flash => "Gemini 3.5 Flash (Evolink)",
            ModelId::EvolinkMinimaxM3 => "MiniMax-M3 (Evolink)",
            ModelId::EvolinkClaudeSonnet46 => "Claude Sonnet 4.6 (Evolink)",
            ModelId::EvolinkClaudeOpus48 => "Claude Opus 4.8 (Evolink)",
            ModelId::EvolinkClaudeHaiku45 => "Claude Haiku 4.5 (Evolink)",
            ModelId::OpenRouterMoonshotaiKimiK26 => "Kimi K2.6 (OpenRouter)",
            ModelId::OpenRouterMoonshotaiKimiK27Code => "Kimi K2.7 Code (OpenRouter)",
            ModelId::OpenRouterZaiGlm51 => "GLM-5.1 (OpenRouter)",
            ModelId::OpenRouterOpenAIGpt55 => "OpenAI GPT-5.5 (OpenRouter)",
            // MiniMax models
            ModelId::MinimaxM3 => "MiniMax-M3",
            ModelId::MinimaxM27 => "MiniMax-M2.7",
            // Poolside models
            ModelId::PoolsideLagunaM1 => "Laguna M.1",
            ModelId::PoolsideLagunaXs2 => "Laguna XS.2",
            // Moonshot models
            ModelId::MoonshotKimiK27Code => "Kimi K2.7 Code (Moonshot)",
            ModelId::MoonshotKimiK26 => "Kimi K2.6 (Moonshot)",
            // OpenCode Zen models
            ModelId::OpenCodeZenGPT54 => "GPT-5.4 (OpenCode Zen)",
            ModelId::OpenCodeZenGPT54Mini => "GPT-5.4 Mini (OpenCode Zen)",
            ModelId::OpenCodeZenClaudeSonnet46 => "Claude Sonnet 4.6 (OpenCode Zen)",
            ModelId::OpenCodeZenGlm51 => "GLM-5.1 (OpenCode Zen)",
            // OpenCode Go models
            ModelId::OpenCodeGoGlm51 => "GLM-5.1 (OpenCode Go)",
            ModelId::OpenCodeGoMinimaxM27 => "MiniMax-M2.7 (OpenCode Go)",
            // OpenRouter models
            model => {
                model
                    .openrouter_metadata()
                    .expect("generated OpenRouter model should have metadata")
                    .display
            }
        }
    }
}
