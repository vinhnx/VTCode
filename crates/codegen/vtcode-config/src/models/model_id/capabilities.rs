use crate::models::{Provider, ProviderModelSupport};

use super::ModelId;

#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
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
    pub reasoning_efforts: &'static [&'static str],
    pub is_pro: bool,
    pub lightweight_model: Option<&'static str>,
    pub tool_call: bool,
    pub vision: bool,
    pub input_modalities: &'static [&'static str],
    pub caching: bool,
    pub structured_output: bool,
    pub supports_sampling: bool,
    pub supports_logprobs: bool,
    pub prompt_cache_ttl: Option<&'static str>,
    pub prompt_contract: Option<&'static str>,
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
    } else if provider.eq_ignore_ascii_case("meta") || provider.eq_ignore_ascii_case("meta-ai") {
        "meta"
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
    } else if provider.eq_ignore_ascii_case("nvidia") {
        "nvidia"
    } else if provider.eq_ignore_ascii_case("merge-gateway") {
        "merge-gateway"
    } else if provider.eq_ignore_ascii_case("vercel") || provider.eq_ignore_ascii_case("vercel-ai-gateway") {
        "vercel"
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
        Provider::Meta => "meta",
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
        Provider::NVIDIA => "nvidia",
        Provider::MergeGateway => "merge-gateway",
        Provider::Vercel => "vercel",
    }
}

fn catalog_lookup_id<'a>(provider: &str, id: &'a str) -> &'a str {
    let provider_key = catalog_provider_key(provider);
    if provider_key == "evolink"
        && let Some((prefix, model_id)) = id.split_once('/')
        && prefix.eq_ignore_ascii_case(provider_key)
    {
        // Evolink namespaces its ModelId values to avoid collisions with
        // first-class providers, while its catalog stores the upstream model
        // id that the gateway receives (for example, `deepseek-v4-pro`).
        return model_id;
    }
    id
}

fn generated_catalog_entry(provider: &str, id: &str) -> Option<ModelCatalogEntry> {
    let provider_key = catalog_provider_key(provider);
    let lookup_id = catalog_lookup_id(provider_key, id);
    // Exact match wins so gateway providers that store prefixed IDs
    // (`merge-gateway`/`vercel` keep `openai/gpt-5.6-luna`) keep working.
    // Fall back to stripping a provider-prefixed request model
    // (`openai/gpt-5.6-luna` on provider `openai` stores `gpt-5.6-luna`).
    // The strip is provider-scoped (prefix must equal the provider key) so an
    // unrelated prefix (`other-org/gpt-5.6-luna`) does not resolve to the bare
    // model's capabilities. Without this, reasoning-effort and context
    // lookups miss and the harness omits the effort with an empty
    // `supported:` diagnostic.
    let entry = capability_generated::metadata_for(provider_key, lookup_id).or_else(|| {
        lookup_id
            .split_once('/')
            .filter(|(prefix, _)| prefix.eq_ignore_ascii_case(provider_key))
            .and_then(|(_, rest)| capability_generated::metadata_for(provider_key, rest))
    });
    entry.map(|entry| ModelCatalogEntry {
        provider: entry.provider,
        id: entry.id,
        display_name: entry.display_name,
        description: entry.description,
        context_window: entry.context_window,
        max_output_tokens: entry.max_output_tokens,
        reasoning: entry.reasoning,
        reasoning_efforts: entry.reasoning_efforts,
        is_pro: entry.is_pro,
        lightweight_model: entry.lightweight_model,
        tool_call: entry.tool_call,
        vision: entry.vision,
        input_modalities: entry.input_modalities,
        caching: entry.caching,
        structured_output: entry.structured_output,
        supports_sampling: entry.supports_sampling,
        supports_logprobs: entry.supports_logprobs,
        prompt_cache_ttl: entry.prompt_cache_ttl,
        prompt_contract: entry.prompt_contract,
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
        let target_id = self.generated_capabilities()?.lightweight_model?;
        Self::all_models().into_iter().find(|candidate| {
            candidate != self && candidate.provider() == self.provider() && candidate.as_str() == target_id
        })
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

        self.preferred_lightweight_variant()
            .filter(|candidate| !candidate.is_reasoning_variant())
    }

    /// Check if this is a "flash" variant (optimized for speed)
    pub fn is_flash_variant(&self) -> bool {
        matches!(
            self,
            ModelId::Gemini38Flash
                | ModelId::MergeGatewayGoogleGemini38Flash
                | ModelId::VercelGoogleGemini38Flash
                | ModelId::VercelDeepseekFlash
                | ModelId::VercelOpenAiGpt56Luna
                | ModelId::VercelAnthropicClaudeHaiku45
                | ModelId::ClaudeHaiku55
                | ModelId::MergeGatewayAnthropicClaudeHaiku4520251001
                | ModelId::MergeGatewayAnthropicClaudeHaiku55
                | ModelId::StepFun37Flash
                | ModelId::MergeGatewayDeepseekFlash
                | ModelId::DeepSeekFlash
                | ModelId::ZaiGlm53Flash
                | ModelId::ZaiGlm53Flashx
                | ModelId::MergeGatewayZaiGlm53Flash
                | ModelId::MergeGatewayZaiGlm53Flashx
                | ModelId::HuggingFaceGlm53FlashTogether
                | ModelId::MiMoV26Flash
                | ModelId::MergeGatewayXiaomimimoMimoV26Flash
        )
    }

    /// Check if this is a "pro" variant (optimized for capability)
    pub fn is_pro_variant(&self) -> bool {
        self.generated_capabilities().is_some_and(|entry| entry.is_pro)
    }

    /// Check if this is an optimized/efficient variant
    pub fn is_efficient_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.efficient;
        }
        matches!(
            self,
            ModelId::Gemini38Flash
                | ModelId::MergeGatewayGoogleGemini38Flash
                | ModelId::GPT56Luna
                | ModelId::GPT6Luna
                | ModelId::MergeGatewayOpenAIGpt56Luna
                | ModelId::MergeGatewayOpenAIGpt6Luna
                | ModelId::CopilotGPT54Mini
                | ModelId::ClaudeHaiku55
                | ModelId::MergeGatewayAnthropicClaudeHaiku55
                | ModelId::DeepSeekFlash
                | ModelId::MergeGatewayDeepseekFlash
                | ModelId::MetaMuseSpark11
                | ModelId::MergeGatewayMinimaxH3
                | ModelId::MiMoV26Flash
                | ModelId::ZaiGlm53Flash
                | ModelId::ZaiGlm53Flashx
                | ModelId::MergeGatewayZaiGlm53Flash
                | ModelId::MergeGatewayZaiGlm53Flashx
                | ModelId::HuggingFaceGlm53FlashTogether
                | ModelId::VercelDeepseekFlash
                | ModelId::MergeGatewayXiaomimimoMimoV26Flash
        )
    }

    /// Check if this is a top-tier model
    pub fn is_top_tier(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.top_tier;
        }
        matches!(
            self,
            ModelId::Gemini38Flash
                | ModelId::MergeGatewayGoogleGemini38Flash
                | ModelId::GPT6Astra
                | ModelId::GPT6Sol
                | ModelId::GPT61Sol
                | ModelId::GPT56Sol
                | ModelId::MergeGatewayOpenAIGpt56Sol
                | ModelId::MergeGatewayOpenAIGpt56Terra
                | ModelId::MergeGatewayOpenAIGpt56Luna
                | ModelId::MergeGatewayOpenAIGpt6Astra
                | ModelId::MergeGatewayOpenAIGpt6Sol
                | ModelId::MergeGatewayOpenAIGpt61Sol
                | ModelId::MergeGatewayOpenAIGpt6Luna
                | ModelId::ClaudeSonnet5
                | ModelId::ClaudeSonnet55
                | ModelId::ClaudeFable5
                | ModelId::ClaudeFable51
                | ModelId::ClaudeOpus5
                | ModelId::ClaudeOpus55
                | ModelId::MistralLarge4
                | ModelId::OpenCodeGoGlm53
                | ModelId::MiMoV26Pro
                | ModelId::MiMoV26ProUltraspeed
                | ModelId::OpenCodeGoMinimaxM3
                | ModelId::MergeGatewayDeepseekFlash
                | ModelId::VercelDeepseekFlash
                | ModelId::MetaMuseSpark13
                | ModelId::MetaMuseSpark13Contributor
                | ModelId::ZaiGlm53
                | ModelId::ZaiGlm53Flash
                | ModelId::ZaiGlm53Flashx
                | ModelId::MergeGatewayZaiGlm53Flash
                | ModelId::MergeGatewayZaiGlm53Flashx
                | ModelId::HuggingFaceGlm53FlashTogether
                | ModelId::HuggingFaceGlm53Together
                | ModelId::HuggingFaceMinimaxM3Novita
                | ModelId::StepFun5Preview
                | ModelId::OpenRouterMoonshotaiKimiK3
                | ModelId::MoonshotKimiK3
                | ModelId::MergeGatewayMoonshotKimiK3
                | ModelId::OllamaGlm53Cloud
                | ModelId::XaiGrok46
                | ModelId::XaiGrok47
                | ModelId::MergeGatewayXaiGrok46
                | ModelId::MergeGatewayXaiGrok47
                | ModelId::VercelSpacexaiGrok47
                | ModelId::MergeGatewayXiaomimimoMimoV26Pro
                | ModelId::MergeGatewayXiaomimimoMimoV26Flash
                | ModelId::MergeGatewayMistralLarge4
        )
    }

    /// Determine whether the model is a reasoning-capable variant
    pub fn is_reasoning_variant(&self) -> bool {
        if let Some(meta) = self.openrouter_metadata() {
            return meta.reasoning;
        }
        self.provider().supports_reasoning(&self.as_str())
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
            // OpenAI generations
            ModelId::GPT6Astra | ModelId::GPT6Sol | ModelId::GPT6Luna => "6",
            ModelId::GPT61Sol => "6.1",
            ModelId::GPT56Sol | ModelId::GPT56Terra | ModelId::GPT56Luna => "5.6",
            ModelId::OpenAIGptOss20b | ModelId::OpenAIGptOss120b => "5",
            // Anthropic generations
            ModelId::ClaudeSonnet5 => "5",
            ModelId::ClaudeSonnet55 => "5.5",
            ModelId::ClaudeHaiku55 => "5.5",
            ModelId::ClaudeFable5 => "5",
            ModelId::ClaudeFable51 => "5.1",
            ModelId::ClaudeOpus5 => "5",
            ModelId::ClaudeOpus55 => "5.5",
            // DeepSeek generations
            ModelId::DeepSeekFlash => "4",
            // Mistral generations
            ModelId::MistralLarge3 => "3",
            ModelId::MistralLarge4 => "4",
            ModelId::MergeGatewayDeepseekFlash => "4.1",
            ModelId::MetaMuseSpark11 => "Muse-Spark-1.1",
            ModelId::MetaMuseSpark13 | ModelId::MetaMuseSpark13Contributor => "Muse-Spark-1.3",
            // Z.AI generations
            ModelId::ZaiGlm53
            | ModelId::ZaiGlm53Flash
            | ModelId::ZaiGlm53Flashx
            | ModelId::MergeGatewayZaiGlm53Flash
            | ModelId::MergeGatewayZaiGlm53Flashx => "5.3",
            ModelId::Gemini38Flash => "3.8",
            ModelId::MergeGatewayGoogleGemini38Flash => "3.8",
            ModelId::OpenCodeGoGlm53 => "5.3",
            ModelId::OpenCodeGoGpt56Luna => "5.6-luna",
            ModelId::OpenCodeGoKimiK3 => "k3",
            ModelId::MiMoV26Pro | ModelId::MiMoV26Flash | ModelId::MiMoV26ProUltraspeed => "v2.6",
            ModelId::MergeGatewayXiaomimimoMimoV26Pro | ModelId::MergeGatewayXiaomimimoMimoV26Flash => "v2.6",
            ModelId::MergeGatewayMistralLarge4 => "4",
            ModelId::OpenCodeGoMinimaxM3 => "m3",
            ModelId::OllamaGptOss20b => "oss",
            ModelId::OllamaGptOss20bCloud => "oss-cloud",
            ModelId::OllamaGptOss120bCloud => "oss-cloud",
            ModelId::OllamaMinimaxM3Cloud => "minimax-m3",
            ModelId::OllamaGlm53Cloud => "glm-5.3",
            ModelId::OllamaGemma4 => "gemma-4",
            ModelId::LlamaCppGemma426bA4b => "4",
            ModelId::LlamaCppGemma4E4b => "4",
            ModelId::LlamaCppGptOss20b => "oss",
            // MiniMax models
            ModelId::MinimaxM3 => "M3",
            // StepFun models
            ModelId::StepFun37Flash => "3.7",
            ModelId::StepFun5Preview => "5",
            // Moonshot models
            ModelId::MoonshotKimiK3 => "k3",
            ModelId::MergeGatewayMoonshotKimiK3 => "k3",
            // Hugging Face generations
            ModelId::HuggingFaceOpenAIGptOss20b => "oss",
            ModelId::HuggingFaceOpenAIGptOss120b => "oss",
            ModelId::HuggingFaceMinimaxM3Novita => "m3",
            ModelId::HuggingFaceGlm53FlashTogether => "5.3",
            ModelId::HuggingFaceGlm53Together => "5.3",
            // xAI models
            ModelId::XaiGrok46 => "4.6",
            ModelId::XaiGrok47 => "4.7",
            ModelId::MergeGatewayXaiGrok46 => "4.6",
            ModelId::MergeGatewayXaiGrok47 => "4.7",
            // Qwen models
            ModelId::MergeGatewayDefaultRouting => "routing",
            ModelId::MergeGatewayAnthropicClaudeOpus5 => "5",
            ModelId::MergeGatewayAnthropicClaudeOpus55 => "5.5",
            ModelId::MergeGatewayAnthropicClaudeSonnet5 => "5",
            ModelId::MergeGatewayAnthropicClaudeSonnet55 => "5.5",
            ModelId::MergeGatewayAnthropicClaudeHaiku55 => "5.5",
            ModelId::MergeGatewayAnthropicClaudeFable51 => "5.1",
            ModelId::MergeGatewayAnthropicClaudeHaiku4520251001 => "4.5",
            ModelId::MergeGatewayMinimaxH3 => "H3",
            ModelId::MergeGatewayThinkingMachinesInkling => "Inkling",
            ModelId::MergeGatewayOpenAIGpt56Luna
            | ModelId::MergeGatewayOpenAIGpt56Sol
            | ModelId::MergeGatewayOpenAIGpt56Terra => "5.6",
            ModelId::MergeGatewayOpenAIGpt6Astra
            | ModelId::MergeGatewayOpenAIGpt6Sol
            | ModelId::MergeGatewayOpenAIGpt6Luna => "6",
            ModelId::MergeGatewayOpenAIGpt61Sol => "6.1",
            // Vercel AI Gateway models
            ModelId::VercelAnthropicClaudeSonnet5 => "5",
            ModelId::VercelAnthropicClaudeOpus5 => "5",
            ModelId::VercelAnthropicClaudeOpus55 => "5.5",
            ModelId::VercelAnthropicClaudeHaiku45 => "4.5",
            ModelId::VercelOpenAiGpt56Sol | ModelId::VercelOpenAiGpt56Luna => "5.6",
            ModelId::VercelOpenAiGpt6Astra => "6",
            ModelId::VercelGoogleGemini38Flash => "3.8",
            ModelId::VercelDeepseekFlash => "4.1",
            ModelId::VercelMoonshotaiKimiK3 => "k3",
            ModelId::VercelMinimaxM3 => "M3",
            ModelId::VercelSpacexaiGrok47 => "4.7",
            _ => "unknown",
        }
    }

    /// Determine if this model supports GPT-5.1+/5.2+/5.3+ shell tool type
    pub(crate) fn supports_shell_tool(&self) -> bool {
        matches!(
            self,
            ModelId::GPT6Astra
                | ModelId::GPT6Sol
                | ModelId::GPT61Sol
                | ModelId::GPT6Luna
                | ModelId::GPT56Sol
                | ModelId::GPT56Terra
                | ModelId::GPT56Luna
        )
    }

    /// Determine if this model supports optimized apply_patch tool
    pub fn supports_apply_patch_tool(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixed_request_model_resolves_on_bare_provider() {
        // Regression for `openai/gpt-5.6-luna` on provider `openai` resolving
        // to empty `supported:` and omitting reasoning effort: exact bare hit
        // and prefixed fallback must agree, while gateway prefixed entries
        // keep their exact match.
        let bare = model_catalog_entry("openai", "gpt-5.6-luna").expect("bare openai entry exists");
        let prefixed = model_catalog_entry("openai", "openai/gpt-5.6-luna").expect("prefixed fallback must resolve");
        assert_eq!(bare.reasoning_efforts, prefixed.reasoning_efforts);
        assert!(!prefixed.reasoning_efforts.is_empty());

        let gateway =
            model_catalog_entry("merge-gateway", "openai/gpt-5.6-luna").expect("gateway prefixed entry exists");
        assert!(!gateway.reasoning_efforts.is_empty());

        // Asymmetric boundary: unknown model stays missing on both shapes,
        // and an unrelated prefix must not resolve to the bare model.
        assert!(model_catalog_entry("openai", "no-such-model-xyz").is_none());
        assert!(model_catalog_entry("openai", "openai/no-such-model-xyz").is_none());
        assert!(model_catalog_entry("openai", "other-org/gpt-5.6-luna").is_none());
    }
}
