use super::providers::{
    AnthropicProvider, DeepSeekProvider, GeminiProvider, MoonshotProvider, OllamaProvider,
    OpenAIProvider, OpenRouterProvider, XAIProvider, ZAIProvider,
};
use crate::config::core::PromptCachingConfig;
use crate::config::models::{ModelId, Provider};
use crate::llm::provider::{LLMError, LLMProvider};
use std::collections::HashMap;
use std::str::FromStr;

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

trait BuiltinProvider: LLMProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider>;
}

macro_rules! register_providers {
    ($factory:expr, $( $name:literal => $provider:ty ),+ $(,)?) => {
        $(
            $factory.register_builtin::<$provider>($name);
        )+
    };
}

impl LLMFactory {
    pub fn new() -> Self {
        let mut factory = Self {
            providers: HashMap::new(),
        };

        // Register built-in providers using shared adapters
        register_providers!(
            factory,
            "gemini" => GeminiProvider,
            "openai" => OpenAIProvider,
            "anthropic" => AnthropicProvider,
            "deepseek" => DeepSeekProvider,
            "openrouter" => OpenRouterProvider,
            "moonshot" => MoonshotProvider,
            "ollama" => OllamaProvider,
            "xai" => XAIProvider,
            "zai" => ZAIProvider,
        );

        factory
    }

    fn register_builtin<P>(&mut self, name: &str)
    where
        P: BuiltinProvider + 'static,
    {
        self.register_provider(name, Box::new(|config| P::build_from_config(config)));
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
        if m.starts_with("gpt-oss") {
            Some("ollama".to_string())
        } else if m.starts_with("gpt-") || m.starts_with("o3") || m.starts_with("o1") {
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
        } else if m.starts_with("moonshot-") || m.starts_with("kimi-") {
            Some("moonshot".to_string())
        } else if m.contains('/') || m.contains('@') {
            Some("openrouter".to_string())
        } else {
            None
        }
    }
}

/// Infer a [`Provider`] from an optional override and model string.
///
/// Attempts, in order:
/// 1. Parse the override if provided.
/// 2. Parse the model into a [`ModelId`] and return its provider.
/// 3. Fall back to heuristic detection via [`LLMFactory::provider_from_model`].
pub fn infer_provider(override_provider: Option<&str>, model: &str) -> Option<Provider> {
    if let Some(name) = override_provider.and_then(normalize_override) {
        return Provider::from_str(name).ok();
    }

    if let Ok(model_id) = ModelId::from_str(model) {
        return Some(model_id.provider());
    }

    let factory = get_factory().lock().unwrap();
    factory
        .provider_from_model(model)
        .and_then(|name| Provider::from_str(&name).ok())
}

fn normalize_override(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
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

impl BuiltinProvider for GeminiProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
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
        ))
    }
}

impl BuiltinProvider for OpenAIProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
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
        ))
    }
}

impl BuiltinProvider for AnthropicProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
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
        ))
    }
}

impl BuiltinProvider for DeepSeekProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
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
        ))
    }
}

impl BuiltinProvider for OpenRouterProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
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
        ))
    }
}

impl BuiltinProvider for MoonshotProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
        let ProviderConfig {
            api_key,
            base_url,
            model,
            prompt_cache,
        } = config;

        Box::new(MoonshotProvider::from_config(
            api_key,
            model,
            base_url,
            prompt_cache,
        ))
    }
}

impl BuiltinProvider for OllamaProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
        let ProviderConfig {
            api_key,
            base_url,
            model,
            prompt_cache,
        } = config;

        Box::new(OllamaProvider::from_config(
            api_key,
            model,
            base_url,
            prompt_cache,
        ))
    }
}

impl BuiltinProvider for XAIProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
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
        ))
    }
}

impl BuiltinProvider for ZAIProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
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
        ))
    }
}
