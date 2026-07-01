use std::borrow::Cow;

use super::ModelId;

impl ModelId {
    /// Get the display name for the model (human-readable).
    ///
    /// Returns `Cow<'static, str>` because custom user-defined models
    /// carry runtime strings that may not be `'static`.
    pub fn display_name(&self) -> Cow<'static, str> {
        if let Some(meta) = self.openrouter_metadata() {
            return Cow::Borrowed(meta.display);
        }
        match self {
            // Gemini models
            ModelId::Gemini31ProPreview => Cow::Borrowed("Gemini 3.1 Pro Preview"),
            ModelId::Gemini31ProPreviewCustomTools => {
                Cow::Borrowed("Gemini 3.1 Pro Preview (Custom Tools)")
            }
            ModelId::Gemini35Flash => Cow::Borrowed("Gemini 3.5 Flash"),
            // OpenAI models
            ModelId::GPT55 => Cow::Borrowed("GPT-5.5"),
            ModelId::GPT54 => Cow::Borrowed("GPT-5.4"),
            ModelId::GPT54Pro => Cow::Borrowed("GPT-5.4 Pro"),
            ModelId::GPT54Nano => Cow::Borrowed("GPT-5.4 Nano"),
            ModelId::GPT54Mini => Cow::Borrowed("GPT-5.4 Mini"),
            ModelId::GPT53Codex => Cow::Borrowed("GPT-5.3 Codex"),
            ModelId::OpenAIGptOss20b => Cow::Borrowed("GPT-OSS 20B"),
            ModelId::OpenAIGptOss120b => Cow::Borrowed("GPT-OSS 120B"),
            // Anthropic models
            ModelId::ClaudeSonnet5 => Cow::Borrowed("Claude Sonnet 5"),
            ModelId::ClaudeFable5 => Cow::Borrowed("Claude Fable 5"),
            ModelId::ClaudeMythos5 => Cow::Borrowed("Claude Mythos 5"),
            ModelId::ClaudeOpus48 => Cow::Borrowed("Claude Opus 4.8"),
            ModelId::ClaudeSonnet46 => Cow::Borrowed("Claude Sonnet 4.6"),
            ModelId::ClaudeHaiku45 => Cow::Borrowed("Claude Haiku 4.5"),
            ModelId::CopilotAuto => Cow::Borrowed("GitHub Copilot Auto"),
            ModelId::CopilotGPT52Codex => Cow::Borrowed("GitHub Copilot GPT-5.2 Codex"),
            ModelId::CopilotGPT51CodexMax => Cow::Borrowed("GitHub Copilot GPT-5.1 Codex Max"),
            ModelId::CopilotGPT54 => Cow::Borrowed("GitHub Copilot GPT-5.4"),
            ModelId::CopilotGPT54Mini => Cow::Borrowed("GitHub Copilot GPT-5.4 Mini"),
            ModelId::CopilotClaudeSonnet46 => Cow::Borrowed("GitHub Copilot Claude Sonnet 4.6"),
            // DeepSeek models
            ModelId::DeepSeekV4Pro => Cow::Borrowed("DeepSeek V4 Pro"),
            ModelId::DeepSeekV4Flash => Cow::Borrowed("DeepSeek V4 Flash"),
            // Mistral models
            ModelId::MistralLarge3 => Cow::Borrowed("Mistral Large 3"),
            // MiMo models
            ModelId::MiMoV25Pro => Cow::Borrowed("MiMo V2.5 Pro"),
            ModelId::MiMoV25 => Cow::Borrowed("MiMo V2.5"),
            // Z.AI models
            ModelId::ZaiGlm52 => Cow::Borrowed("GLM 5.2"),
            ModelId::ZaiGlm51 => Cow::Borrowed("GLM 5.1"),
            // Qwen models
            ModelId::QwenDeepSeekV4Flash => Cow::Borrowed("DeepSeek V4 Flash (Qwen)"),
            ModelId::QwenDeepSeekV4Pro => Cow::Borrowed("DeepSeek V4 Pro (Qwen)"),
            ModelId::QwenGlm51 => Cow::Borrowed("GLM-5.1 (Qwen)"),
            // Ollama models
            ModelId::OllamaGptOss20b => Cow::Borrowed("GPT-OSS 20B (local)"),
            ModelId::OllamaGptOss20bCloud => Cow::Borrowed("GPT-OSS 20B (cloud)"),
            ModelId::OllamaGptOss120bCloud => Cow::Borrowed("GPT-OSS 120B (cloud)"),
            ModelId::OllamaDeepseekV4FlashCloud => Cow::Borrowed("DeepSeek V4 Flash (cloud)"),
            ModelId::OllamaDeepseekV4ProCloud => Cow::Borrowed("DeepSeek V4 Pro (cloud)"),
            ModelId::OllamaGlm51Cloud => Cow::Borrowed("GLM-5.1 (cloud)"),
            ModelId::OllamaGlm52Cloud => Cow::Borrowed("GLM-5.2 (cloud)"),
            ModelId::OllamaMinimaxM27Cloud => Cow::Borrowed("MiniMax-M2.7 (cloud)"),
            ModelId::OllamaMinimaxM3Cloud => Cow::Borrowed("MiniMax-M3 (cloud)"),
            ModelId::OllamaKimiK26Cloud => Cow::Borrowed("Kimi-K2.6 (cloud)"),
            ModelId::OllamaKimiK27CodeCloud => Cow::Borrowed("Kimi-K2.7-Code (cloud)"),
            ModelId::OllamaGemma4 => Cow::Borrowed("Gemma 4"),
            ModelId::OllamaLagunaXs2 => Cow::Borrowed("Laguna XS.2 (local)"),
            ModelId::LlamaCppGemma426bA4b => Cow::Borrowed("Gemma 4 26B A4B (llama.cpp)"),
            ModelId::LlamaCppGemma4E4b => Cow::Borrowed("Gemma 4 E4B (llama.cpp)"),
            ModelId::LlamaCppGptOss20b => Cow::Borrowed("GPT-OSS 20B (llama.cpp)"),
            ModelId::LlamaCppStep35Flash => Cow::Borrowed("Step 3.5 Flash (llama.cpp)"),
            // Hugging Face models
            ModelId::HuggingFaceOpenAIGptOss20b => Cow::Borrowed("GPT-OSS 20B (HF)"),
            ModelId::HuggingFaceOpenAIGptOss120b => Cow::Borrowed("GPT-OSS 120B (HF)"),
            ModelId::HuggingFaceGlm51ZaiOrg => Cow::Borrowed("GLM-5.1 (zai-org)"),
            ModelId::HuggingFaceGlm52Novita => Cow::Borrowed("GLM-5.2 (Novita)"),
            ModelId::HuggingFaceKimiK26Novita => Cow::Borrowed("Kimi K2.6 (Novita)"),
            ModelId::HuggingFaceDeepseekV4FlashNovita => {
                Cow::Borrowed("DeepSeek V4 Flash (Novita)")
            }
            ModelId::HuggingFaceDeepseekV4ProTogether => {
                Cow::Borrowed("DeepSeek V4 Pro (Together)")
            }
            ModelId::HuggingFaceStep35Flash => Cow::Borrowed("Step 3.5 Flash (HF)"),
            ModelId::HuggingFaceGlm51Deepinfra => Cow::Borrowed("GLM-5.1 (DeepInfra)"),
            ModelId::HuggingFaceMinimaxM27Novita => Cow::Borrowed("MiniMax-M2.7 (Novita)"),
            ModelId::HuggingFaceMinimaxM3Novita => Cow::Borrowed("MiniMax-M3 (Novita)"),
            ModelId::HuggingFaceDeepseekV4ProNovita => Cow::Borrowed("DeepSeek V4 Pro (Novita)"),
            ModelId::StepFun37Flash => Cow::Borrowed("Step 3.7 Flash"),
            ModelId::EvolinkGpt52 => Cow::Borrowed("GPT-5.2 (Evolink)"),
            ModelId::EvolinkGpt55 => Cow::Borrowed("GPT-5.5 (Evolink)"),
            ModelId::EvolinkDeepseekV4Pro => Cow::Borrowed("DeepSeek V4 Pro (Evolink)"),
            ModelId::EvolinkDeepseekV4Flash => Cow::Borrowed("DeepSeek V4 Flash (Evolink)"),
            ModelId::EvolinkDoubaoSeed20Pro => Cow::Borrowed("Doubao Seed 2.0 Pro (Evolink)"),
            ModelId::EvolinkGemini31Pro => Cow::Borrowed("Gemini 3.1 Pro (Evolink)"),
            ModelId::EvolinkGemini35Flash => Cow::Borrowed("Gemini 3.5 Flash (Evolink)"),
            ModelId::EvolinkMinimaxM3 => Cow::Borrowed("MiniMax-M3 (Evolink)"),
            ModelId::EvolinkClaudeSonnet46 => Cow::Borrowed("Claude Sonnet 4.6 (Evolink)"),
            ModelId::EvolinkClaudeOpus48 => Cow::Borrowed("Claude Opus 4.8 (Evolink)"),
            ModelId::EvolinkClaudeHaiku45 => Cow::Borrowed("Claude Haiku 4.5 (Evolink)"),
            ModelId::OpenRouterMoonshotaiKimiK26 => Cow::Borrowed("Kimi K2.6 (OpenRouter)"),
            ModelId::OpenRouterMoonshotaiKimiK27Code => {
                Cow::Borrowed("Kimi K2.7 Code (OpenRouter)")
            }
            ModelId::OpenRouterZaiGlm51 => Cow::Borrowed("GLM-5.1 (OpenRouter)"),
            ModelId::OpenRouterZaiGlm52 => Cow::Borrowed("GLM-5.2 (OpenRouter)"),
            ModelId::OpenRouterOpenAIGpt55 => Cow::Borrowed("OpenAI GPT-5.5 (OpenRouter)"),
            // MiniMax models
            ModelId::MinimaxM3 => Cow::Borrowed("MiniMax-M3"),
            ModelId::MinimaxM27 => Cow::Borrowed("MiniMax-M2.7"),
            // Poolside models
            ModelId::PoolsideLagunaM1 => Cow::Borrowed("Laguna M.1"),
            ModelId::PoolsideLagunaXs2 => Cow::Borrowed("Laguna XS.2"),
            // Moonshot models
            ModelId::MoonshotKimiK27Code => Cow::Borrowed("Kimi K2.7 Code (Moonshot)"),
            ModelId::MoonshotKimiK26 => Cow::Borrowed("Kimi K2.6 (Moonshot)"),
            // OpenCode Zen models
            ModelId::OpenCodeZenGPT54 => Cow::Borrowed("GPT-5.4 (OpenCode Zen)"),
            ModelId::OpenCodeZenGPT54Mini => Cow::Borrowed("GPT-5.4 Mini (OpenCode Zen)"),
            ModelId::OpenCodeZenClaudeSonnet46 => Cow::Borrowed("Claude Sonnet 4.6 (OpenCode Zen)"),
            ModelId::OpenCodeZenGlm51 => Cow::Borrowed("GLM-5.1 (OpenCode Zen)"),
            // OpenCode Go models
            ModelId::OpenCodeGoGlm52 => Cow::Borrowed("GLM-5.2 (OpenCode Go)"),
            ModelId::OpenCodeGoGlm51 => Cow::Borrowed("GLM-5.1 (OpenCode Go)"),
            ModelId::OpenCodeGoKimiK27Code => Cow::Borrowed("Kimi K2.7 Code (OpenCode Go)"),
            ModelId::OpenCodeGoKimiK26 => Cow::Borrowed("Kimi K2.6 (OpenCode Go)"),
            ModelId::OpenCodeGoMimoV25 => Cow::Borrowed("MiMo-V2.5 (OpenCode Go)"),
            ModelId::OpenCodeGoMimoV25Pro => Cow::Borrowed("MiMo-V2.5-Pro (OpenCode Go)"),
            ModelId::OpenCodeGoMinimaxM3 => Cow::Borrowed("MiniMax-M3 (OpenCode Go)"),
            ModelId::OpenCodeGoMinimaxM27 => Cow::Borrowed("MiniMax-M2.7 (OpenCode Go)"),
            ModelId::OpenCodeGoQwen37Max => Cow::Borrowed("Qwen3.7 Max (OpenCode Go)"),
            ModelId::OpenCodeGoQwen37Plus => Cow::Borrowed("Qwen3.7 Plus (OpenCode Go)"),
            ModelId::OpenCodeGoQwen36Plus => Cow::Borrowed("Qwen3.6 Plus (OpenCode Go)"),
            ModelId::OpenCodeGoDeepseekV4Pro => Cow::Borrowed("DeepSeek V4 Pro (OpenCode Go)"),
            ModelId::OpenCodeGoDeepseekV4Flash => Cow::Borrowed("DeepSeek V4 Flash (OpenCode Go)"),
            // Custom user-defined models
            ModelId::Custom(_, model) => Cow::Owned(model.clone()),
            // OpenRouter models
            model => Cow::Borrowed(
                model
                    .openrouter_metadata()
                    .expect("generated OpenRouter model should have metadata")
                    .display,
            ),
        }
    }
}
