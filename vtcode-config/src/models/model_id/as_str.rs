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
            ModelId::Gemini3ProPreview => models::GEMINI_3_PRO_PREVIEW,
            ModelId::Gemini3FlashPreview => models::GEMINI_3_FLASH_PREVIEW,
            // OpenAI models
            ModelId::GPT5 => models::GPT_5,
            ModelId::GPT52 => models::GPT_5_2,
            ModelId::GPT52Codex => models::openai::GPT_5_2_CODEX,
            ModelId::GPT5Mini => models::GPT_5_MINI,
            ModelId::GPT5Nano => models::GPT_5_NANO,
            ModelId::GPT53Codex => models::openai::GPT_5_3_CODEX,
            ModelId::OpenAIGptOss20b => models::openai::GPT_OSS_20B,
            ModelId::OpenAIGptOss120b => models::openai::GPT_OSS_120B,
            // Anthropic models
            ModelId::ClaudeOpus46 => models::CLAUDE_OPUS_4_6,
            ModelId::ClaudeSonnet46 => models::CLAUDE_SONNET_4_6,
            ModelId::ClaudeOpus45 => models::CLAUDE_OPUS_4_5,
            ModelId::ClaudeOpus41 => models::CLAUDE_OPUS_4_1,
            ModelId::ClaudeSonnet45 => models::CLAUDE_SONNET_4_5,
            ModelId::ClaudeHaiku45 => models::CLAUDE_HAIKU_4_5,
            ModelId::ClaudeSonnet4 => models::CLAUDE_SONNET_4_5_20250929,
            ModelId::ClaudeOpus4 => models::CLAUDE_OPUS_4_0,
            ModelId::ClaudeSonnet37 => models::CLAUDE_3_7_SONNET_LATEST,
            ModelId::ClaudeHaiku35 => models::CLAUDE_3_5_HAIKU_LATEST,
            // DeepSeek models
            ModelId::DeepSeekChat => models::DEEPSEEK_CHAT,
            ModelId::DeepSeekReasoner => models::DEEPSEEK_REASONER,
            // xAI models
            ModelId::XaiGrok4 => models::xai::GROK_4,
            ModelId::XaiGrok4Mini => models::xai::GROK_4_MINI,
            ModelId::XaiGrok4Code => models::xai::GROK_4_CODE,
            ModelId::XaiGrok4CodeLatest => models::xai::GROK_4_CODE_LATEST,
            ModelId::XaiGrok4Vision => models::xai::GROK_4_VISION,
            // Z.AI models
            ModelId::ZaiGlm5 => models::zai::GLM_5,
            // Moonshot models
            ModelId::MoonshotMinimaxM25 => models::moonshot::MINIMAX_M2_5,
            ModelId::MoonshotQwen3CoderNext => models::moonshot::QWEN3_CODER_NEXT,
            // Ollama models
            ModelId::OllamaGptOss20b => models::ollama::GPT_OSS_20B,
            ModelId::OllamaGptOss20bCloud => models::ollama::GPT_OSS_20B_CLOUD,
            ModelId::OllamaGptOss120bCloud => models::ollama::GPT_OSS_120B_CLOUD,
            ModelId::OllamaQwen317b => models::ollama::QWEN3_1_7B,
            ModelId::OllamaDeepseekV32Cloud => models::ollama::DEEPSEEK_V32_CLOUD,
            ModelId::OllamaQwen3Next80bCloud => models::ollama::QWEN3_NEXT_80B_CLOUD,
            ModelId::OllamaMistralLarge3675bCloud => models::ollama::MISTRAL_LARGE_3_675B_CLOUD,
            ModelId::OllamaGlm5Cloud => models::ollama::GLM_5_CLOUD,
            ModelId::OllamaMinimaxM25Cloud => models::ollama::MINIMAX_M25_CLOUD,
            ModelId::OllamaGemini3FlashPreviewCloud => models::ollama::GEMINI_3_FLASH_PREVIEW_CLOUD,
            ModelId::OllamaQwen3Coder480bCloud => models::ollama::QWEN3_CODER_480B_CLOUD,
            ModelId::OllamaGemini3ProPreviewLatestCloud => {
                models::ollama::GEMINI_3_PRO_PREVIEW_LATEST_CLOUD
            }
            ModelId::OllamaDevstral2123bCloud => models::ollama::DEVSTRAL_2_123B_CLOUD,
            ModelId::OllamaMinimaxM2Cloud => models::ollama::MINIMAX_M2_CLOUD,
            ModelId::OllamaNemotron3Nano30bCloud => models::ollama::NEMOTRON_3_NANO_30B_CLOUD,
            // LM Studio models
            ModelId::LmStudioMetaLlama38BInstruct => models::lmstudio::META_LLAMA_3_8B_INSTRUCT,
            ModelId::LmStudioMetaLlama318BInstruct => models::lmstudio::META_LLAMA_31_8B_INSTRUCT,
            ModelId::LmStudioQwen257BInstruct => models::lmstudio::QWEN25_7B_INSTRUCT,
            ModelId::LmStudioGemma22BIt => models::lmstudio::GEMMA_2_2B_IT,
            ModelId::LmStudioGemma29BIt => models::lmstudio::GEMMA_2_9B_IT,
            ModelId::LmStudioPhi31Mini4kInstruct => models::lmstudio::PHI_31_MINI_4K_INSTRUCT,
            // Hugging Face models
            ModelId::HuggingFaceDeepseekV32 => models::huggingface::DEEPSEEK_V32,
            ModelId::HuggingFaceOpenAIGptOss20b => models::huggingface::OPENAI_GPT_OSS_20B,
            ModelId::HuggingFaceOpenAIGptOss120b => models::huggingface::OPENAI_GPT_OSS_120B,
            ModelId::HuggingFaceMinimaxM25Novita => models::huggingface::MINIMAX_M2_5_NOVITA,
            ModelId::HuggingFaceDeepseekV32Novita => models::huggingface::DEEPSEEK_V32_NOVITA,
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => {
                models::huggingface::XIAOMI_MIMO_V2_FLASH_NOVITA
            }
            ModelId::HuggingFaceGlm5Novita => models::huggingface::ZAI_GLM_5_NOVITA,
            ModelId::HuggingFaceQwen3CoderNextNovita => {
                models::huggingface::QWEN3_CODER_NEXT_NOVITA
            }
            // MiniMax models
            ModelId::MinimaxM25 => models::minimax::MINIMAX_M2_5,
            ModelId::MinimaxM2 => models::minimax::MINIMAX_M2,
            // OpenRouter models
            ModelId::OpenRouterMinimaxM25 => "minimax/minimax-m2.5",
            ModelId::OpenRouterQwen3CoderNext => "qwen/qwen3-coder-next",
            _ => unreachable!(),
        }
    }
}
