//! Capability and variant helpers for model identifiers.

use super::ModelId;

impl ModelId {
    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.provider().supports_reasoning_effort(self.as_str())
    }

    /// Attempt to find a non-reasoning variant for this model.
    ///
    /// Returns another [`ModelId`] from the same provider that does not support
    /// configurable reasoning effort, allowing callers to offer a "no
    /// reasoning" option in user interfaces.
    pub fn non_reasoning_variant(&self) -> Option<Self> {
        if let Some(meta) = self.openrouter_metadata() {
            if !meta.reasoning {
                return None;
            }

            let vendor = meta.vendor;
            let mut candidates: Vec<Self> = Self::openrouter_vendor_groups()
                .into_iter()
                .find(|(candidate_vendor, _)| *candidate_vendor == vendor)
                .map(|(_, models)| {
                    models
                        .iter()
                        .copied()
                        .filter(|candidate| candidate != self)
                        .filter(|candidate| {
                            candidate
                                .openrouter_metadata()
                                .map(|other| !other.reasoning)
                                .unwrap_or(false)
                        })
                        .collect()
                })
                .unwrap_or_default();

            if candidates.is_empty() {
                return None;
            }

            candidates.sort_by_key(|candidate| {
                let data = candidate
                    .openrouter_metadata()
                    .expect("OpenRouter metadata missing for candidate");
                (!data.efficient, data.display)
            });

            return candidates.into_iter().next();
        }

        let direct = match self {
            ModelId::Gemini25Pro => Some(ModelId::Gemini25Flash),
            ModelId::Gemini3ProPreview => Some(ModelId::Gemini25Flash),
            ModelId::GPT52 => Some(ModelId::GPT5Mini),
            ModelId::GPT52Codex => Some(ModelId::CodexMiniLatest),
            ModelId::GPT5 => Some(ModelId::GPT5Mini),
            ModelId::GPT5Codex => Some(ModelId::CodexMiniLatest),
            ModelId::GPT51 => Some(ModelId::GPT51Mini),
            ModelId::GPT51Codex => Some(ModelId::CodexMiniLatest),
            ModelId::GPT51CodexMax => Some(ModelId::CodexMiniLatest),
            ModelId::DeepSeekReasoner => Some(ModelId::DeepSeekChat),
            ModelId::XaiGrok4 => Some(ModelId::XaiGrok4Mini),
            ModelId::XaiGrok4Code => Some(ModelId::XaiGrok4CodeLatest),
            ModelId::ZaiGlm47 => Some(ModelId::ZaiGlm45Flash),
            ModelId::ZaiGlm46 => Some(ModelId::ZaiGlm45Flash),
            ModelId::ClaudeOpus45 | ModelId::ClaudeOpus4 | ModelId::ClaudeOpus41 => {
                Some(ModelId::ClaudeSonnet45)
            }
            ModelId::ClaudeSonnet37 => Some(ModelId::ClaudeSonnet45),
            ModelId::MinimaxM21 => Some(ModelId::MinimaxM21Lightning),
            _ => None,
        };

        direct.and_then(|candidate| {
            if candidate.supports_reasoning_effort() {
                None
            } else {
                Some(candidate)
            }
        })
    }

    /// Check if this is a "flash" variant (optimized for speed)
    pub fn is_flash_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25FlashPreview
                | ModelId::Gemini25Flash
                | ModelId::Gemini25FlashLite
                | ModelId::Gemini3FlashPreview
                | ModelId::ZaiGlm45Flash
                | ModelId::MinimaxM21Lightning
                | ModelId::OllamaGemini3FlashPreviewCloud
        )
    }

    /// Check if this is a "pro" variant (optimized for capability)
    pub fn is_pro_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25Pro
                | ModelId::GPT5
                | ModelId::GPT52
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus41
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::ZaiGlm47
                | ModelId::ZaiGlm47DeepThinking
                | ModelId::ZaiGlm46
                | ModelId::ZaiGlm46DeepThinking
                | ModelId::ZaiGlm45DeepThinking
                | ModelId::MinimaxM21
                | ModelId::OllamaGlm47Cloud
                | ModelId::OllamaMinimaxM21Cloud
                | ModelId::MoonshotKimiK25
        )
    }

    /// Check if this is an optimized/efficient variant
    pub fn is_efficient_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.efficient;
        }
        matches!(
            self,
            ModelId::Gemini25FlashPreview
                | ModelId::Gemini25Flash
                | ModelId::Gemini25FlashLite
                | ModelId::Gemini3FlashPreview
                | ModelId::GPT5Mini
                | ModelId::GPT5Nano
                | ModelId::ClaudeHaiku45
                | ModelId::ClaudeHaiku35
                | ModelId::DeepSeekChat
                | ModelId::XaiGrok4Code
                | ModelId::ZaiGlm45Air
                | ModelId::ZaiGlm45Airx
                | ModelId::ZaiGlm45Flash
        )
    }

    /// Check if this is a top-tier model
    pub fn is_top_tier(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.top_tier;
        }
        matches!(
            self,
            ModelId::Gemini25Pro
                | ModelId::Gemini3FlashPreview
                | ModelId::GPT52
                | ModelId::GPT5
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus45
                | ModelId::ClaudeOpus41
                | ModelId::ClaudeOpus4
                | ModelId::ClaudeSonnet45
                | ModelId::ClaudeSonnet4
                | ModelId::ClaudeSonnet37
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::XaiGrok4CodeLatest
                | ModelId::ZaiGlm47
                | ModelId::ZaiGlm47DeepThinking
                | ModelId::ZaiGlm46
                | ModelId::ZaiGlm46DeepThinking
                | ModelId::MoonshotKimiK25
        )
    }

    /// Determine whether the model is a reasoning-capable variant
    pub fn is_reasoning_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.reasoning;
        }
        self.provider().supports_reasoning_effort(self.as_str())
    }

    /// Determine whether the model supports tool calls/function execution
    pub fn supports_tool_calls(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.tool_call;
        }
        true
    }

    /// Get the generation/version string for this model
    pub fn generation(&self) -> &'static str {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.generation;
        }
        match self {
            // Gemini generations
            ModelId::Gemini25FlashPreview
            | ModelId::Gemini25Flash
            | ModelId::Gemini25FlashLite
            | ModelId::Gemini25Pro => "2.5",
            ModelId::Gemini3ProPreview | ModelId::Gemini3FlashPreview => "3",
            // OpenAI generations
            ModelId::GPT52 | ModelId::GPT52Codex => "5.2",
            ModelId::GPT5
            | ModelId::GPT5Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::GPT51
            | ModelId::GPT51Codex
            | ModelId::GPT51CodexMax
            | ModelId::GPT51Mini
            | ModelId::CodexMiniLatest
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => "5",
            // Anthropic generations
            ModelId::ClaudeOpus45 | ModelId::ClaudeSonnet45 | ModelId::ClaudeHaiku45 => "4.5",
            ModelId::ClaudeOpus41 => "4.1",
            ModelId::ClaudeOpus4 | ModelId::ClaudeSonnet4 => "4",
            ModelId::ClaudeSonnet37 => "3.7",
            ModelId::ClaudeHaiku35 => "3.5",
            // DeepSeek generations
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => "V3.2-Exp",
            // Hugging Face generations
            ModelId::HuggingFaceDeepseekV32 => "V3.2-Exp",
            ModelId::HuggingFaceOpenAIGptOss20b => "oss",
            ModelId::HuggingFaceOpenAIGptOss120b => "oss",
            ModelId::HuggingFaceGlm47
            | ModelId::HuggingFaceGlm47Novita
            | ModelId::HuggingFaceGlm47FlashNovita => "4.7",
            ModelId::HuggingFaceKimiK2Thinking => "k2",
            ModelId::HuggingFaceKimiK25Novita => "k2.5",
            ModelId::HuggingFaceMinimaxM21Novita => "m2.1",
            ModelId::HuggingFaceDeepseekV32Novita => "v3.2",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "v2-flash",
            // xAI generations
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => "4",
            // Z.AI generations
            ModelId::ZaiGlm47 | ModelId::ZaiGlm47DeepThinking | ModelId::ZaiGlm47Flash => "4.7",
            ModelId::ZaiGlm46 | ModelId::ZaiGlm46DeepThinking => "4.6",
            ModelId::ZaiGlm45
            | ModelId::ZaiGlm45DeepThinking
            | ModelId::ZaiGlm45Air
            | ModelId::ZaiGlm45X
            | ModelId::ZaiGlm45Airx
            | ModelId::ZaiGlm45Flash => "4.5",
            ModelId::ZaiGlm432b0414128k => "4-32B",
            // Moonshot generations
            ModelId::MoonshotKimiK25 => "K2.5",
            ModelId::OllamaGptOss20b => "oss",
            ModelId::OllamaGptOss20bCloud => "oss-cloud",
            ModelId::OllamaGptOss120bCloud => "oss-cloud",
            ModelId::OllamaQwen317b => "oss",
            ModelId::OllamaDeepseekV32Cloud => "deepseek-v3.2",
            ModelId::OllamaQwen3Next80bCloud => "qwen3-next",
            ModelId::OllamaMistralLarge3675bCloud => "mistral-large-3",
            ModelId::OllamaKimiK2ThinkingCloud => "kimi-k2-thinking",
            ModelId::OllamaKimiK25Cloud => "kimi-k2.5",

            ModelId::OllamaQwen3Coder480bCloud => "qwen3",
            ModelId::OllamaGlm46Cloud => "glm-4.6",
            ModelId::OllamaGemini3ProPreviewLatestCloud => "gemini-3",
            ModelId::OllamaDevstral2123bCloud => "devstral-2",
            ModelId::OllamaMinimaxM2Cloud => "minimax-m2",
            ModelId::OllamaNemotron3Nano30bCloud => "nemotron-3",
            ModelId::OllamaGlm47Cloud => "glm-4.7",
            ModelId::OllamaMinimaxM21Cloud => "minimax-m2.1",
            ModelId::OllamaGemini3FlashPreviewCloud => "gemini-3",
            ModelId::LmStudioMetaLlama38BInstruct => "meta-llama-3",
            ModelId::LmStudioMetaLlama318BInstruct => "meta-llama-3.1",
            ModelId::LmStudioQwen257BInstruct => "qwen2.5",
            ModelId::LmStudioGemma22BIt => "gemma-2",
            ModelId::LmStudioGemma29BIt => "gemma-2",
            ModelId::LmStudioPhi31Mini4kInstruct => "phi-3.1",
            // MiniMax models
            ModelId::MinimaxM21 | ModelId::MinimaxM21Lightning => "M2.1",
            ModelId::MinimaxM2 => "m2",
            // OpenRouter models - fallback for any OpenRouter model without metadata
            ModelId::OpenRouterGrokCodeFast1
            | ModelId::OpenRouterGrok4Fast
            | ModelId::OpenRouterGrok41Fast
            | ModelId::OpenRouterGrok4
            | ModelId::OpenRouterZaiGlm46
            | ModelId::OpenRouterMoonshotaiKimiK20905
            | ModelId::OpenRouterMoonshotaiKimiK2Thinking
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
            | ModelId::OpenRouterQwen3Coder
            | ModelId::OpenRouterQwen3CoderPlus
            | ModelId::OpenRouterQwen3CoderFlash
            | ModelId::OpenRouterQwen3Coder30bA3bInstruct
            | ModelId::OpenRouterDeepSeekV32Exp
            | ModelId::OpenRouterDeepSeekChatV31
            | ModelId::OpenRouterDeepSeekR1
            | ModelId::OpenRouterOpenAIGptOss120b
            | ModelId::OpenRouterOpenAIGptOss120bFree
            | ModelId::OpenRouterOpenAIGptOss20b
            | ModelId::OpenRouterOpenAIGpt5
            | ModelId::OpenRouterOpenAIGpt5Codex
            | ModelId::OpenRouterOpenAIGpt5Chat
            | ModelId::OpenRouterAnthropicClaudeSonnet45
            | ModelId::OpenRouterAnthropicClaudeHaiku45
            | ModelId::OpenRouterAnthropicClaudeOpus41
            | ModelId::OpenRouterDeepseekChat
            | ModelId::OpenRouterDeepSeekV32
            | ModelId::OpenRouterDeepseekReasoner
            | ModelId::OpenRouterDeepSeekV32Speciale
            | ModelId::OpenRouterAmazonNova2LiteV1
            | ModelId::OpenRouterMistralaiMistralLarge2512
            | ModelId::OpenRouterNexAgiDeepseekV31NexN1
            | ModelId::OpenRouterOpenAIGpt51
            | ModelId::OpenRouterOpenAIGpt51Codex
            | ModelId::OpenRouterOpenAIGpt51CodexMax
            | ModelId::OpenRouterOpenAIGpt51CodexMini
            | ModelId::OpenRouterOpenAIGpt51Chat
            | ModelId::OpenRouterOpenAIGpt52
            | ModelId::OpenRouterOpenAIGpt52Chat
            | ModelId::OpenRouterOpenAIGpt52Codex
            | ModelId::OpenRouterOpenAIGpt52Pro
            | ModelId::OpenRouterOpenAIO1Pro
            | ModelId::OpenRouterZaiGlm46V
            | ModelId::OpenRouterZaiGlm47
            | ModelId::OpenRouterZaiGlm47Flash
            | ModelId::OpenRouterMoonshotaiKimiK25 => "unknown", // fallback generation for OpenRouter models
        }
    }

    /// Determine if this model is a GPT-5.1+ variant with enhanced tool support
    pub fn is_gpt51_variant(&self) -> bool {
        matches!(
            self,
            ModelId::GPT52
                | ModelId::GPT52Codex
                | ModelId::GPT51
                | ModelId::GPT51Codex
                | ModelId::GPT51CodexMax
                | ModelId::GPT51Mini
        )
    }

    /// Determine if this model supports GPT-5.1+/5.2 apply_patch tool type
    pub fn supports_apply_patch_tool(&self) -> bool {
        self.is_gpt51_variant()
    }

    /// Determine if this model supports GPT-5.1+/5.2 shell tool type
    pub fn supports_shell_tool(&self) -> bool {
        self.is_gpt51_variant()
    }
}

impl Default for ModelId {
    fn default() -> Self {
        Self::default_model()
    }
}
