use crate::models::Provider;

use super::ModelId;

#[cfg(not(docsrs))]
#[allow(dead_code)]
mod capability_generated {
    include!(concat!(env!("OUT_DIR"), "/model_capabilities.rs"));
}

#[cfg(docsrs)]
#[allow(dead_code)]
mod capability_generated {
    #[derive(Clone, Copy)]
    pub struct Entry {
        pub provider: &'static str,
        pub id: &'static str,
        pub context_window: usize,
        pub tool_call: bool,
        pub input_modalities: &'static [&'static str],
    }

    pub const ENTRIES: &[Entry] = &[];
    pub const PROVIDERS: &[&str] = &[];

    pub fn metadata_for(_provider: &str, _id: &str) -> Option<Entry> {
        None
    }

    pub fn models_for_provider(_provider: &str) -> Option<&'static [&'static str]> {
        None
    }
}

/// Catalog metadata generated from `docs/models.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelCatalogEntry {
    pub provider: &'static str,
    pub id: &'static str,
    pub context_window: usize,
    pub tool_call: bool,
    pub input_modalities: &'static [&'static str],
}

fn catalog_provider_key(provider: &str) -> &str {
    if provider.eq_ignore_ascii_case("google") || provider.eq_ignore_ascii_case("gemini") {
        "gemini"
    } else if provider.eq_ignore_ascii_case("openai") {
        "openai"
    } else if provider.eq_ignore_ascii_case("anthropic") {
        "anthropic"
    } else if provider.eq_ignore_ascii_case("deepseek") {
        "deepseek"
    } else if provider.eq_ignore_ascii_case("openrouter") {
        "openrouter"
    } else if provider.eq_ignore_ascii_case("ollama") {
        "ollama"
    } else if provider.eq_ignore_ascii_case("lmstudio") {
        "lmstudio"
    } else if provider.eq_ignore_ascii_case("moonshot") {
        "moonshot"
    } else if provider.eq_ignore_ascii_case("zai") {
        "zai"
    } else if provider.eq_ignore_ascii_case("minimax") {
        "minimax"
    } else if provider.eq_ignore_ascii_case("huggingface") {
        "huggingface"
    } else if provider.eq_ignore_ascii_case("litellm") {
        "litellm"
    } else {
        provider
    }
}

fn capability_provider_key(provider: Provider) -> &'static str {
    match provider {
        Provider::Gemini => "gemini",
        Provider::OpenAI => "openai",
        Provider::Anthropic => "anthropic",
        Provider::DeepSeek => "deepseek",
        Provider::OpenRouter => "openrouter",
        Provider::Ollama => "ollama",
        Provider::LmStudio => "lmstudio",
        Provider::Moonshot => "moonshot",
        Provider::ZAI => "zai",
        Provider::Minimax => "minimax",
        Provider::HuggingFace => "huggingface",
        Provider::LiteLLM => "litellm",
    }
}

fn generated_catalog_entry(provider: &str, id: &str) -> Option<ModelCatalogEntry> {
    capability_generated::metadata_for(catalog_provider_key(provider), id).map(|entry| {
        ModelCatalogEntry {
            provider: entry.provider,
            id: entry.id,
            context_window: entry.context_window,
            tool_call: entry.tool_call,
            input_modalities: entry.input_modalities,
        }
    })
}

pub fn model_catalog_entry(provider: &str, id: &str) -> Option<ModelCatalogEntry> {
    generated_catalog_entry(provider, id)
}

pub fn supported_models_for_provider(provider: &str) -> Option<&'static [&'static str]> {
    capability_generated::models_for_provider(catalog_provider_key(provider))
}

pub fn catalog_provider_keys() -> &'static [&'static str] {
    capability_generated::PROVIDERS
}

impl ModelId {
    fn generated_capabilities(&self) -> Option<ModelCatalogEntry> {
        generated_catalog_entry(capability_provider_key(self.provider()), self.as_str())
    }

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
            ModelId::Gemini31ProPreview
            | ModelId::Gemini31ProPreviewCustomTools
            | ModelId::Gemini31FlashLitePreview => Some(ModelId::Gemini3FlashPreview),
            ModelId::GPT52
            | ModelId::GPT54
            | ModelId::GPT54Pro
            | ModelId::GPT54Nano
            | ModelId::GPT54Mini
            | ModelId::GPT5 => Some(ModelId::GPT5Mini),
            ModelId::DeepSeekReasoner => Some(ModelId::DeepSeekChat),
            ModelId::ZaiGlm5 => Some(ModelId::OllamaGlm5Cloud),
            ModelId::ClaudeOpus46 | ModelId::ClaudeSonnet46 => Some(ModelId::ClaudeSonnet46),
            ModelId::MinimaxM27 | ModelId::MinimaxM25 => None,
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
                | ModelId::Gemini31FlashLitePreview
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree
                | ModelId::OllamaGemini3FlashPreviewCloud
                | ModelId::HuggingFaceStep35Flash
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
                | ModelId::GPT52Codex
                | ModelId::GPT54
                | ModelId::GPT54Pro
                | ModelId::GPT53Codex
                | ModelId::GPT51Codex
                | ModelId::GPT51CodexMax
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeSonnet46
                | ModelId::DeepSeekReasoner
                | ModelId::ZaiGlm5
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree
                | ModelId::MinimaxM27
                | ModelId::MinimaxM25
                | ModelId::OllamaGlm5Cloud
                | ModelId::OllamaNemotron3SuperCloud
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
                | ModelId::Gemini31FlashLitePreview
                | ModelId::GPT5Mini
                | ModelId::GPT5Nano
                | ModelId::ClaudeHaiku45
                | ModelId::DeepSeekChat
                | ModelId::HuggingFaceStep35Flash
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
                | ModelId::Gemini31FlashLitePreview
                | ModelId::GPT5
                | ModelId::GPT52
                | ModelId::GPT52Codex
                | ModelId::GPT54
                | ModelId::GPT54Pro
                | ModelId::GPT53Codex
                | ModelId::GPT51Codex
                | ModelId::GPT51CodexMax
                | ModelId::GPT5Codex
                | ModelId::ClaudeOpus46
                | ModelId::ClaudeSonnet46
                | ModelId::DeepSeekReasoner
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
        if let Some(meta) = self.generated_capabilities() {
            return meta.tool_call;
        }
        if let Some(meta) = self.openrouter_metadata() {
            return meta.tool_call;
        }
        true
    }

    /// Ordered list of supported input modalities when VT Code has metadata for this model.
    pub fn input_modalities(&self) -> &'static [&'static str] {
        self.generated_capabilities()
            .map(|meta| meta.input_modalities)
            .unwrap_or(&[])
    }

    /// Get the generation/version string for this model
    pub fn generation(&self) -> &'static str {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.generation;
        }
        match self {
            // Gemini generations
            ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => "3.1",
            ModelId::Gemini31FlashLitePreview => "3.1-lite",
            ModelId::Gemini3FlashPreview => "3",
            // OpenAI generations
            ModelId::GPT52 | ModelId::GPT52Codex => "5.2",
            ModelId::GPT54 | ModelId::GPT54Pro | ModelId::GPT54Nano | ModelId::GPT54Mini => "5.4",
            ModelId::GPT53Codex => "5.3",
            ModelId::GPT51Codex | ModelId::GPT51CodexMax => "5.1",
            ModelId::GPT5
            | ModelId::GPT5Codex
            | ModelId::GPT5Mini
            | ModelId::GPT5Nano
            | ModelId::OpenAIGptOss20b
            | ModelId::OpenAIGptOss120b => "5",
            // Anthropic generations
            ModelId::ClaudeOpus46 | ModelId::ClaudeSonnet46 => "4.6",
            ModelId::ClaudeHaiku45 => "4.5",
            // DeepSeek generations
            ModelId::DeepSeekChat | ModelId::DeepSeekReasoner => "V3.2-Exp",
            // Z.AI generations
            ModelId::ZaiGlm5 => "5",
            ModelId::OllamaGptOss20b => "oss",
            ModelId::OllamaGptOss20bCloud => "oss-cloud",
            ModelId::OllamaGptOss120bCloud => "oss-cloud",
            ModelId::OllamaQwen317b => "oss",
            ModelId::OllamaQwen3CoderNext => "qwen3-coder-next:cloud",
            ModelId::OllamaDeepseekV32Cloud => "deepseek-v3.2",
            ModelId::OllamaQwen3Next80bCloud => "qwen3-next",
            ModelId::OllamaMinimaxM2Cloud => "minimax-m2",
            ModelId::OllamaGlm5Cloud => "glm-5",
            ModelId::OllamaMinimaxM25Cloud => "minimax-m2.5",
            ModelId::OllamaNemotron3SuperCloud => "nemotron-3",
            ModelId::OllamaGemini3FlashPreviewCloud => "gemini-3",
            // MiniMax models
            ModelId::MinimaxM27 => "M2.7",
            ModelId::MinimaxM25 => "M2.5",
            // Moonshot models
            ModelId::MoonshotKimiK25 => "k2.5",
            // Hugging Face generations
            ModelId::HuggingFaceDeepseekV32 => "V3.2-Exp",
            ModelId::HuggingFaceOpenAIGptOss20b => "oss",
            ModelId::HuggingFaceOpenAIGptOss120b => "oss",
            ModelId::HuggingFaceMinimaxM25Novita => "m2.5",
            ModelId::HuggingFaceDeepseekV32Novita => "v3.2",
            ModelId::HuggingFaceXiaomiMimoV2FlashNovita => "v2-flash",
            ModelId::HuggingFaceGlm5Novita => "5",
            ModelId::HuggingFaceStep35Flash => "3.5",
            ModelId::HuggingFaceQwen3CoderNextNovita | ModelId::OpenRouterQwen3CoderNext => {
                "qwen3-coder-next"
            }
            _ => "unknown",
        }
    }

    /// Determine if this model supports GPT-5.1+/5.2+/5.3+ shell tool type
    pub fn supports_shell_tool(&self) -> bool {
        matches!(
            self,
            ModelId::GPT52
                | ModelId::GPT52Codex
                | ModelId::GPT54
                | ModelId::GPT54Pro
                | ModelId::GPT53Codex
                | ModelId::GPT51Codex
                | ModelId::GPT51CodexMax
                | ModelId::GPT5Codex
        )
    }

    /// Determine if this model supports optimized apply_patch tool
    pub fn supports_apply_patch_tool(&self) -> bool {
        false // Placeholder for future optimization
    }
}
