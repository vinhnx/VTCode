use super::ModelId;

impl ModelId {
    /// Check if this is a "flash" variant (optimized for speed)
    pub fn is_flash_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini3FlashPreview | ModelId::OllamaGemini3FlashPreviewCloud
        )
    }

    /// Check if this is a "pro" variant (optimized for capability)
    pub fn is_pro_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini31ProPreview
                | ModelId::Gemini31ProPreviewCustomTools
                | ModelId::GPT5
                | ModelId::GPT52
                | ModelId::GPT53Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeSonnet46
                | ModelId::ClaudeOpus41
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::ZaiGlm5
                | ModelId::MinimaxM25
                | ModelId::OllamaGlm5Cloud
                | ModelId::OllamaMinimaxM25Cloud
        )
    }

    /// Check if this is an optimized/efficient variant
    pub fn is_efficient_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.efficient;
        }
        matches!(
            self,
            ModelId::Gemini3FlashPreview
                | ModelId::GPT5Mini
                | ModelId::GPT5Nano
                | ModelId::ClaudeHaiku45
                | ModelId::ClaudeHaiku35
                | ModelId::DeepSeekChat
                | ModelId::XaiGrok4Code
        )
    }

    /// Check if this is a top-tier model
    pub fn is_top_tier(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.top_tier;
        }
        matches!(
            self,
            ModelId::Gemini31ProPreview
                | ModelId::Gemini31ProPreviewCustomTools
                | ModelId::Gemini3FlashPreview
                | ModelId::GPT5
                | ModelId::GPT52
                | ModelId::GPT53Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeSonnet46
                | ModelId::ClaudeOpus45
                | ModelId::ClaudeOpus41
                | ModelId::ClaudeOpus4
                | ModelId::ClaudeSonnet45
                | ModelId::ClaudeSonnet4
                | ModelId::ClaudeSonnet37
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::XaiGrok4CodeLatest
                | ModelId::ZaiGlm5
        )
    }

    /// Determine whether the model is a reasoning-capable variant
    pub fn is_reasoning_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.reasoning;
        }
        matches!(self, ModelId::ZaiGlm5) || self.provider().supports_reasoning_effort(self.as_str())
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
            ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => "3.1",
            ModelId::Gemini3ProPreview | ModelId::Gemini3FlashPreview => "3",
            // OpenAI generations
            ModelId::GPT52 | ModelId::GPT52Codex => "5.2",
            ModelId::GPT53Codex => "5.3",
            ModelId::GPT5
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => "5",
            // Anthropic generations
            ModelId::ClaudeOpus46 | ModelId::ClaudeSonnet46 => "4.6",
            ModelId::ClaudeOpus45 | ModelId::ClaudeSonnet45 | ModelId::ClaudeHaiku45 => "4.5",
            ModelId::ClaudeOpus41 => "4.1",
            ModelId::ClaudeOpus4 | ModelId::ClaudeSonnet4 => "4",
            ModelId::ClaudeSonnet37 => "3.7",
            ModelId::ClaudeHaiku35 => "3.5",
            // DeepSeek generations
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => "V3.2-Exp",
            // xAI generations
            ModelId::XaiGrok4
            | ModelId::XaiGrok4Mini
            | ModelId::XaiGrok4Code
            | ModelId::XaiGrok4CodeLatest
            | ModelId::XaiGrok4Vision => "4",
            // Z.AI generations
            ModelId::ZaiGlm5 => "GLM-5",
            ModelId::OllamaGptOss20b => "oss",
            ModelId::OllamaGptOss20bCloud => "oss-cloud",
            ModelId::OllamaGptOss120bCloud => "oss-cloud",
            ModelId::OllamaQwen317b => "oss",
            ModelId::OllamaDeepseekV32Cloud => "deepseek-v3.2",
            ModelId::OllamaQwen3Next80bCloud => "qwen3-next",
            ModelId::OllamaMistralLarge3675bCloud => "mistral-large-3",
            ModelId::OllamaQwen3Coder480bCloud => "qwen3-coder-cloud",
            ModelId::OllamaMinimaxM2Cloud => "minimax-cloud",
            ModelId::OllamaMinimaxM25Cloud => "minimax-cloud",
            ModelId::OllamaGlm5Cloud => "glm-5-cloud",
            ModelId::OllamaGemini3ProPreviewLatestCloud => "gemini-3-pro-cloud",
            ModelId::OllamaGemini3FlashPreviewCloud => "gemini-3-flash-cloud",
            ModelId::OllamaNemotron3Nano30bCloud => "nemotron-cloud",
            ModelId::OllamaDevstral2123bCloud => "devstral-cloud",
            ModelId::LmStudioMetaLlama38BInstruct => "meta-llama-3",
            ModelId::LmStudioMetaLlama318BInstruct => "meta-llama-3.1",
            ModelId::LmStudioQwen257BInstruct => "qwen2.5",
            ModelId::LmStudioGemma22BIt => "gemma-2",
            ModelId::LmStudioGemma29BIt => "gemma-2",
            ModelId::LmStudioPhi31Mini4kInstruct => "phi-3.1",
            ModelId::MinimaxM25
            | ModelId::HuggingFaceMinimaxM25Novita
            | ModelId::MoonshotMinimaxM25
            | ModelId::OpenRouterMinimaxM25 => "M2.5",
            ModelId::MinimaxM2 => "M2",
            ModelId::HuggingFaceDeepseekV32 | ModelId::HuggingFaceDeepseekV32Novita => "v3.2",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "v2-flash",
            ModelId::HuggingFaceQwen3CoderNextNovita
            | ModelId::OpenRouterQwen3CoderNext
            | ModelId::MoonshotQwen3CoderNext => "qwen3-coder-next",
            ModelId::HuggingFaceGlm5Novita => "GLM-5",
            ModelId::HuggingFaceOpenAIGptOss20b | ModelId::HuggingFaceOpenAIGptOss120b => "oss",
            _ => unreachable!(),
        }
    }
}
