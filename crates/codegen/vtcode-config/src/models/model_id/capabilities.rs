use crate::models::{Provider, ProviderModelSupport};

use super::ModelId;

#[allow(dead_code)]
mod capability_generated {
    include!(concat!(env!("OUT_DIR"), "/model_capabilities.rs"));
}

/// Catalog metadata generated from `docs/models.json`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelPricing {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelCatalogEntry {
    pub(crate) provider: &'static str,
    id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub context_window: usize,
    max_output_tokens: Option<usize>,
    pub reasoning: bool,
    pub tool_call: bool,
    pub vision: bool,
    pub input_modalities: &'static [&'static str],
    pub caching: bool,
    pub structured_output: bool,
    pub pricing: ModelPricing,
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
    } else if provider.eq_ignore_ascii_case("llamacpp") || provider.eq_ignore_ascii_case("llama.cpp") {
        "llamacpp"
    } else if provider.eq_ignore_ascii_case("moonshot") {
        "moonshot"
    } else if provider.eq_ignore_ascii_case("zai") {
        "zai"
    } else if provider.eq_ignore_ascii_case("minimax") {
        "minimax"
    } else if provider.eq_ignore_ascii_case("huggingface") {
        "huggingface"
    } else if provider.eq_ignore_ascii_case("stepfun") {
        "stepfun"
    } else if provider.eq_ignore_ascii_case("evolink") {
        "evolink"
    } else if provider.eq_ignore_ascii_case("poolside") {
        "poolside"
    } else if provider.eq_ignore_ascii_case("xai") {
        "xai"
    } else {
        provider
    }
}

fn capability_provider_key(provider: Provider) -> &'static str {
    match provider {
        Provider::Gemini => "gemini",
        Provider::OpenAI => "openai",
        Provider::Anthropic => "anthropic",
        Provider::Copilot => "copilot",
        Provider::DeepSeek => "deepseek",
        Provider::OpenRouter => "openrouter",
        Provider::Ollama => "ollama",
        Provider::OllamaCloud => "ollama-cloud",
        Provider::LmStudio => "lmstudio",
        Provider::LlamaCpp => "llamacpp",
        Provider::Moonshot => "moonshot",
        Provider::ZAI => "zai",
        Provider::Minimax => "minimax",
        Provider::MiMo => "mimo",
        Provider::Mistral => "mistral",
        Provider::HuggingFace => "huggingface",
        Provider::OpenCodeZen => "opencode-zen",
        Provider::OpenCodeGo => "opencode-go",
        Provider::Qwen => "qwen",
        Provider::StepFun => "stepfun",
        Provider::Evolink => "evolink",
        Provider::Poolside => "poolside",
        Provider::XAI => "xai",
    }
}

fn generated_catalog_entry(provider: &str, id: &str) -> Option<ModelCatalogEntry> {
    capability_generated::metadata_for(catalog_provider_key(provider), id).map(|entry| ModelCatalogEntry {
        provider: entry.provider,
        id: entry.id,
        display_name: entry.display_name,
        description: entry.description,
        context_window: entry.context_window,
        max_output_tokens: entry.max_output_tokens,
        reasoning: entry.reasoning,
        tool_call: entry.tool_call,
        vision: entry.vision,
        input_modalities: entry.input_modalities,
        caching: entry.caching,
        structured_output: entry.structured_output,
        pricing: ModelPricing {
            input: entry.pricing.input,
            output: entry.pricing.output,
            cache_read: entry.pricing.cache_read,
            cache_write: entry.pricing.cache_write,
        },
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
        generated_catalog_entry(capability_provider_key(self.provider()), &self.as_str())
    }

    /// Preferred built-in lightweight sibling or lower-tier fallback for this model.
    pub fn preferred_lightweight_variant(&self) -> Option<Self> {
        match self {
            ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => Some(ModelId::Gemini35Flash),
            ModelId::GPT55 | ModelId::GPT54 | ModelId::GPT54Pro | ModelId::GPT53Codex => Some(ModelId::GPT54Mini),
            ModelId::GPT56Sol => Some(ModelId::GPT56Terra),
            ModelId::GPT56Terra => Some(ModelId::GPT56Luna),
            ModelId::OpenCodeZenGPT54 => Some(ModelId::OpenCodeZenGPT54Mini),
            ModelId::ClaudeSonnet5
            | ModelId::ClaudeFable5
            | ModelId::ClaudeMythos5
            | ModelId::ClaudeOpus5
            | ModelId::ClaudeOpus48
            | ModelId::ClaudeSonnet46 => Some(ModelId::ClaudeHaiku45),
            ModelId::CopilotGPT54 => Some(ModelId::CopilotGPT54Mini),
            ModelId::CopilotGPT52Codex | ModelId::CopilotGPT51CodexMax => Some(ModelId::CopilotGPT54Mini),
            ModelId::DeepSeekV4Pro => Some(ModelId::DeepSeekV4Flash),
            ModelId::OpenCodeGoDeepseekV4Pro => Some(ModelId::OpenCodeGoDeepseekV4Flash),
            ModelId::OpenCodeGoGlm52 => Some(ModelId::OpenCodeGoGlm51),
            ModelId::OpenCodeGoMinimaxM3 => Some(ModelId::OpenCodeGoMinimaxM27),
            ModelId::OpenCodeGoMimoV25Pro => Some(ModelId::OpenCodeGoMimoV25),
            ModelId::OpenCodeGoQwen37Max => Some(ModelId::OpenCodeGoQwen37Plus),
            ModelId::OpenCodeGoKimiK27Code => Some(ModelId::OpenCodeGoKimiK26),
            ModelId::HuggingFaceDeepseekV4ProTogether => Some(ModelId::HuggingFaceDeepseekV4FlashNovita),
            ModelId::HuggingFaceDeepseekV4ProNovita => Some(ModelId::HuggingFaceDeepseekV4FlashNovita),
            ModelId::OllamaDeepseekV4ProCloud => Some(ModelId::OllamaDeepseekV4FlashCloud),
            ModelId::StepFun37Flash => None,
            ModelId::EvolinkGpt52
            | ModelId::EvolinkGpt55
            | ModelId::EvolinkDeepseekV4Pro
            | ModelId::EvolinkDeepseekV4Flash
            | ModelId::EvolinkDoubaoSeed20Pro
            | ModelId::EvolinkGemini31Pro
            | ModelId::EvolinkGemini35Flash
            | ModelId::EvolinkMinimaxM3
            | ModelId::EvolinkClaudeSonnet46
            | ModelId::EvolinkClaudeOpus48
            | ModelId::EvolinkClaudeHaiku45 => None,
            ModelId::XaiGrok45 | ModelId::XaiGrok420Reasoning => Some(ModelId::XaiGrokBuild01),
            ModelId::PoolsideLagunaM1 => Some(ModelId::PoolsideLagunaXs2),
            ModelId::PoolsideLagunaS21 => Some(ModelId::PoolsideLagunaXs2),
            _ => None,
        }
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
                        .filter(|&candidate| candidate != self)
                        .filter(|&candidate| {
                            candidate.openrouter_metadata().map(|other| !other.reasoning).unwrap_or(false)
                        })
                        .cloned()
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
            ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => Some(ModelId::Gemini35Flash),
            ModelId::GPT55 | ModelId::GPT54 | ModelId::GPT54Pro | ModelId::GPT54Nano | ModelId::GPT54Mini => {
                Some(ModelId::GPT54Mini)
            }
            ModelId::OpenCodeZenGPT54 => Some(ModelId::OpenCodeZenGPT54Mini),
            ModelId::CopilotGPT52Codex | ModelId::CopilotGPT54 => Some(ModelId::CopilotGPT54Mini),
            ModelId::DeepSeekV4Pro => Some(ModelId::DeepSeekV4Flash),
            ModelId::EvolinkDeepseekV4Pro => Some(ModelId::EvolinkDeepseekV4Flash),
            ModelId::HuggingFaceDeepseekV4ProTogether => Some(ModelId::HuggingFaceDeepseekV4FlashNovita),
            ModelId::HuggingFaceDeepseekV4ProNovita => Some(ModelId::HuggingFaceDeepseekV4FlashNovita),
            ModelId::OllamaDeepseekV4ProCloud => Some(ModelId::OllamaDeepseekV4FlashCloud),
            ModelId::ClaudeSonnet5
            | ModelId::ClaudeFable5
            | ModelId::ClaudeMythos5
            | ModelId::ClaudeOpus5
            | ModelId::ClaudeOpus48
            | ModelId::ClaudeSonnet46 => Some(ModelId::ClaudeSonnet46),
            ModelId::XaiGrok420Reasoning => Some(ModelId::XaiGrokBuild01),
            ModelId::MinimaxM27 => None,
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
            ModelId::Gemini35Flash
                | ModelId::EvolinkGemini35Flash
                | ModelId::EvolinkDeepseekV4Flash
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::HuggingFaceStep35Flash
                | ModelId::StepFun37Flash
                | ModelId::HuggingFaceDeepseekV4FlashNovita
        )
    }

    /// Check if this is a "pro" variant (optimized for capability)
    pub fn is_pro_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini31ProPreview
                | ModelId::Gemini31ProPreviewCustomTools
                | ModelId::OpenRouterGoogleGemini31ProPreview
                | ModelId::GPT56Sol
                | ModelId::GPT55
                | ModelId::GPT54
                | ModelId::GPT54Pro
                | ModelId::GPT53Codex
                | ModelId::CopilotGPT52Codex
                | ModelId::CopilotGPT51CodexMax
                | ModelId::CopilotGPT54
                | ModelId::CopilotClaudeSonnet46
                | ModelId::ClaudeSonnet5
                | ModelId::ClaudeFable5
                | ModelId::ClaudeMythos5
                | ModelId::ClaudeOpus5
                | ModelId::ClaudeOpus48
                | ModelId::ClaudeSonnet46
                | ModelId::OpenCodeZenGPT54
                | ModelId::OpenCodeZenClaudeSonnet46
                | ModelId::OpenCodeZenGlm51
                | ModelId::OpenCodeGoGlm51
                | ModelId::OpenCodeGoGlm52
                | ModelId::OpenCodeGoKimiK27Code
                | ModelId::OpenCodeGoMimoV25Pro
                | ModelId::OpenCodeGoMinimaxM27
                | ModelId::OpenCodeGoMinimaxM3
                | ModelId::OpenCodeGoQwen37Max
                | ModelId::OpenCodeGoDeepseekV4Pro
                | ModelId::DeepSeekV4Pro
                | ModelId::EvolinkDeepseekV4Pro
                | ModelId::ZaiGlm52
                | ModelId::ZaiGlm51
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::MinimaxM27
                | ModelId::OllamaGlm51Cloud
                | ModelId::OllamaGlm52Cloud
                | ModelId::HuggingFaceDeepseekV4ProTogether
                | ModelId::HuggingFaceGlm51Deepinfra
                | ModelId::HuggingFaceGlm52Novita
                | ModelId::HuggingFaceMinimaxM27Novita
                | ModelId::HuggingFaceMinimaxM3Novita
                | ModelId::HuggingFaceDeepseekV4ProNovita
                | ModelId::OpenRouterMoonshotaiKimiK3
                | ModelId::OpenRouterMoonshotaiKimiK26
                | ModelId::OpenRouterMoonshotaiKimiK27Code
                | ModelId::MoonshotKimiK3
                | ModelId::MoonshotKimiK27Code
                | ModelId::PoolsideLagunaM1
                | ModelId::PoolsideLagunaS21
                | ModelId::XaiGrok45
                | ModelId::XaiGrok420Reasoning
        )
    }

    /// Check if this is an optimized/efficient variant
    pub fn is_efficient_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.efficient;
        }
        matches!(
            self,
            ModelId::Gemini35Flash
                | ModelId::GPT54Mini
                | ModelId::GPT56Luna
                | ModelId::CopilotGPT54Mini
                | ModelId::ClaudeHaiku45
                | ModelId::OpenCodeZenGPT54Mini
                | ModelId::DeepSeekV4Flash
                | ModelId::HuggingFaceStep35Flash
                | ModelId::HuggingFaceDeepseekV4FlashNovita
                | ModelId::PoolsideLagunaXs2
                | ModelId::OpenCodeGoMimoV25
                | ModelId::OpenCodeGoQwen37Plus
                | ModelId::OpenCodeGoQwen36Plus
                | ModelId::OpenCodeGoDeepseekV4Flash
                | ModelId::XaiGrokBuild01
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
                | ModelId::Gemini35Flash
                | ModelId::GPT56Sol
                | ModelId::GPT56Terra
                | ModelId::GPT55
                | ModelId::GPT54
                | ModelId::GPT54Pro
                | ModelId::GPT53Codex
                | ModelId::ClaudeSonnet5
                | ModelId::ClaudeFable5
                | ModelId::ClaudeMythos5
                | ModelId::ClaudeOpus5
                | ModelId::ClaudeOpus48
                | ModelId::ClaudeSonnet46
                | ModelId::OpenCodeZenGPT54
                | ModelId::OpenCodeZenClaudeSonnet46
                | ModelId::OpenCodeZenGlm51
                | ModelId::OpenCodeGoGlm51
                | ModelId::OpenCodeGoGlm52
                | ModelId::OpenCodeGoKimiK27Code
                | ModelId::OpenCodeGoKimiK26
                | ModelId::OpenCodeGoMimoV25Pro
                | ModelId::OpenCodeGoMinimaxM27
                | ModelId::OpenCodeGoMinimaxM3
                | ModelId::OpenCodeGoQwen37Max
                | ModelId::OpenCodeGoQwen37Plus
                | ModelId::OpenCodeGoDeepseekV4Pro
                | ModelId::DeepSeekV4Pro
                | ModelId::ZaiGlm52
                | ModelId::ZaiGlm51
                | ModelId::OpenRouterStepfunStep35FlashFree
                | ModelId::HuggingFaceDeepseekV4FlashNovita
                | ModelId::HuggingFaceDeepseekV4ProTogether
                | ModelId::HuggingFaceGlm51Deepinfra
                | ModelId::HuggingFaceGlm52Novita
                | ModelId::HuggingFaceMinimaxM27Novita
                | ModelId::HuggingFaceMinimaxM3Novita
                | ModelId::HuggingFaceDeepseekV4ProNovita
                | ModelId::OpenRouterMoonshotaiKimiK3
                | ModelId::OpenRouterMoonshotaiKimiK26
                | ModelId::OpenRouterMoonshotaiKimiK27Code
                | ModelId::MoonshotKimiK3
                | ModelId::MoonshotKimiK27Code
                | ModelId::PoolsideLagunaM1
                | ModelId::PoolsideLagunaS21
                | ModelId::OllamaGlm52Cloud
                | ModelId::XaiGrok45
                | ModelId::XaiGrok420Reasoning
        )
    }

    /// Determine whether the model is a reasoning-capable variant
    pub fn is_reasoning_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.reasoning;
        }
        self.provider().supports_reasoning_effort(&self.as_str())
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
        self.generated_capabilities().map(|meta| meta.input_modalities).unwrap_or(&[])
    }

    /// Get the generation/version string for this model
    pub fn generation(&self) -> &'static str {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.generation;
        }
        match self {
            // Gemini generations
            ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => "3.1",
            // OpenAI generations
            ModelId::GPT56Sol | ModelId::GPT56Terra | ModelId::GPT56Luna => "5.6",
            ModelId::GPT55 => "5.5",
            ModelId::GPT54 | ModelId::GPT54Pro | ModelId::GPT54Nano | ModelId::GPT54Mini => "5.4",
            ModelId::GPT53Codex => "5.3",
            ModelId::OpenAIGptOss20b | ModelId::OpenAIGptOss120b => "5",
            // Anthropic generations
            ModelId::ClaudeSonnet5 => "5",
            ModelId::ClaudeFable5 => "5",
            ModelId::ClaudeMythos5 => "5",
            ModelId::ClaudeOpus5 => "5",
            ModelId::ClaudeOpus48 => "4.8",
            ModelId::ClaudeSonnet46 => "4.6",
            ModelId::ClaudeHaiku45 => "4.5",
            // DeepSeek generations
            ModelId::DeepSeekV4Pro | ModelId::DeepSeekV4Flash => "4",
            // Z.AI generations
            ModelId::ZaiGlm52 => "5.2",
            ModelId::ZaiGlm51 => "5.1",
            ModelId::Gemini35Flash => "3.5",
            ModelId::OpenCodeZenGPT54 | ModelId::OpenCodeZenGPT54Mini => "5.4",
            ModelId::OpenCodeZenClaudeSonnet46 => "4.6",
            ModelId::OpenCodeZenGlm51 | ModelId::OpenCodeGoGlm51 => "5.1",
            ModelId::OpenCodeGoGlm52 => "5.2",
            ModelId::OpenCodeGoKimiK27Code => "k2.7",
            ModelId::OpenCodeGoKimiK26 => "k2.6",
            ModelId::OpenCodeGoMimoV25 | ModelId::OpenCodeGoMimoV25Pro => "v2.5",
            ModelId::OpenCodeGoMinimaxM3 => "m3",
            ModelId::OpenCodeGoMinimaxM27 => "m2.7",
            ModelId::OpenCodeGoQwen37Max => "3.7-max",
            ModelId::OpenCodeGoQwen37Plus => "3.7-plus",
            ModelId::OpenCodeGoQwen36Plus => "3.6-plus",
            ModelId::OpenCodeGoDeepseekV4Pro | ModelId::OpenCodeGoDeepseekV4Flash => "v4",
            ModelId::OllamaGptOss20b => "oss",
            ModelId::OllamaGptOss20bCloud => "oss-cloud",
            ModelId::OllamaGptOss120bCloud => "oss-cloud",
            ModelId::OllamaDeepseekV4FlashCloud => "deepseek-v4-flash",
            ModelId::OllamaDeepseekV4ProCloud => "deepseek-v4-pro",
            ModelId::OllamaMinimaxM27Cloud => "minimax-m2.7",
            ModelId::OllamaMinimaxM3Cloud => "minimax-m3",
            ModelId::OllamaGlm51Cloud => "glm-5.1",
            ModelId::OllamaGlm52Cloud => "glm-5.2",
            ModelId::OllamaKimiK26Cloud => "kimi-k2.6",
            ModelId::OllamaKimiK27CodeCloud => "kimi-k2.7-code",
            ModelId::OllamaLagunaXs2 => "laguna-xs.2",
            ModelId::OllamaGemma4 => "gemma-4",
            ModelId::LlamaCppGemma426bA4b => "4",
            ModelId::LlamaCppGemma4E4b => "4",
            ModelId::LlamaCppGptOss20b => "oss",
            ModelId::LlamaCppStep35Flash => "3.5",
            // MiniMax models
            ModelId::MinimaxM3 => "M3",
            ModelId::MinimaxM27 => "M2.7",
            // Moonshot models
            ModelId::MoonshotKimiK3 => "k3",
            ModelId::MoonshotKimiK27Code => "k2.7",
            ModelId::MoonshotKimiK26 => "k2.6",
            // Hugging Face generations
            ModelId::HuggingFaceOpenAIGptOss20b => "oss",
            ModelId::HuggingFaceOpenAIGptOss120b => "oss",
            ModelId::HuggingFaceMinimaxM27Novita => "m2.7",
            ModelId::HuggingFaceMinimaxM3Novita => "m3",
            ModelId::HuggingFaceGlm51ZaiOrg => "5.1",
            ModelId::HuggingFaceGlm52Novita => "5.2",
            ModelId::HuggingFaceGlm51Deepinfra => "5.1",
            ModelId::HuggingFaceKimiK26Novita => "k2.6",
            ModelId::HuggingFaceDeepseekV4FlashNovita => "v4-flash",
            ModelId::HuggingFaceDeepseekV4ProTogether => "v4-pro",
            ModelId::HuggingFaceDeepseekV4ProNovita => "v4-pro",
            ModelId::HuggingFaceStep35Flash => "3.5",
            // xAI models
            ModelId::XaiGrokBuild01 => "build-0.1",
            ModelId::XaiGrok45 => "4.5",
            ModelId::XaiGrok43 => "4.3",
            ModelId::XaiGrok420Reasoning => "4.20",
            // Poolside models
            ModelId::PoolsideLagunaM1 => "laguna-m.1",
            ModelId::PoolsideLagunaXs2 => "laguna-xs.2",
            ModelId::PoolsideLagunaS21 => "laguna-s.2.1",
            // Qwen models
            ModelId::QwenDeepSeekV4Flash | ModelId::QwenDeepSeekV4Pro => "v4",
            ModelId::QwenGlm51 => "5.1",
            _ => "unknown",
        }
    }

    /// Determine if this model supports GPT-5.1+/5.2+/5.3+ shell tool type
    pub(crate) fn supports_shell_tool(&self) -> bool {
        matches!(
            self,
            ModelId::GPT56Sol
                | ModelId::GPT56Terra
                | ModelId::GPT56Luna
                | ModelId::GPT55
                | ModelId::GPT54
                | ModelId::GPT54Pro
                | ModelId::GPT53Codex
        )
    }

    /// Determine if this model supports optimized apply_patch tool
    pub fn supports_apply_patch_tool(&self) -> bool {
        false
    }
}
