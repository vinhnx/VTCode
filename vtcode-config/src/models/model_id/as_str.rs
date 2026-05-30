use super::ModelId;

impl ModelId {
    /// Convert the model identifier to its string representation
    /// used in API calls and configurations
    pub fn as_str(&self) -> &'static str {
        use crate::constants::models;
        if let Some(meta) = self.openrouter_metadata() {
            return meta.id;
        }
        match self {
            // Gemini models
            ModelId::Gemini31ProPreview => models::GEMINI_3_1_PRO_PREVIEW,
            ModelId::Gemini31ProPreviewCustomTools => models::GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS,
            ModelId::Gemini31FlashLitePreview => models::GEMINI_3_1_FLASH_LITE_PREVIEW,
            ModelId::Gemini35Flash => models::GEMINI_3_5_FLASH,
            // OpenAI models
            ModelId::GPT55 => models::openai::GPT_5_5,
            ModelId::GPT54 => models::GPT_5_4,
            ModelId::GPT54Pro => models::GPT_5_4_PRO,
            ModelId::GPT54Nano => models::openai::GPT_5_4_NANO,
            ModelId::GPT54Mini => models::openai::GPT_5_4_MINI,
            ModelId::GPT53Codex => models::openai::GPT_5_3_CODEX,
            ModelId::OpenAIGptOss20b => models::openai::GPT_OSS_20B,
            ModelId::OpenAIGptOss120b => models::openai::GPT_OSS_120B,
            // Anthropic models
            ModelId::ClaudeOpus48 => models::CLAUDE_OPUS_4_8,
            ModelId::ClaudeSonnet46 => models::CLAUDE_SONNET_4_6,
            ModelId::ClaudeHaiku45 => models::CLAUDE_HAIKU_4_5,
            ModelId::ClaudeMythosPreview => models::CLAUDE_MYTHOS_PREVIEW,
            ModelId::CopilotAuto => models::copilot::AUTO,
            ModelId::CopilotGPT52Codex => models::copilot::GPT_5_2_CODEX,
            ModelId::CopilotGPT51CodexMax => models::copilot::GPT_5_1_CODEX_MAX,
            ModelId::CopilotGPT54 => models::copilot::GPT_5_4,
            ModelId::CopilotGPT54Mini => models::copilot::GPT_5_4_MINI,
            ModelId::CopilotClaudeSonnet46 => models::copilot::CLAUDE_SONNET_4_6,
            // DeepSeek models
            ModelId::DeepSeekV4Pro => models::deepseek::DEEPSEEK_V4_PRO,
            ModelId::DeepSeekV4Flash => models::deepseek::DEEPSEEK_V4_FLASH,
            // Mistral models
            ModelId::MistralLarge3 => models::mistral::MISTRAL_LARGE_3,
            // MiMo models
            ModelId::MiMoV25Pro => models::mimo::MIMO_V2_5_PRO,
            ModelId::MiMoV25 => models::mimo::MIMO_V2_5,
            // Z.AI models
            ModelId::ZaiGlm5 => models::zai::GLM_5,
            ModelId::ZaiGlm51 => models::zai::GLM_5_1,
            // Moonshot models
            ModelId::MoonshotKimiK26 => models::moonshot::KIMI_K2_6,
            // OpenCode Zen models
            ModelId::OpenCodeZenGPT54 => models::opencode_zen::GPT_5_4,
            ModelId::OpenCodeZenGPT54Mini => models::opencode_zen::GPT_5_4_MINI,
            ModelId::OpenCodeZenClaudeSonnet46 => models::opencode_zen::CLAUDE_SONNET_4_6,
            ModelId::OpenCodeZenGlm51 => models::opencode_zen::GLM_5_1,
            // OpenCode Go models
            ModelId::OpenCodeGoGlm51 => models::opencode_go::GLM_5_1,
            ModelId::OpenCodeGoMinimaxM25 => models::opencode_go::MINIMAX_M2_5,
            ModelId::OpenCodeGoMinimaxM27 => models::opencode_go::MINIMAX_M2_7,
            // Ollama models
            ModelId::OllamaGptOss20b => models::ollama::GPT_OSS_20B,
            ModelId::OllamaGptOss20bCloud => models::ollama::GPT_OSS_20B_CLOUD,
            ModelId::OllamaGptOss120bCloud => models::ollama::GPT_OSS_120B_CLOUD,
            ModelId::OllamaQwen317b => models::ollama::QWEN3_1_7B,
            ModelId::OllamaQwen3CoderNext => models::ollama::QWEN3_CODER_NEXT,
            ModelId::OllamaDeepseekV4FlashCloud => models::ollama::DEEPSEEK_V4_FLASH_CLOUD,
            ModelId::OllamaDeepseekV4ProCloud => models::ollama::DEEPSEEK_V4_PRO_CLOUD,
            ModelId::OllamaQwen3Next80bCloud => models::ollama::QWEN3_NEXT_80B_CLOUD,
            ModelId::OllamaGlm5Cloud => models::ollama::GLM_5_CLOUD,
            ModelId::OllamaGlm51Cloud => models::ollama::GLM_5_1_CLOUD,
            ModelId::OllamaGemini3FlashPreviewCloud => models::ollama::GEMINI_3_FLASH_PREVIEW_CLOUD,
            ModelId::OllamaMinimaxM2Cloud => models::ollama::MINIMAX_M2_CLOUD,
            ModelId::OllamaMinimaxM27Cloud => models::ollama::MINIMAX_M27_CLOUD,
            ModelId::OllamaMinimaxM25Cloud => models::ollama::MINIMAX_M25_CLOUD,
            ModelId::OllamaKimiK26Cloud => models::ollama::KIMI_K2_6_CLOUD,
            ModelId::OllamaNemotron3SuperCloud => models::ollama::NEMOTRON_3_SUPER_CLOUD,
            ModelId::OllamaLagunaXs2 => models::ollama::LAGUNA_XS_2,
            // Hugging Face models
            ModelId::HuggingFaceOpenAIGptOss20b => models::huggingface::OPENAI_GPT_OSS_20B,
            ModelId::HuggingFaceOpenAIGptOss120b => models::huggingface::OPENAI_GPT_OSS_120B,
            ModelId::HuggingFaceMinimaxM25Novita => models::huggingface::MINIMAX_M2_5_NOVITA,
            ModelId::HuggingFaceGlm5Novita => models::huggingface::ZAI_GLM_5_NOVITA,
            ModelId::HuggingFaceGlm51ZaiOrg => models::huggingface::ZAI_GLM_5_1_ZAI_ORG,
            ModelId::HuggingFaceQwen3CoderNextNovita => {
                models::huggingface::QWEN3_CODER_NEXT_NOVITA
            }
            ModelId::HuggingFaceQwen35397BA17BTogether => {
                models::huggingface::QWEN3_5_397B_A17B_TOGETHER
            }
            ModelId::HuggingFaceKimiK26Novita => models::huggingface::KIMI_K2_6_NOVITA,
            ModelId::HuggingFaceDeepseekV4FlashNovita => {
                models::huggingface::DEEPSEEK_V4_FLASH_NOVITA
            }
            ModelId::HuggingFaceDeepseekV4ProTogether => {
                models::huggingface::DEEPSEEK_V4_PRO_TOGETHER
            }
            ModelId::HuggingFaceStep35Flash => models::huggingface::STEP_3_5_FLASH,
            ModelId::StepFun37Flash => models::stepfun::STEP_3_7_FLASH,
            // Qwen models
            ModelId::Qwen37Max => models::qwen::QWEN3_7_MAX,
            ModelId::Qwen36Flash => models::qwen::QWEN3_6_FLASH,
            ModelId::Qwen36Plus => models::qwen::QWEN3_6_PLUS,
            ModelId::QwenDeepSeekV4Flash => models::qwen::DEEPSEEK_V4_FLASH,
            ModelId::QwenDeepSeekV4Pro => models::qwen::DEEPSEEK_V4_PRO,
            ModelId::QwenGlm51 => models::qwen::GLM_5_1,
            // MiniMax models
            ModelId::MinimaxM27 => models::minimax::MINIMAX_M2_7,
            ModelId::MinimaxM25 => models::minimax::MINIMAX_M2_5,
            // Poolside models
            ModelId::PoolsideLagunaM1 => models::poolside::LAGUNA_M1,
            ModelId::PoolsideLagunaXs2 => models::poolside::LAGUNA_XS2,
            // OpenRouter models
            ModelId::OpenRouterMinimaxM25 => "minimax/minimax-m2.5",
            ModelId::OpenRouterQwen3CoderNext => "qwen/qwen3-coder-next",
            ModelId::OpenRouterMoonshotaiKimiK26 => "moonshotai/kimi-k2.6",
            _ => unreachable!(),
        }
    }
}
