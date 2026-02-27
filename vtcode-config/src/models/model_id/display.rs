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
            ModelId::Gemini3FlashPreview => "Gemini 3 Flash",
            // OpenAI models
            ModelId::GPT5 => "GPT-5",
            ModelId::GPT52 => "GPT-5.2",
            ModelId::GPT5Mini => "GPT-5 Mini",
            ModelId::GPT5Nano => "GPT-5 Nano",
            ModelId::GPT53Codex => "GPT-5.3 Codex",
            ModelId::OpenAIGptOss20b => "GPT-OSS 20B",
            ModelId::OpenAIGptOss120b => "GPT-OSS 120B",
            // Anthropic models
            ModelId::ClaudeOpus46 => "Claude Opus 4.6",
            ModelId::ClaudeSonnet46 => "Claude Sonnet 4.6",
            ModelId::ClaudeOpus45 => "Claude Opus 4.5",
            ModelId::ClaudeOpus41 => "Claude Opus 4.1",
            ModelId::ClaudeSonnet45 => "Claude Sonnet 4.5",
            ModelId::ClaudeHaiku45 => "Claude Haiku 4.5",
            ModelId::ClaudeSonnet4 => "Claude Sonnet 4",
            // DeepSeek models
            ModelId::DeepSeekChat => "DeepSeek V3.2 Chat",
            ModelId::DeepSeekReasoner => "DeepSeek V3.2 Reasoner",
            // Z.AI models
            ModelId::ZaiGlm5 => "GLM 5",
            // Ollama models
            ModelId::OllamaGptOss20b => "GPT-OSS 20B (local)",
            ModelId::OllamaGptOss20bCloud => "GPT-OSS 20B (cloud)",
            ModelId::OllamaGptOss120bCloud => "GPT-OSS 120B (cloud)",
            ModelId::OllamaQwen317b => "Qwen3 1.7B (local)",
            ModelId::OllamaDeepseekV32Cloud => "DeepSeek V3.2 (cloud)",
            ModelId::OllamaQwen3Next80bCloud => "Qwen3 Next 80B (cloud)",
            ModelId::OllamaMistralLarge3675bCloud => "Mistral Large 3 675B (cloud)",
            ModelId::OllamaQwen3Coder480bCloud => "Qwen3 Coder 480B (cloud)",
            ModelId::OllamaGemini3FlashPreviewCloud => "Gemini 3 Flash Preview (cloud)",
            ModelId::OllamaDevstral2123bCloud => "Devstral 2 123B (cloud)",
            ModelId::OllamaMinimaxM2Cloud => "MiniMax-M2 (cloud)",
            ModelId::OllamaGlm5Cloud => "GLM-5 (cloud)",
            ModelId::OllamaMinimaxM25Cloud => "MiniMax-M2.5 (cloud)",
            ModelId::OllamaNemotron3Nano30bCloud => "Nemotron-3-Nano 30B (cloud)",
            // Hugging Face models
            ModelId::HuggingFaceDeepseekV32 => "DeepSeek V3.2 (HF)",
            ModelId::HuggingFaceOpenAIGptOss20b => "GPT-OSS 20B (HF)",
            ModelId::HuggingFaceOpenAIGptOss120b => "GPT-OSS 120B (HF)",
            ModelId::HuggingFaceMinimaxM25Novita => "MiniMax-M2.5 (Novita)",
            ModelId::HuggingFaceDeepseekV32Novita => "DeepSeek V3.2 (Novita)",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "MiMo-V2-Flash (Novita)",
            ModelId::HuggingFaceGlm5Novita => "GLM-5 (Novita)",
            ModelId::HuggingFaceQwen3CoderNextNovita => "Qwen3-Coder-Next (Novita)",
            ModelId::HuggingFaceQwen35397BA17BTogether => "Qwen3.5-397B-A17B (Together)",
            ModelId::OpenRouterMinimaxM25 => "MiniMax-M2.5 (OpenRouter)",
            ModelId::OpenRouterQwen3CoderNext => "Qwen3-Coder-Next (OpenRouter)",
            // MiniMax models
            ModelId::MinimaxM25 => "MiniMax-M2.5",
            ModelId::MinimaxM2 => "MiniMax-M2",
            ModelId::MoonshotMinimaxM25 => "MiniMax-M2.5 (Moonshot)",
            ModelId::MoonshotQwen3CoderNext => "Qwen3-Coder-Next (Moonshot)",
            // OpenRouter models
            _ => unreachable!(),
        }
    }
}
