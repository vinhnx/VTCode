use super::ModelId;

impl ModelId {
    /// Attempt to find a non-reasoning variant for this model.
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
                candidate
                    .openrouter_metadata()
                    .map(|data| (!data.efficient, data.display))
                    .unwrap_or((true, ""))
            });

            return candidates.into_iter().next();
        }

        let direct = match self {
            ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => {
                Some(ModelId::Gemini3FlashPreview)
            }
            ModelId::GPT52 | ModelId::GPT5 => Some(ModelId::GPT5Mini),
            ModelId::DeepSeekReasoner => Some(ModelId::DeepSeekChat),
            ModelId::XaiGrok4 => Some(ModelId::XaiGrok4Mini),
            ModelId::XaiGrok4Code => Some(ModelId::XaiGrok4CodeLatest),
            ModelId::ZaiGlm5 => Some(ModelId::OllamaGlm5Cloud),
            ModelId::ClaudeOpus46
            | ModelId::ClaudeSonnet46
            | ModelId::ClaudeOpus45
            | ModelId::ClaudeOpus41 => Some(ModelId::ClaudeSonnet45),
            ModelId::ClaudeSonnet4 => Some(ModelId::ClaudeSonnet45),
            ModelId::MinimaxM25 => Some(ModelId::MinimaxM2),
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
            ModelId::Gemini3FlashPreview
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::OllamaGemini3FlashPreviewCloud
        )
    }

    /// Check if this is a "pro" variant (optimized for capability)
    pub fn is_pro_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini31ProPreview
                | ModelId::Gemini31ProPreviewCustomTools
                | ModelId::OpenRouterGoogleGemini31ProPreview
                | ModelId::GPT5
                | ModelId::GPT52
                | ModelId::GPT53Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeSonnet46
                | ModelId::ClaudeOpus41
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::ZaiGlm5
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::MinimaxM25
                | ModelId::OllamaGlm5Cloud
                | ModelId::OllamaMinimaxM25Cloud
                | ModelId::HuggingFaceQwen3CoderNextNovita
                | ModelId::HuggingFaceQwen35397BA17BTogether
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
                | ModelId::OpenRouterGoogleGemini31ProPreview
                | ModelId::Gemini3FlashPreview
                | ModelId::GPT5
                | ModelId::GPT52
                | ModelId::GPT53Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeSonnet46
                | ModelId::ClaudeOpus45
                | ModelId::ClaudeOpus41
                | ModelId::ClaudeSonnet45
                | ModelId::ClaudeSonnet4
                | ModelId::DeepSeekReasoner
                | ModelId::XaiGrok4
                | ModelId::XaiGrok4CodeLatest
                | ModelId::ZaiGlm5
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::HuggingFaceQwen3CoderNextNovita
                | ModelId::HuggingFaceQwen35397BA17BTogether
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
            ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => "3.1",
            ModelId::Gemini3FlashPreview => "3",
            // OpenAI generations
            ModelId::GPT52 => "5.2",
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
            ModelId::ZaiGlm5 => "5",
            ModelId::OllamaGptOss20b => "oss",
            ModelId::OllamaGptOss20bCloud => "oss-cloud",
            ModelId::OllamaGptOss120bCloud => "oss-cloud",
            ModelId::OllamaQwen317b => "oss",
            ModelId::OllamaDeepseekV32Cloud => "deepseek-v3.2",
            ModelId::OllamaQwen3Next80bCloud => "qwen3-next",
            ModelId::OllamaMistralLarge3675bCloud => "mistral-large-3",
            ModelId::OllamaQwen3Coder480bCloud => "qwen3",
            ModelId::OllamaDevstral2123bCloud => "devstral-2",
            ModelId::OllamaMinimaxM2Cloud => "minimax-m2",
            ModelId::OllamaNemotron3Nano30bCloud => "nemotron-3",
            ModelId::OllamaGlm5Cloud => "glm-5",
            ModelId::OllamaMinimaxM25Cloud => "minimax-m2.5",
            ModelId::OllamaGemini3FlashPreviewCloud => "gemini-3",
            // MiniMax models
            ModelId::MinimaxM25 => "M2.5",
            ModelId::MinimaxM2 => "m2",
            // Moonshot models
            ModelId::MoonshotMinimaxM25 | ModelId::OpenRouterMinimaxM25 => "M2.5",
            // Hugging Face generations
            ModelId::HuggingFaceDeepseekV32 => "V3.2-Exp",
            ModelId::HuggingFaceOpenAIGptOss20b => "oss",
            ModelId::HuggingFaceOpenAIGptOss120b => "oss",
            ModelId::HuggingFaceMinimaxM25Novita => "m2.5",
            ModelId::HuggingFaceDeepseekV32Novita => "v3.2",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "v2-flash",
            ModelId::HuggingFaceGlm5Novita => "5",
            ModelId::HuggingFaceQwen3CoderNextNovita
            | ModelId::OpenRouterQwen3CoderNext
            | ModelId::MoonshotQwen3CoderNext => "qwen3-coder-next",
            _ => unreachable!(),
        }
    }

    /// Determine if this model supports GPT-5.1+/5.2+/5.3+ shell tool type
    pub fn supports_shell_tool(&self) -> bool {
        matches!(self, ModelId::GPT52 | ModelId::GPT53Codex)
    }
}
