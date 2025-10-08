use super::providers::{
    AnthropicProvider, DeepSeekProvider, GeminiProvider, OpenAIProvider, OpenRouterProvider,
    XAIProvider, ZAIProvider,
};
use crate::config::core::PromptCachingConfig;
use crate::llm::provider::{LLMError, LLMProvider};
use std::collections::HashMap;

/// LLM provider factory and registry
pub struct LLMFactory {
    providers: HashMap<String, Box<dyn Fn(ProviderConfig) -> Box<dyn LLMProvider> + Send + Sync>>,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub prompt_cache: Option<PromptCachingConfig>,
}

impl LLMFactory {
    pub fn new() -> Self {
        let mut factory = Self {
            providers: HashMap::new(),
        };

        // Register built-in providers
        factory.register_provider(
            "gemini",
            Box::new(|config: ProviderConfig| {
                let ProviderConfig {
                    api_key,
                    base_url,
                    model,
                    prompt_cache,
                } = config;
                Box::new(GeminiProvider::from_config(
                    api_key,
                    model,
                    base_url,
                    prompt_cache,
                )) as Box<dyn LLMProvider>
            }),
        );

        factory.register_provider(
            "openai",
            Box::new(|config: ProviderConfig| {
                let ProviderConfig {
                    api_key,
                    base_url,
                    model,
                    prompt_cache,
                } = config;
                Box::new(OpenAIProvider::from_config(
                    api_key,
                    model,
                    base_url,
                    prompt_cache,
                )) as Box<dyn LLMProvider>
            }),
        );

        factory.register_provider(
            "anthropic",
            Box::new(|config: ProviderConfig| {
                let ProviderConfig {
                    api_key,
                    base_url,
                    model,
                    prompt_cache,
                } = config;
                Box::new(AnthropicProvider::from_config(
                    api_key,
                    model,
                    base_url,
                    prompt_cache,
                )) as Box<dyn LLMProvider>
            }),
        );

        factory.register_provider(
            "deepseek",
            Box::new(|config: ProviderConfig| {
                let ProviderConfig {
                    api_key,
                    base_url,
                    model,
                    prompt_cache,
                } = config;
                Box::new(DeepSeekProvider::from_config(
                    api_key,
                    model,
                    base_url,
                    prompt_cache,
                )) as Box<dyn LLMProvider>
            }),
        );

        factory.register_provider(
            "openrouter",
            Box::new(|config: ProviderConfig| {
                let ProviderConfig {
                    api_key,
                    base_url,
                    model,
                    prompt_cache,
                } = config;
                Box::new(OpenRouterProvider::from_config(
                    api_key,
                    model,
                    base_url,
                    prompt_cache,
                )) as Box<dyn LLMProvider>
            }),
        );

        factory.register_provider(
            "xai",
            Box::new(|config: ProviderConfig| {
                let ProviderConfig {
                    api_key,
                    base_url,
                    model,
                    prompt_cache,
                } = config;
                Box::new(XAIProvider::from_config(
                    api_key,
                    model,
                    base_url,
                    prompt_cache,
                )) as Box<dyn LLMProvider>
            }),
        );

        factory.register_provider(
            "zai",
            Box::new(|config: ProviderConfig| {
                let ProviderConfig {
                    api_key,
                    base_url,
                    model,
                    prompt_cache,
                } = config;
                Box::new(ZAIProvider::from_config(
                    api_key,
                    model,
                    base_url,
                    prompt_cache,
                )) as Box<dyn LLMProvider>
            }),
        );

        factory
    }

    /// Register a new provider
    pub fn register_provider<F>(&mut self, name: &str, factory_fn: F)
    where
        F: Fn(ProviderConfig) -> Box<dyn LLMProvider> + Send + Sync + 'static,
    {
        self.providers
            .insert(name.to_string(), Box::new(factory_fn));
    }

    /// Create provider instance
    pub fn create_provider(
        &self,
        provider_name: &str,
        config: ProviderConfig,
    ) -> Result<Box<dyn LLMProvider>, LLMError> {
        let factory_fn = self.providers.get(provider_name).ok_or_else(|| {
            LLMError::InvalidRequest(format!("Unknown provider: {}", provider_name))
        })?;

        Ok(factory_fn(config))
    }

    /// List available providers
    pub fn list_providers(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Determine provider name from model string
    pub fn provider_from_model(&self, model: &str) -> Option<String> {
        let m = model.to_lowercase();
        if m.starts_with("gpt-") || m.starts_with("o3") || m.starts_with("o1") {
            Some("openai".to_string())
        } else if m.starts_with("claude-") {
            Some("anthropic".to_string())
        } else if m.starts_with("deepseek-") {
            Some("deepseek".to_string())
        } else if m.contains("gemini") || m.starts_with("palm") {
            Some("gemini".to_string())
        } else if m.starts_with("grok-") || m.starts_with("xai-") {
            Some("xai".to_string())
        } else if m.starts_with("glm-") {
            Some("zai".to_string())
        } else if m.contains('/') || m.contains('@') {
            Some("openrouter".to_string())
        } else {
            None
        }
    }
}

impl Default for LLMFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Global factory instance
use std::sync::{LazyLock, Mutex};

static FACTORY: LazyLock<Mutex<LLMFactory>> = LazyLock::new(|| Mutex::new(LLMFactory::new()));

/// Get global factory instance
pub fn get_factory() -> &'static Mutex<LLMFactory> {
    &FACTORY
}

/// Create provider from model name and API key
pub fn create_provider_for_model(
    model: &str,
    api_key: String,
    prompt_cache: Option<PromptCachingConfig>,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    let factory = get_factory().lock().unwrap();
    let provider_name = factory.provider_from_model(model).ok_or_else(|| {
        LLMError::InvalidRequest(format!("Cannot determine provider for model: {}", model))
    })?;
    drop(factory);

    create_provider_with_config(
        &provider_name,
        Some(api_key),
        None,
        Some(model.to_string()),
        prompt_cache,
    )
}

/// Create provider with full configuration
pub fn create_provider_with_config(
    provider_name: &str,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    let factory = get_factory().lock().unwrap();
    let config = ProviderConfig {
        api_key,
        base_url,
        model,
        prompt_cache,
    };

    factory.create_provider(provider_name, config)
}
