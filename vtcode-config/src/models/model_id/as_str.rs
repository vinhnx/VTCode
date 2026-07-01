use std::borrow::Cow;

use super::ModelId;

impl ModelId {
    /// Convert the model identifier to its string representation
    /// used in API calls and configurations.
    ///
    /// Returns `Cow<'static, str>` because custom user-defined models
    /// carry runtime strings that may not be `'static`.
    pub fn as_str(&self) -> Cow<'static, str> {
        use crate::constants::models;
        if let Some(meta) = self.openrouter_metadata() {
            return Cow::Borrowed(meta.id);
        }
        match self {
            // Gemini models
            ModelId::Gemini31ProPreview => Cow::Borrowed(models::GEMINI_3_1_PRO_PREVIEW),
            ModelId::Gemini31ProPreviewCustomTools => {
                Cow::Borrowed(models::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS)
            }
            ModelId::Gemini35Flash => Cow::Borrowed(models::GEMINI_3_5_FLASH),
            // OpenAI models
            ModelId::GPT55 => Cow::Borrowed(models::openai::GPT_5_5),
            ModelId::GPT54 => Cow::Borrowed(models::GPT_5_4),
            ModelId::GPT54Pro => Cow::Borrowed(models::GPT_5_4_PRO),
            ModelId::GPT54Nano => Cow::Borrowed(models::openai::GPT_5_4_NANO),
            ModelId::GPT54Mini => Cow::Borrowed(models::openai::GPT_5_4_MINI),
            ModelId::GPT53Codex => Cow::Borrowed(models::openai::GPT_5_3_CODEX),
            ModelId::OpenAIGptOss20b => Cow::Borrowed(models::openai::GPT_OSS_20B),
            ModelId::OpenAIGptOss120b => Cow::Borrowed(models::openai::GPT_OSS_120B),
            // Anthropic models
            ModelId::ClaudeSonnet5 => Cow::Borrowed(models::CLAUDE_SONNET_5),
            ModelId::ClaudeFable5 => Cow::Borrowed(models::CLAUDE_FABLE_5),
            ModelId::ClaudeMythos5 => Cow::Borrowed(models::CLAUDE_MYTHOS_5),
            ModelId::ClaudeOpus48 => Cow::Borrowed(models::CLAUDE_OPUS_4_8),
            ModelId::ClaudeSonnet46 => Cow::Borrowed(models::CLAUDE_SONNET_4_6),
            ModelId::ClaudeHaiku45 => Cow::Borrowed(models::CLAUDE_HAIKU_4_5),
            ModelId::CopilotAuto => Cow::Borrowed(models::copilot::AUTO),
            ModelId::CopilotGPT52Codex => Cow::Borrowed(models::copilot::GPT_5_2_CODEX),
            ModelId::CopilotGPT51CodexMax => Cow::Borrowed(models::copilot::GPT_5_1_CODEX_MAX),
            ModelId::CopilotGPT54 => Cow::Borrowed(models::copilot::GPT_5_4),
            ModelId::CopilotGPT54Mini => Cow::Borrowed(models::copilot::GPT_5_4_MINI),
            ModelId::CopilotClaudeSonnet46 => Cow::Borrowed(models::copilot::CLAUDE_SONNET_4_6),
            // DeepSeek models
            ModelId::DeepSeekV4Pro => Cow::Borrowed(models::deepseek::DEEPSEEK_V4_PRO),
            ModelId::DeepSeekV4Flash => Cow::Borrowed(models::deepseek::DEEPSEEK_V4_FLASH),
            // Mistral models
            ModelId::MistralLarge3 => Cow::Borrowed(models::mistral::MISTRAL_LARGE_3),
            // MiMo models
            ModelId::MiMoV25Pro => Cow::Borrowed(models::mimo::MIMO_V2_5_PRO),
            ModelId::MiMoV25 => Cow::Borrowed(models::mimo::MIMO_V2_5),
            // Z.AI models
            ModelId::ZaiGlm52 => Cow::Borrowed(models::zai::GLM_5_2),
            ModelId::ZaiGlm51 => Cow::Borrowed(models::zai::GLM_5_1),
            // Moonshot models
            ModelId::MoonshotKimiK26 => Cow::Borrowed(models::moonshot::KIMI_K2_6),
            // OpenCode Zen models
            ModelId::OpenCodeZenGPT54 => Cow::Borrowed(models::opencode_zen::GPT_5_4),
            ModelId::OpenCodeZenGPT54Mini => Cow::Borrowed(models::opencode_zen::GPT_5_4_MINI),
            ModelId::OpenCodeZenClaudeSonnet46 => {
                Cow::Borrowed(models::opencode_zen::CLAUDE_SONNET_4_6)
            }
            ModelId::OpenCodeZenGlm51 => Cow::Borrowed(models::opencode_zen::GLM_5_1),
            // OpenCode Go models
            ModelId::OpenCodeGoGlm52 => Cow::Borrowed(models::opencode_go::GLM_5_2),
            ModelId::OpenCodeGoGlm51 => Cow::Borrowed(models::opencode_go::GLM_5_1),
            ModelId::OpenCodeGoKimiK27Code => Cow::Borrowed(models::opencode_go::KIMI_K2_7_CODE),
            ModelId::OpenCodeGoKimiK26 => Cow::Borrowed(models::opencode_go::KIMI_K2_6),
            ModelId::OpenCodeGoMimoV25 => Cow::Borrowed(models::opencode_go::MIMO_V2_5),
            ModelId::OpenCodeGoMimoV25Pro => Cow::Borrowed(models::opencode_go::MIMO_V2_5_PRO),
            ModelId::OpenCodeGoMinimaxM3 => Cow::Borrowed(models::opencode_go::MINIMAX_M3),
            ModelId::OpenCodeGoMinimaxM27 => Cow::Borrowed(models::opencode_go::MINIMAX_M2_7),
            ModelId::OpenCodeGoQwen37Max => Cow::Borrowed(models::opencode_go::QWEN_3_7_MAX),
            ModelId::OpenCodeGoQwen37Plus => Cow::Borrowed(models::opencode_go::QWEN_3_7_PLUS),
            ModelId::OpenCodeGoQwen36Plus => Cow::Borrowed(models::opencode_go::QWEN_3_6_PLUS),
            ModelId::OpenCodeGoDeepseekV4Pro => Cow::Borrowed(models::opencode_go::DEEPSEEK_V4_PRO),
            ModelId::OpenCodeGoDeepseekV4Flash => {
                Cow::Borrowed(models::opencode_go::DEEPSEEK_V4_FLASH)
            }
            // Ollama models
            ModelId::OllamaGptOss20b => Cow::Borrowed(models::ollama::GPT_OSS_20B),
            ModelId::OllamaGptOss20bCloud => Cow::Borrowed(models::ollama::GPT_OSS_20B_CLOUD),
            ModelId::OllamaGptOss120bCloud => Cow::Borrowed(models::ollama::GPT_OSS_120B_CLOUD),
            ModelId::OllamaDeepseekV4FlashCloud => {
                Cow::Borrowed(models::ollama::DEEPSEEK_V4_FLASH_CLOUD)
            }
            ModelId::OllamaDeepseekV4ProCloud => {
                Cow::Borrowed(models::ollama::DEEPSEEK_V4_PRO_CLOUD)
            }
            ModelId::OllamaGlm51Cloud => Cow::Borrowed(models::ollama::GLM_5_1_CLOUD),
            ModelId::OllamaGlm52Cloud => Cow::Borrowed(models::ollama::GLM_5_2_CLOUD),
            ModelId::OllamaMinimaxM27Cloud => Cow::Borrowed(models::ollama::MINIMAX_M27_CLOUD),
            ModelId::OllamaMinimaxM3Cloud => Cow::Borrowed(models::ollama::MINIMAX_M3_CLOUD),
            ModelId::OllamaKimiK26Cloud => Cow::Borrowed(models::ollama::KIMI_K2_6_CLOUD),
            ModelId::OllamaKimiK27CodeCloud => Cow::Borrowed(models::ollama::KIMI_K2_7_CODE_CLOUD),
            ModelId::OllamaGemma4 => Cow::Borrowed(models::ollama::GEMMA_4),
            ModelId::OllamaLagunaXs2 => Cow::Borrowed(models::ollama::LAGUNA_XS_2),
            // llama.cpp models
            ModelId::LlamaCppGemma426bA4b => Cow::Borrowed(models::llamacpp::GEMMA_4_26B_A4B),
            ModelId::LlamaCppGemma4E4b => Cow::Borrowed(models::llamacpp::GEMMA_4_E4B),
            ModelId::LlamaCppGptOss20b => Cow::Borrowed(models::llamacpp::GPT_OSS_20B),
            ModelId::LlamaCppStep35Flash => Cow::Borrowed(models::llamacpp::STEP_3_5_FLASH),
            // Hugging Face models
            ModelId::HuggingFaceOpenAIGptOss20b => {
                Cow::Borrowed(models::huggingface::OPENAI_GPT_OSS_20B)
            }
            ModelId::HuggingFaceOpenAIGptOss120b => {
                Cow::Borrowed(models::huggingface::OPENAI_GPT_OSS_120B)
            }
            ModelId::HuggingFaceGlm51ZaiOrg => {
                Cow::Borrowed(models::huggingface::ZAI_GLM_5_1_ZAI_ORG)
            }
            ModelId::HuggingFaceGlm52Novita => {
                Cow::Borrowed(models::huggingface::ZAI_GLM_5_2_NOVITA)
            }
            ModelId::HuggingFaceKimiK26Novita => {
                Cow::Borrowed(models::huggingface::KIMI_K2_6_NOVITA)
            }
            ModelId::HuggingFaceDeepseekV4FlashNovita => {
                Cow::Borrowed(models::huggingface::DEEPSEEK_V4_FLASH_NOVITA)
            }
            ModelId::HuggingFaceDeepseekV4ProTogether => {
                Cow::Borrowed(models::huggingface::DEEPSEEK_V4_PRO_TOGETHER)
            }
            ModelId::HuggingFaceStep35Flash => Cow::Borrowed(models::huggingface::STEP_3_5_FLASH),
            ModelId::HuggingFaceGlm51Deepinfra => {
                Cow::Borrowed(models::huggingface::ZAI_GLM_5_1_DEEPINFRA)
            }
            ModelId::HuggingFaceMinimaxM27Novita => {
                Cow::Borrowed(models::huggingface::MINIMAX_M2_7_NOVITA)
            }
            ModelId::HuggingFaceMinimaxM3Novita => {
                Cow::Borrowed(models::huggingface::MINIMAX_M3_NOVITA)
            }
            ModelId::HuggingFaceDeepseekV4ProNovita => {
                Cow::Borrowed(models::huggingface::DEEPSEEK_V4_PRO_NOVITA)
            }
            ModelId::StepFun37Flash => Cow::Borrowed(models::stepfun::STEP_3_7_FLASH),
            // Evolink gateway models (namespaced; the provider strips the `evolink/` prefix)
            ModelId::EvolinkGpt52 => Cow::Borrowed("evolink/gpt-5.2"),
            ModelId::EvolinkGpt55 => Cow::Borrowed("evolink/gpt-5.5"),
            ModelId::EvolinkDeepseekV4Pro => Cow::Borrowed("evolink/deepseek-v4-pro"),
            ModelId::EvolinkDeepseekV4Flash => Cow::Borrowed("evolink/deepseek-v4-flash"),
            ModelId::EvolinkDoubaoSeed20Pro => Cow::Borrowed("evolink/doubao-seed-2.0-pro"),
            ModelId::EvolinkGemini31Pro => Cow::Borrowed("evolink/gemini-3.1-pro-preview"),
            ModelId::EvolinkGemini35Flash => Cow::Borrowed("evolink/gemini-3.5-flash"),
            ModelId::EvolinkMinimaxM3 => Cow::Borrowed("evolink/MiniMax-M3"),
            ModelId::EvolinkClaudeSonnet46 => Cow::Borrowed("evolink/claude-sonnet-4-6"),
            ModelId::EvolinkClaudeOpus48 => Cow::Borrowed("evolink/claude-opus-4-8"),
            ModelId::EvolinkClaudeHaiku45 => Cow::Borrowed("evolink/claude-haiku-4-5-20251001"),
            // Qwen models
            ModelId::QwenDeepSeekV4Flash => Cow::Borrowed(models::qwen::DEEPSEEK_V4_FLASH),
            ModelId::QwenDeepSeekV4Pro => Cow::Borrowed(models::qwen::DEEPSEEK_V4_PRO),
            ModelId::QwenGlm51 => Cow::Borrowed(models::qwen::GLM_5_1),
            // MiniMax models
            ModelId::MinimaxM3 => Cow::Borrowed(models::minimax::MINIMAX_M3),
            ModelId::MinimaxM27 => Cow::Borrowed(models::minimax::MINIMAX_M2_7),
            // Poolside models
            ModelId::PoolsideLagunaM1 => Cow::Borrowed(models::poolside::LAGUNA_M1),
            ModelId::PoolsideLagunaXs2 => Cow::Borrowed(models::poolside::LAGUNA_XS2),
            // Moonshot models
            ModelId::MoonshotKimiK27Code => Cow::Borrowed(models::moonshot::KIMI_K2_7_CODE),
            // OpenRouter models
            ModelId::OpenRouterMoonshotaiKimiK26 => Cow::Borrowed("moonshotai/kimi-k2.6"),
            ModelId::OpenRouterMoonshotaiKimiK27Code => Cow::Borrowed("moonshotai/kimi-k2.7-code"),
            ModelId::OpenRouterZaiGlm51 => Cow::Borrowed("z-ai/glm-5.1"),
            ModelId::OpenRouterZaiGlm52 => Cow::Borrowed("z-ai/glm-5.2"),
            // Custom user-defined models
            ModelId::Custom(_, model) => Cow::Owned(model.clone()),
            model => Cow::Borrowed(
                model
                    .openrouter_metadata()
                    .expect("generated OpenRouter model should have metadata")
                    .id,
            ),
        }
    }
}
