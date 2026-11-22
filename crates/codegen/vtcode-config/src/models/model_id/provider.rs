use crate::models::{Provider, ProviderModelSupport};

use super::ModelId;

/// Canonical catalog key for a built-in provider. Kept here so capability
/// lookups can derive a `'static` key from a temporary `Provider` without an
/// allocation, and so a new provider variant forces this table to be updated.
fn canonical_provider_key(provider: Provider) -> &'static str {
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

impl ModelId {
    /// Raw provider key for this model.
    ///
    /// Custom variants preserve the user-declared key even when it is not a
    /// built-in [`Provider`]; every other variant uses its canonical provider
    /// key. Capability lookups must use this rather than [`Self::provider`],
    /// whose fallback would attribute a custom route to OpenAI's catalog.
    pub(super) fn provider_key(&self) -> &str {
        match self {
            ModelId::Custom(provider_key, _) => provider_key,
            _ => canonical_provider_key(self.provider()),
        }
    }

    /// Whether the declared provider key names a built-in provider. Custom
    /// routes without one have no reliable catalog or provider capabilities.
    fn has_builtin_provider_key(&self) -> bool {
        match self {
            ModelId::Custom(provider_key, _) => {
                use std::str::FromStr;
                Provider::from_str(provider_key).is_ok()
            }
            _ => true,
        }
    }

    /// Get the provider for this model
    pub fn provider(&self) -> Provider {
        if self.openrouter_metadata().is_some() {
            return Provider::OpenRouter;
        }
        if let Some(provider) = self.table_provider() {
            return provider;
        }
        match self {
            ModelId::Custom(provider_key, _) => {
                use std::str::FromStr;
                Provider::from_str(provider_key).unwrap_or(Provider::OpenAI)
            }
            // Every remaining variant is a hand-written OpenRouter model without
            // generated metadata; the table covers all other providers.
            _ => Provider::OpenRouter,
        }
    }

    /// Whether this model supports configurable reasoning effort levels
    pub fn supports_reasoning_effort(&self) -> bool {
        self.has_builtin_provider_key() && self.provider().supports_reasoning_effort(&self.as_str())
    }

    /// Whether this model exposes structured reasoning output.
    pub fn supports_reasoning(&self) -> bool {
        self.has_builtin_provider_key() && self.provider().supports_reasoning(&self.as_str())
    }
}
