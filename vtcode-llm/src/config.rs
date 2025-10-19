//! Provider configuration traits decoupled from VTCode's dot-config storage.
//!
//! Consumers can implement [`ProviderConfig`] for their own types and use the
//! conversion helpers to build `vtcode_core` provider factories without
//! depending on VTCode's internal configuration structs.

use std::borrow::Cow;

use vtcode_core::config::core::PromptCachingConfig;

/// Trait describing the configuration required to instantiate an LLM provider.
///
/// The trait intentionally returns owned-friendly values so that consumers can
/// back the configuration with environment variables, secret managers, or
/// custom structs. The [`as_factory_config`] helper converts a trait object into
/// the concrete configuration type expected by `vtcode_core`'s provider
/// factory.
pub trait ProviderConfig {
    /// API key or bearer token used to authenticate with the provider.
    fn api_key(&self) -> Option<Cow<'_, str>>;

    /// Optional override for the provider's base URL.
    fn base_url(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Preferred model identifier for the provider.
    fn model(&self) -> Option<Cow<'_, str>> {
        None
    }

    /// Optional prompt cache configuration forwarded to providers that support
    /// caching.
    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        None
    }
}

/// Convert an implementor of [`ProviderConfig`] into the configuration used by
/// the `vtcode_core` provider factory.
pub fn as_factory_config(source: &dyn ProviderConfig) -> vtcode_core::llm::factory::ProviderConfig {
    vtcode_core::llm::factory::ProviderConfig {
        api_key: source.api_key().map(Cow::into_owned),
        base_url: source.base_url().map(Cow::into_owned),
        model: source.model().map(Cow::into_owned),
        prompt_cache: source.prompt_cache().map(|cfg| cfg.into_owned()),
    }
}

/// [`ProviderConfig`] implementation for VTCode's dot-config provider entries.
impl ProviderConfig for vtcode_core::utils::dot_config::ProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        self.api_key.as_deref().map(Cow::Borrowed)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        self.base_url.as_deref().map(Cow::Borrowed)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        self.model.as_deref().map(Cow::Borrowed)
    }
}

/// [`ProviderConfig`] implementation for the concrete factory configuration.
impl ProviderConfig for vtcode_core::llm::factory::ProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        self.api_key.as_deref().map(Cow::Borrowed)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        self.base_url.as_deref().map(Cow::Borrowed)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        self.model.as_deref().map(Cow::Borrowed)
    }

    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        self.prompt_cache
            .as_ref()
            .map(|cfg| Cow::Owned(cfg.clone()))
    }
}

/// Simple builder-friendly provider configuration backed by owned values.
#[derive(Clone, Debug, Default)]
pub struct OwnedProviderConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
}

impl OwnedProviderConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api_key(mut self, value: impl Into<String>) -> Self {
        self.api_key = Some(value.into());
        self
    }

    pub fn with_base_url(mut self, value: impl Into<String>) -> Self {
        self.base_url = Some(value.into());
        self
    }

    pub fn with_model(mut self, value: impl Into<String>) -> Self {
        self.model = Some(value.into());
        self
    }

    pub fn with_prompt_cache(mut self, value: PromptCachingConfig) -> Self {
        self.prompt_cache = Some(value);
        self
    }
}

impl ProviderConfig for OwnedProviderConfig {
    fn api_key(&self) -> Option<Cow<'_, str>> {
        self.api_key.as_deref().map(Cow::Borrowed)
    }

    fn base_url(&self) -> Option<Cow<'_, str>> {
        self.base_url.as_deref().map(Cow::Borrowed)
    }

    fn model(&self) -> Option<Cow<'_, str>> {
        self.model.as_deref().map(Cow::Borrowed)
    }

    fn prompt_cache(&self) -> Option<Cow<'_, PromptCachingConfig>> {
        self.prompt_cache
            .as_ref()
            .map(|cfg| Cow::Owned(cfg.clone()))
    }
}
