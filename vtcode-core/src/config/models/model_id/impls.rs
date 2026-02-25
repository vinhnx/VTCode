use super::ModelId;
use crate::config::models::provider::Provider;

impl ModelId {
    /// Convert the model identifier to its string representation
    /// used in API calls and configurations
    pub fn as_str(&self) -> &'static str {
        use crate::config::constants::models;
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
            ModelId::GPT53Codex => models::openai::GPT_5_3_CODEX,
            ModelId::GPT5Mini => models::GPT_5_MINI,
            ModelId::GPT5Nano => models::GPT_5_NANO,
            ModelId::OpenAIGptOss20b => models::openai::GPT_OSS_20B,
            ModelId::OpenAIGptOss120b => models::openai::GPT_OSS_120B,
            // Anthropic models
            ModelId::ClaudeOpus46 => models::CLAUDE_OPUS_4_6,
            ModelId::ClaudeSonnet46 => models::CLAUDE_SONNET_4_6,
            ModelId::ClaudeOpus41 => models::CLAUDE_OPUS_4_1,
            ModelId::ClaudeOpus45 => models::CLAUDE_OPUS_4_5,
            ModelId::ClaudeSonnet45 => models::CLAUDE_SONNET_4_5,
            ModelId::ClaudeHaiku45 => models::CLAUDE_HAIKU_4_5,
            ModelId::ClaudeOpus4 => models::CLAUDE_OPUS_4_0,
            ModelId::ClaudeSonnet4 => models::CLAUDE_SONNET_4_0,
            ModelId::ClaudeSonnet37 => models::CLAUDE_3_7_SONNET_LATEST,
            ModelId::ClaudeHaiku35 => models::CLAUDE_3_5_HAIKU_LATEST,
            // DeepSeek models
            ModelId::DeepSeekChat => models::DEEPSEEK_CHAT,
            ModelId::DeepSeekReasoner => models::DEEPSEEK_REASONER,
            // Hugging Face Inference Providers
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
            // xAI models
            ModelId::XaiGrok4 => models::xai::GROK_4,
            ModelId::XaiGrok4Mini => models::xai::GROK_4_MINI,
            ModelId::XaiGrok4Code => models::xai::GROK_4_CODE,
            ModelId::XaiGrok4CodeLatest => models::xai::GROK_4_CODE_LATEST,
            ModelId::XaiGrok4Vision => models::xai::GROK_4_VISION,
            // Z.AI models
            ModelId::ZaiGlm5 => models::zai::GLM_5,
            // Moonshot models
            // Ollama models
            ModelId::OllamaGptOss20b => models::ollama::GPT_OSS_20B,
            ModelId::OllamaGptOss20bCloud => models::ollama::GPT_OSS_20B_CLOUD,
            ModelId::OllamaGptOss120bCloud => models::ollama::GPT_OSS_120B_CLOUD,
            ModelId::OllamaQwen317b => models::ollama::QWEN3_1_7B,
            ModelId::OllamaDeepseekV32Cloud => models::ollama::DEEPSEEK_V32_CLOUD,
            ModelId::OllamaQwen3Next80bCloud => models::ollama::QWEN3_NEXT_80B_CLOUD,
            ModelId::OllamaMistralLarge3675bCloud => models::ollama::MISTRAL_LARGE_3_675B_CLOUD,
            ModelId::OllamaQwen3Coder480bCloud => models::ollama::QWEN3_CODER_480B_CLOUD,
            ModelId::OllamaGlm5Cloud => models::ollama::GLM_5_CLOUD,
            ModelId::OllamaGemini3ProPreviewLatestCloud => {
                models::ollama::GEMINI_3_PRO_PREVIEW_LATEST_CLOUD
            }
            ModelId::OllamaGemini3FlashPreviewCloud => models::ollama::GEMINI_3_FLASH_PREVIEW_CLOUD,
            ModelId::OllamaDevstral2123bCloud => models::ollama::DEVSTRAL_2_123B_CLOUD,
            ModelId::OllamaMinimaxM2Cloud => models::ollama::MINIMAX_M2_CLOUD,
            ModelId::OllamaMinimaxM25Cloud => models::ollama::MINIMAX_M25_CLOUD,
            ModelId::OllamaNemotron3Nano30bCloud => models::ollama::NEMOTRON_3_NANO_30B_CLOUD,
            ModelId::LmStudioMetaLlama38BInstruct => models::lmstudio::META_LLAMA_3_8B_INSTRUCT,
            ModelId::LmStudioMetaLlama318BInstruct => models::lmstudio::META_LLAMA_31_8B_INSTRUCT,
            ModelId::LmStudioQwen257BInstruct => models::lmstudio::QWEN25_7B_INSTRUCT,
            ModelId::LmStudioGemma22BIt => models::lmstudio::GEMMA_2_2B_IT,
            ModelId::LmStudioGemma29BIt => models::lmstudio::GEMMA_2_9B_IT,
            ModelId::LmStudioPhi31Mini4kInstruct => models::lmstudio::PHI_31_MINI_4K_INSTRUCT,
            // MiniMax models
            ModelId::MinimaxM25 => models::minimax::MINIMAX_M2_5,
            ModelId::MinimaxM2 => models::minimax::MINIMAX_M2,
            // OpenRouter models - fallback for any OpenRouter model without metadata
            ModelId::OpenRouterGrokCodeFast1
            | ModelId::OpenRouterGrok4Fast
            | ModelId::OpenRouterGrok41Fast
            | ModelId::OpenRouterGrok4
            | ModelId::OpenRouterQwen3Max
            | ModelId::OpenRouterQwen3235bA22b
            | ModelId::OpenRouterQwen3235bA22b2507
            | ModelId::OpenRouterQwen3235bA22bThinking2507
            | ModelId::OpenRouterQwen332b
            | ModelId::OpenRouterQwen330bA3b
            | ModelId::OpenRouterQwen330bA3bInstruct2507
            | ModelId::OpenRouterQwen330bA3bThinking2507
            | ModelId::OpenRouterQwen314b
            | ModelId::OpenRouterQwen38b
            | ModelId::OpenRouterQwen3Next80bA3bInstruct
            | ModelId::OpenRouterQwen3Next80bA3bThinking
            | ModelId::OpenRouterQwen35Plus0215
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash
            | ModelId::OpenRouterQwen3Coder30bA3bInstruct
            | ModelId::OpenRouterQwen3CoderNext
            | ModelId::OpenRouterDeepSeekV32Exp
            | ModelId::OpenRouterDeepSeekChatV31
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGptOss120b
            | ModelId::OpenRouterOpenAIGptOss20b
            | ModelId::OpenRouterOpenAIGptOss120bFree
            | ModelId::OpenRouterOpenAIGpt5
            | ModelId::OpenRouterOpenAIGpt5Chat
            | ModelId::OpenRouterGoogleGemini31ProPreview
            | ModelId::OpenRouterAnthropicClaudeSonnet45
            | ModelId::OpenRouterAnthropicClaudeSonnet46
            | ModelId::OpenRouterAnthropicClaudeHaiku45
            | ModelId::OpenRouterAnthropicClaudeOpus41
            | ModelId::OpenRouterDeepseekChat
            | ModelId::OpenRouterDeepSeekV32
            | ModelId::OpenRouterDeepseekReasoner
            | ModelId::OpenRouterDeepSeekV32Speciale
            | ModelId::OpenRouterAmazonNova2LiteV1
            | ModelId::OpenRouterMistralaiMistralLarge2512
            | ModelId::OpenRouterNexAgiDeepseekV31NexN1
            | ModelId::OpenRouterOpenAIGpt52
            | ModelId::OpenRouterOpenAIGpt52Chat
            | ModelId::OpenRouterOpenAIGpt52Codex
            | ModelId::OpenRouterOpenAIGpt52Pro
            | ModelId::OpenRouterOpenAIO1Pro
            | ModelId::OpenRouterStepfunStep35FlashFree
            | ModelId::OpenRouterMoonshotaiKimiK20905
            | ModelId::OpenRouterMoonshotaiKimiK2Thinking
            | ModelId::OpenRouterMoonshotaiKimiK25
            | ModelId::OpenRouterZaiGlm5 => {
                // Fallback to a default value for OpenRouter models without metadata
                "openrouter-model"
            }
        }
    }

    /// Get the provider for this model
    pub fn provider(&self) -> Provider {
        if self.openrouter_metadata().is_some() {
            return Provider::OpenRouter;
        }
        match self {
            ModelId::Gemini31ProPreview
            | ModelId::Gemini31ProPreviewCustomTools
            | ModelId::Gemini3ProPreview
            | ModelId::Gemini3FlashPreview => Provider::Gemini,
            ModelId::GPT5
            | ModelId::GPT52
            | ModelId::GPT52Codex
            | ModelId::GPT53Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => Provider::OpenAI,
            ModelId::ClaudeOpus46
            | ModelId::ClaudeSonnet46
            | ModelId::ClaudeOpus41
            | ModelId::ClaudeOpus45
            | ModelId::ClaudeSonnet45
            | ModelId::ClaudeHaiku45
            | ModelId::ClaudeOpus4
            | ModelId::ClaudeSonnet4
            | ModelId::ClaudeSonnet37
            | ModelId::ClaudeHaiku35 => Provider::Anthropic,
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => Provider::DeepSeek,
            ModelId::HuggingFaceDeepseekV32
            | ModelId::HuggingFaceOpenAIGptOss20b
            | ModelId::HuggingFaceOpenAIGptOss120b
            | ModelId::HuggingFaceMinimaxM25Novita
            | ModelId::HuggingFaceDeepseekV32Novita
            | ModelId::HuggingFaceXiaomiMimoV2FlashNovita
            | ModelId::HuggingFaceGlm5Novita
            | ModelId::HuggingFaceQwen3CoderNextNovita => Provider::HuggingFace,
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => Provider::XAI,
            ModelId::ZaiGlm5 => Provider::ZAI,
            ModelId::OllamaGptOss20b
            | ModelId::OllamaGptOss20bCloud
            | ModelId::OllamaGptOss120bCloud
            | ModelId::OllamaQwen317b
            | ModelId::OllamaDeepseekV32Cloud
            | ModelId::OllamaQwen3Next80bCloud
            | ModelId::OllamaMistralLarge3675bCloud
            | ModelId::OllamaQwen3Coder480bCloud
            | ModelId::OllamaGemini3ProPreviewLatestCloud
            | ModelId::OllamaGemini3FlashPreviewCloud
            | ModelId::OllamaDevstral2123bCloud
            | ModelId::OllamaMinimaxM2Cloud
            | ModelId::OllamaMinimaxM25Cloud
            | ModelId::OllamaGlm5Cloud
            | ModelId::OllamaNemotron3Nano30bCloud => Provider::Ollama,
            ModelId::LmStudioMetaLlama38BInstruct
            | ModelId::LmStudioMetaLlama318BInstruct
            | ModelId::LmStudioQwen257BInstruct
            | ModelId::LmStudioGemma22BIt
            | ModelId::LmStudioGemma29BIt
            | ModelId::LmStudioPhi31Mini4kInstruct => Provider::LmStudio,
            ModelId::MinimaxM25 | ModelId::MinimaxM2 => Provider::Minimax,
            // OpenRouter models - fallback for any OpenRouter model without metadata
            ModelId::OpenRouterGrokCodeFast1
            | ModelId::OpenRouterGrok4Fast
            | ModelId::OpenRouterGrok41Fast
            | ModelId::OpenRouterGrok4
            | ModelId::OpenRouterQwen3Max
            | ModelId::OpenRouterQwen3235bA22b
            | ModelId::OpenRouterQwen3235bA22b2507
            | ModelId::OpenRouterQwen3235bA22bThinking2507
            | ModelId::OpenRouterQwen332b
            | ModelId::OpenRouterQwen330bA3b
            | ModelId::OpenRouterQwen330bA3bInstruct2507
            | ModelId::OpenRouterQwen330bA3bThinking2507
            | ModelId::OpenRouterQwen314b
            | ModelId::OpenRouterQwen38b
            | ModelId::OpenRouterQwen3Next80bA3bInstruct
            | ModelId::OpenRouterQwen3Next80bA3bThinking
            | ModelId::OpenRouterQwen35Plus0215
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash
            | ModelId::OpenRouterQwen3Coder30bA3bInstruct
            | ModelId::OpenRouterQwen3CoderNext
            | ModelId::OpenRouterDeepSeekV32Exp
            | ModelId::OpenRouterDeepSeekChatV31
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGptOss120b
            | ModelId::OpenRouterOpenAIGptOss120bFree
            | ModelId::OpenRouterOpenAIGptOss20b
            | ModelId::OpenRouterOpenAIGpt5
            | ModelId::OpenRouterOpenAIGpt5Chat
            | ModelId::OpenRouterGoogleGemini31ProPreview
            | ModelId::OpenRouterAnthropicClaudeSonnet45
            | ModelId::OpenRouterAnthropicClaudeSonnet46
            | ModelId::OpenRouterAnthropicClaudeHaiku45
            | ModelId::OpenRouterAnthropicClaudeOpus41
            | ModelId::OpenRouterDeepseekChat
            | ModelId::OpenRouterDeepSeekV32
            | ModelId::OpenRouterDeepseekReasoner
            | ModelId::OpenRouterDeepSeekV32Speciale
            | ModelId::OpenRouterAmazonNova2LiteV1
            | ModelId::OpenRouterMistralaiMistralLarge2512
            | ModelId::OpenRouterNexAgiDeepseekV31NexN1
            | ModelId::OpenRouterOpenAIGpt52
            | ModelId::OpenRouterOpenAIGpt52Chat
            | ModelId::OpenRouterOpenAIGpt52Codex
            | ModelId::OpenRouterOpenAIGpt52Pro
            | ModelId::OpenRouterOpenAIO1Pro
            | ModelId::OpenRouterStepfunStep35FlashFree
            | ModelId::OpenRouterMoonshotaiKimiK20905
            | ModelId::OpenRouterMoonshotaiKimiK2Thinking
            | ModelId::OpenRouterMoonshotaiKimiK25
            | ModelId::OpenRouterZaiGlm5 => Provider::OpenRouter,
        }
    }
}
