use super::providers::{
    AnthropicProvider, DeepSeekProvider, GeminiProvider, HuggingFaceProvider, LmStudioProvider,
    MinimaxProvider, MoonshotProvider, OllamaProvider, OpenAIProvider, OpenResponsesProvider,
    OpenRouterProvider, XAIProvider, ZAIProvider,
};
use crate::config::TimeoutsConfig;
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::config::models::{ModelId, Provider};
use crate::ctx_err;
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
    pub anthropic: Option<AnthropicConfig>,
    pub model_behavior: Option<ModelConfig>,
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
            "huggingface" => HuggingFaceProvider,
            "anthropic" => AnthropicProvider,
            "minimax" => MinimaxProvider,
            "deepseek" => DeepSeekProvider,
            "openrouter" => OpenRouterProvider,
            "openresponses" => OpenResponsesProvider,
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
    #[allow(clippy::result_large_err)]
    pub fn create_provider(
        &self,
        provider_name: &str,
        config: ProviderConfig,
    ) -> Result<Box<dyn LLMProvider>, LLMError> {
        let factory_fn =
            self.providers
                .get(provider_name)
                .ok_or_else(|| LLMError::InvalidRequest {
                    message: format!("Unknown provider: {}", provider_name),
                    metadata: None,
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
        } else if m.starts_with("deepseek-ai/")
            || m.starts_with("openai/gpt-oss-")
            || m.starts_with("zai-org/")
            || m.starts_with("moonshotai/")
            || m.starts_with("minimaxai/")
        {
            Some("huggingface".to_owned())
        } else if m.starts_with("mistral-")
            || m.starts_with("mixtral-")
            || m.starts_with("qwen-")
            || m.starts_with("meta-")
            || m.starts_with("llama-")
            || m.starts_with("command-")
            || m.contains('/')
            || m.contains('@')
        {
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

    let Ok(factory) = get_factory().lock() else {
        return None;
    };
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

use crate::models_manager::ModelsManager;

static FACTORY: LazyLock<Mutex<LLMFactory>> = LazyLock::new(|| Mutex::new(LLMFactory::new()));

static MODELS_MANAGER: LazyLock<ModelsManager> = LazyLock::new(ModelsManager::new);

/// Get global factory instance
pub fn get_factory() -> &'static Mutex<LLMFactory> {
    &FACTORY
}

/// Get global models manager instance
pub fn get_models_manager() -> &'static ModelsManager {
    &MODELS_MANAGER
}

/// Infer provider from model slug using ModelsManager presets.
///
/// This provides a more accurate provider resolution than heuristic-based
/// `provider_from_model` by checking against known model presets first.
pub fn infer_provider_from_model(model: &str) -> Option<Provider> {
    // First check ModelsManager presets for exact match
    let manager = get_models_manager();
    if let Ok(models) = manager.try_list_models()
        && let Some(preset) = models.iter().find(|m| m.model == model || m.id == model)
    {
        return Some(preset.provider);
    }

    // Fall back to ModelFamily detection
    let family = crate::models_manager::find_family_for_model(model);
    Some(family.provider)
}

/// Create provider from model name and API key
#[allow(clippy::result_large_err)]
pub fn create_provider_for_model(
    model: &str,
    api_key: String,
    prompt_cache: Option<PromptCachingConfig>,
    model_behavior: Option<ModelConfig>,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    // Validate model exists in ModelsManager (non-blocking check using local presets)
    if !get_models_manager().model_exists_sync(model) {
        tracing::warn!(
            model = model,
            "Model not found in ModelsManager presets, proceeding with factory heuristics"
        );
    }

    let factory = get_factory().lock().map_err(|_| LLMError::Provider {
        message: ctx_err!("llm factory", "lock poisoned"),
        metadata: None,
    })?;
    let provider_name =
        factory
            .provider_from_model(model)
            .ok_or_else(|| LLMError::InvalidRequest {
                message: format!("Cannot determine provider for model: {}", model),
                metadata: None,
            })?;

    factory.create_provider(
        &provider_name,
        ProviderConfig {
            api_key: Some(api_key),
            base_url: None,
            model: Some(model.to_string()),
            prompt_cache,
            timeouts: None,
            anthropic: None,
            model_behavior,
        },
    )
}

/// Create provider with full configuration
#[allow(clippy::result_large_err)]
pub fn create_provider_with_config(
    provider_name: &str,
    config: ProviderConfig,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    let factory = get_factory().lock().map_err(|_| LLMError::Provider {
        message: ctx_err!("llm factory", "lock poisoned"),
        metadata: None,
    })?;
    factory.create_provider(provider_name, config)
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
                        anthropic,
                        model_behavior,
                    } = config;

                    Box::new(<$provider>::from_config(
                        api_key,
                        model,
                        base_url,
                        prompt_cache,
                        timeouts,
                        anthropic,
                        model_behavior,
                    ))
                }
            }
        )+
    };
}

// Manual implementation for AnthropicProvider to support provider-specific config
impl BuiltinProvider for AnthropicProvider {
    fn build_from_config(config: ProviderConfig) -> Box<dyn LLMProvider> {
        let ProviderConfig {
            api_key,
            base_url,
            model,
            prompt_cache,
            timeouts,
            anthropic,
            model_behavior,
        } = config;

        Box::new(AnthropicProvider::from_config(
            api_key,
            model,
            base_url,
            prompt_cache,
            timeouts,
            anthropic,
            model_behavior,
        ))
    }
}

// Implement BuiltinProvider for all standard providers using the macro
impl_builtin_provider!(
    GeminiProvider,
    OpenAIProvider,
    HuggingFaceProvider,
    // AnthropicProvider is manually implemented above
    MinimaxProvider,
    DeepSeekProvider,
    OpenRouterProvider,
    OpenResponsesProvider,
    MoonshotProvider,
    OllamaProvider,
    LmStudioProvider,
    XAIProvider,
    ZAIProvider,
);
