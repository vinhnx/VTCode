use super::ModelId;

impl ModelId {
    /// Check if this is a "flash" variant (optimized for speed)
    pub fn is_flash_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini25FlashPreview
                | ModelId::Gemini25Flash
                | ModelId::Gemini25FlashLite
                | ModelId::ZaiGlm45Flash
                | ModelId::ZaiGlm46VFlash
                | ModelId::ZaiGlm46VFlashX
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
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeOpus41
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::ZaiGlm4Plus
                | ModelId::ZaiGlm4PlusDeepThinking
                | ModelId::ZaiGlm47
                | ModelId::ZaiGlm47DeepThinking
                | ModelId::ZaiGlm46
                | ModelId::ZaiGlm46DeepThinking
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
                | ModelId::GPT5Mini
                | ModelId::GPT5Nano
                | ModelId::ClaudeHaiku45
                | ModelId::DeepSeekChat
                | ModelId::XaiGrok4Code
                | ModelId::ZaiGlm45Air
                | ModelId::ZaiGlm45Airx
                | ModelId::ZaiGlm45Flash
                | ModelId::ZaiGlm46VFlash
                | ModelId::ZaiGlm46VFlashX
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
                | ModelId::GPT5
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeOpus41
                | ModelId::ClaudeSonnet45
                | ModelId::ClaudeSonnet4
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::XaiGrok4CodeLatest
                | ModelId::ZaiGlm4Plus
                | ModelId::ZaiGlm4PlusDeepThinking
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
        matches!(
            self,
            ModelId::ZaiGlm4PlusDeepThinking
                | ModelId::ZaiGlm47DeepThinking
                | ModelId::ZaiGlm46DeepThinking
                | ModelId::ZaiGlm45DeepThinking
        ) || self.provider().supports_reasoning_effort(self.as_str())
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
            ModelId::Gemini3ProPreview => "3",
            // OpenAI generations
            ModelId::GPT5
            | ModelId::GPT5Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::CodexMiniLatest => "5",
            // Anthropic generations
            ModelId::ClaudeOpus46 => "4.6",
            ModelId::ClaudeOpus45 | ModelId::ClaudeSonnet45 | ModelId::ClaudeHaiku45 => "4.5",
            ModelId::ClaudeOpus41 => "4.1",
            ModelId::ClaudeSonnet4 => "4",
            // DeepSeek generations
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => "V3.2-Exp",
            // xAI generations
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => "4",
            // Z.AI generations
            ModelId::ZaiGlm4Plus | ModelId::ZaiGlm4PlusDeepThinking => "4-Plus",
            ModelId::ZaiGlm47 | ModelId::ZaiGlm47DeepThinking => "4.7",
            ModelId::ZaiGlm46 | ModelId::ZaiGlm46DeepThinking => "4.6",
            ModelId::ZaiGlm46V | ModelId::ZaiGlm46VFlash | ModelId::ZaiGlm46VFlashX => "4.6",
            ModelId::ZaiGlm45
            | ModelId::ZaiGlm45DeepThinking
            | ModelId::ZaiGlm45Air
            | ModelId::ZaiGlm45X
            | ModelId::ZaiGlm45Airx
            | ModelId::ZaiGlm45Flash
            | ModelId::ZaiGlm45V => "4.5",
            ModelId::ZaiGlm432b0414128k => "4-32B",
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
            ModelId::OllamaQwen3Coder480bCloud => "qwen3-coder-cloud",
            ModelId::OllamaGlm46Cloud => "glm-cloud",
            ModelId::OllamaMinimaxM2Cloud => "minimax-cloud",
            ModelId::LmStudioMetaLlama38BInstruct => "meta-llama-3",
            ModelId::LmStudioMetaLlama318BInstruct => "meta-llama-3.1",
            ModelId::LmStudioQwen257BInstruct => "qwen2.5",
            ModelId::LmStudioGemma22BIt => "gemma-2",
            ModelId::LmStudioGemma29BIt => "gemma-2",
            ModelId::LmStudioPhi31Mini4kInstruct => "phi-3.1",
            ModelId::MinimaxM21 | ModelId::MinimaxM21Lightning => "M2.1",
            ModelId::MinimaxM2 => "M2",
            ModelId::HuggingFaceDeepseekV32 | ModelId::HuggingFaceDeepseekV32Novita => "v3.2",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "v2-flash",
            ModelId::HuggingFaceQwen3CoderNextNovita => "qwen3-coder-next",
            ModelId::HuggingFaceGlm47 => "4.7",
            ModelId::HuggingFaceKimiK2Thinking => "k2",
            ModelId::HuggingFaceKimiK25Novita => "k2.5",
            ModelId::HuggingFaceMinimaxM21Novita => "m2.1",
            ModelId::HuggingFaceOpenAIGptOss20b | ModelId::HuggingFaceOpenAIGptOss120b => "oss",
            _ => unreachable!(),
        }
    }
}
