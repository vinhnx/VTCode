use super::ModelId;

impl ModelId {
    /// Get the display name for the model (human-readable)
    pub fn display_name(&self) -> &'static str {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.display;
        }
        match self {
            // Gemini models
            ModelId::Gemini25FlashPreview => "Gemini 2.5 Flash Preview",
            ModelId::Gemini25Flash => "Gemini 2.5 Flash",
            ModelId::Gemini25FlashLite => "Gemini 2.5 Flash Lite",
            ModelId::Gemini25Pro => "Gemini 2.5 Pro",
            ModelId::Gemini3ProPreview => "Gemini 3 Pro Preview",
            ModelId::Gemini3FlashPreview => "Gemini 3 Flash",
            // OpenAI models
            ModelId::GPT5 => "GPT-5",
            ModelId::GPT52 => "GPT-5.2",
            ModelId::GPT52Codex => "GPT-5.2 Codex",
            ModelId::GPT5Codex => "GPT-5 Codex",
            ModelId::GPT5Mini => "GPT-5 Mini",
            ModelId::GPT5Nano => "GPT-5 Nano",
            ModelId::GPT51 => "GPT-5.1",
            ModelId::GPT51Codex => "GPT-5.1 Codex",
            ModelId::GPT51CodexMax => "GPT-5.1 Codex Max",
            ModelId::GPT51Mini => "GPT-5.1 Mini",
            ModelId::CodexMiniLatest => "Codex Mini Latest",
            // Anthropic models
            ModelId::ClaudeOpus46 => "Claude Opus 4.6",
            ModelId::ClaudeOpus45 => "Claude Opus 4.5",
            ModelId::ClaudeOpus41 => "Claude Opus 4.1",
            ModelId::ClaudeSonnet45 => "Claude Sonnet 4.5",
            ModelId::ClaudeHaiku45 => "Claude Haiku 4.5",
            ModelId::ClaudeSonnet4 => "Claude Sonnet 4",
            ModelId::ClaudeOpus4 => "Claude Opus 4",
            ModelId::ClaudeSonnet37 => "Claude 3.7 Sonnet",
            ModelId::ClaudeHaiku35 => "Claude 3.5 Haiku",
            // DeepSeek models
            ModelId::DeepSeekChat => "DeepSeek V3.2 Chat",
            ModelId::DeepSeekReasoner => "DeepSeek V3.2 Reasoner",
            // xAI models
            ModelId::XaiGrok4 => "Grok-4",
            ModelId::XaiGrok4Mini => "Grok-4 Mini",
            ModelId::XaiGrok4Code => "Grok-4 Code",
            ModelId::XaiGrok4CodeLatest => "Grok-4 Code Latest",
            ModelId::XaiGrok4Vision => "Grok-4 Vision",
            // Z.AI models
            ModelId::ZaiGlm4Plus => "GLM 4 Plus",
            ModelId::ZaiGlm4PlusDeepThinking => "GLM 4 Plus Deep Thinking",
            ModelId::ZaiGlm47 => "GLM 4.7",
            ModelId::ZaiGlm47DeepThinking => "GLM 4.7 Deep Thinking",
            ModelId::ZaiGlm47Flash => "GLM 4.7 Flash",
            ModelId::ZaiGlm5 => "GLM 5",
            ModelId::ZaiGlm432b0414128k => "GLM 4 32B 0414 128K",
            // Moonshot models
            ModelId::MoonshotKimiK25 => "Kimi K2.5",
            // Ollama models
            ModelId::OllamaGptOss20b => "GPT-OSS 20B (local)",
            ModelId::OllamaGptOss20bCloud => "GPT-OSS 20B (cloud)",
            ModelId::OllamaGptOss120bCloud => "GPT-OSS 120B (cloud)",
            ModelId::OllamaQwen317b => "Qwen3 1.7B (local)",
            ModelId::OllamaDeepseekV32Cloud => "DeepSeek V3.2 (cloud)",
            ModelId::OllamaQwen3Next80bCloud => "Qwen3 Next 80B (cloud)",
            ModelId::OllamaMistralLarge3675bCloud => "Mistral Large 3 675B (cloud)",
            ModelId::OllamaKimiK2ThinkingCloud => "Kimi K2 Thinking (cloud)",
            ModelId::OllamaKimiK25Cloud => "Kimi K2.5 (cloud)",
            ModelId::OllamaQwen3Coder480bCloud => "Qwen3 Coder 480B (cloud)",
            ModelId::OllamaGemini3ProPreviewLatestCloud => "Gemini 3 Pro Preview (cloud)",
            ModelId::OllamaGemini3FlashPreviewCloud => "Gemini 3 Flash Preview (cloud)",
            ModelId::OllamaDevstral2123bCloud => "Devstral 2 123B (cloud)",
            ModelId::OllamaMinimaxM2Cloud => "MiniMax-M2 (cloud)",
            ModelId::OllamaGlm47Cloud => "GLM-4.7 (cloud)",
            ModelId::OllamaMinimaxM21Cloud => "MiniMax-M2.1 (cloud)",
            ModelId::OllamaNemotron3Nano30bCloud => "Nemotron-3-Nano 30B (cloud)",
            ModelId::LmStudioMetaLlama38BInstruct => "Meta Llama 3 8B (LM Studio)",
            ModelId::LmStudioMetaLlama318BInstruct => "Meta Llama 3.1 8B (LM Studio)",
            ModelId::LmStudioQwen257BInstruct => "Qwen2.5 7B (LM Studio)",
            ModelId::LmStudioGemma22BIt => "Gemma 2 2B (LM Studio)",
            ModelId::LmStudioGemma29BIt => "Gemma 2 9B (LM Studio)",
            ModelId::LmStudioPhi31Mini4kInstruct => "Phi-3.1 Mini 4K (LM Studio)",
            // Hugging Face models
            ModelId::HuggingFaceDeepseekV32 => "DeepSeek V3.2 (HF)",
            ModelId::HuggingFaceOpenAIGptOss20b => "GPT-OSS 20B (HF)",
            ModelId::HuggingFaceOpenAIGptOss120b => "GPT-OSS 120B (HF)",
            ModelId::HuggingFaceGlm47 => "GLM-4.7 (HF)",
            ModelId::HuggingFaceGlm47Novita => "GLM-4.7 (Novita)",
            ModelId::HuggingFaceGlm47FlashNovita => "GLM-4.7-Flash (Novita)",
            ModelId::HuggingFaceKimiK2Thinking => "Kimi K2 Thinking (HF)",
            ModelId::HuggingFaceKimiK25Novita => "Kimi K2.5 (Novita)",
            ModelId::HuggingFaceMinimaxM21Novita => "MiniMax-M2.1 (Novita)",
            ModelId::HuggingFaceDeepseekV32Novita => "DeepSeek V3.2 (Novita)",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "MiMo-V2-Flash (Novita)",
            ModelId::HuggingFaceGlm5Novita => "GLM-5 (Novita)",
            ModelId::HuggingFaceQwen3CoderNextNovita => "Qwen3-Coder-Next (Novita)",
            // MiniMax models
            ModelId::MinimaxM21 => "MiniMax-M2.1",
            ModelId::MinimaxM21Lightning => "MiniMax-M2.1-lightning",
            ModelId::MinimaxM2 => "MiniMax-M2",
            // OpenRouter models
            _ => unreachable!(),
        }
    }
}
