use super::providers::{
    AnthropicProvider, DeepSeekProvider, GeminiProvider, LmStudioProvider, MinimaxProvider,
    MoonshotProvider, OllamaProvider, OpenAIProvider, OpenRouterProvider, XAIProvider, ZAIProvider,
};
use crate::config::TimeoutsConfig;
use crate::config::core::PromptCachingConfig;
use crate::config::models::{ModelId, Provider};
use crate::llm::provider::{LLMError, LLMProvider};
use std::collections::HashMap;
use std::str::FromStr;

type ProviderFactory = Box<dyn Fn(ProviderConfig) -> Box<dyn LLMProvider> + Send + Sync>;

/// LLM provider factory and registry
pub struct LLMFactory {
    providers: HashMap<String, ProviderFactory>,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub prompt_cache: Option<PromptCachingConfig>,
    pub timeouts: Option<TimeoutsConfig>,
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
            "minimax" => MinimaxProvider,
            "deepseek" => DeepSeekProvider,
            "openrouter" => OpenRouterProvider,
            "moonshot" => MoonshotProvider,
            "ollama" => OllamaProvider,
            "lmstudio" => LmStudioProvider,
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
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return None;
        }

        if trimmed.contains(':') && !trimmed.contains('/') && !trimmed.contains('@') {
            return Some("ollama".to_owned());
        }

        let m = trimmed.to_lowercase();
        if m.starts_with("gpt-oss-") || m.starts_with("gpt-") || m.starts_with("o1") {
            Some("openai".to_owned())
        } else if m.starts_with("claude-") {
            Some("anthropic".to_owned())
        } else if m.starts_with("deepseek-") {
            Some("deepseek".to_owned())
        } else if m.contains("gemini") || m.starts_with("palm") {
            Some("gemini".to_owned())
        } else if m.starts_with("grok-") || m.starts_with("xai-") {
            Some("xai".to_owned())
        } else if m.starts_with("glm-") {
            Some("zai".to_owned())
        } else if m.starts_with("lmstudio-community/") {
            Some("lmstudio".to_owned())
        } else if m.starts_with("moonshot-") || m.starts_with("kimi-") {
            Some("moonshot".to_owned())
        } else if m.contains('/') || m.contains('@') {
            Some("openrouter".to_owned())
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

    factory.create_provider(
        &provider_name,
        ProviderConfig {
            api_key: Some(api_key),
            base_url: None,
            model: Some(model.to_string()),
            prompt_cache,
            timeouts: None,
        },
    )
}

/// Create provider with full configuration
pub fn create_provider_with_config(
    provider_name: &str,
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
    timeouts: Option<TimeoutsConfig>,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    let factory = get_factory().lock().unwrap();
    factory.create_provider(
        provider_name,
        ProviderConfig {
            api_key,
            base_url,
            model,
            prompt_cache,
            timeouts,
        },
    )
}

/// Macro to implement BuiltinProvider for providers with standard from_config signature
macro_rules! impl_builtin_provider {
    ($($provider:ty),+ $(,)?) => {
        $(
            impl BuiltinProvider for $provider {
                fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
                    let ProviderConfig {
                        api_key,
                        base_url,
                        model,
                        prompt_cache,
                        timeouts,
                    } = config;

                    Box::new(<$provider>::from_config(
                        api_key,
                        model,
                        base_url,
                        prompt_cache,
                        timeouts,
                    ))
                }
            }
        )+
    };
}

// Implement BuiltinProvider for all standard providers using the macro
impl_builtin_provider!(
    GeminiProvider,
    OpenAIProvider,
    AnthropicProvider,
    MinimaxProvider,
    DeepSeekProvider,
    OpenRouterProvider,
    MoonshotProvider,
    OllamaProvider,
    LmStudioProvider,
    XAIProvider,
    ZAIProvider,
);
