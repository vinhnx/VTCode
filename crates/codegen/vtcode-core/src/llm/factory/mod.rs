use super::cgp::{CanBuildProvider, CanDescribeProvider, register_builtin_cgp_providers};
use super::model_resolver::{ModelResolver, heuristic_provider_from_model};
use super::provider::{LLMError, LLMProvider};
use hashbrown::HashMap;
use vtcode_commons::ctx_err;
use vtcode_config::core::{ModelConfig, PromptCachingConfig};
use vtcode_config::models::Provider;
use vtcode_llm::providers::CustomProviderBackendRouter;
use vtcode_llm::providers::openai::CustomProviderAuthHandle;

// ProviderConfig is the canonical factory config imported from vtcode-llm.
// The struct was consolidated here to eliminate the duplicate definition.
pub use vtcode_llm::provider_config_types::ProviderConfig;

/// Factory closure producing a boxed provider from config.
///
/// Two dynamic-dispatch layers are intentional here, not accidental: the outer
/// `Box<dyn Fn>` lets the registry hold heterogeneous constructors selected at
/// runtime, and the returned `Box<dyn LLMProvider>` is the single vtable layer
/// for the heterogeneous provider collection. Provider wrappers below this
/// point must NOT add further `Box<dyn LLMProvider>` layers — embed the
/// concrete inner provider instead (see `LlamaCppProvider`/`LmStudioProvider`)
/// so each call pays at most one vtable lookup.
type ProviderFactory = Box<dyn Fn(ProviderConfig) -> Box<dyn LLMProvider> + Send + Sync>;

fn resolve_custom_provider_api_key(
    api_key: Option<String>,
    command_auth_configured: bool,
    api_key_env: &str,
) -> Option<String> {
    api_key
        .filter(|value| !value.trim().is_empty())
        .or_else(|| (!command_auth_configured).then(|| std::env::var(api_key_env).ok()).flatten())
}

const BUILTIN_PROVIDER_KEYS: &[&str] = &[
    "openai",
    "anthropic",
    "gemini",
    "copilot",
    "deepseek",
    "meta",
    "openrouter",
    "ollama",
    "ollama-cloud",
    "lmstudio",
    "llamacpp",
    "moonshot",
    "zai",
    "minimax",
    "mimo",
    "mistral",
    "huggingface",
    "openresponses",
    "opencode-zen",
    "opencode-go",
    "qwen",
    "stepfun",
    "evolink",
    "poolside",
    "xai",
    "nvidia",
    "merge-gateway",
    "vercel",
];

/// LLM provider factory and registry
pub struct LLMFactory {
    providers: HashMap<String, ProviderFactory>,
}

impl LLMFactory {
    /// Create a new factory pre-populated with all built-in provider registrations.
    pub fn new() -> Self {
        let mut factory = Self { providers: HashMap::new() };

        register_builtin_cgp_providers(&mut factory);

        factory
    }

    /// Register a provider discovered through the CGP (Context-Generic Provider) substrate.
    pub fn register_cgp_provider<Ctx>(&mut self)
    where
        Ctx: CanDescribeProvider + CanBuildProvider + 'static,
    {
        self.register_provider(Ctx::PROVIDER_KEY, Ctx::build_provider);
    }

    /// Register a new provider
    pub fn register_provider<F>(&mut self, name: &str, factory_fn: F)
    where
        F: Fn(ProviderConfig) -> Box<dyn LLMProvider> + Send + Sync + 'static,
    {
        self.providers.insert(name.to_string(), Box::new(factory_fn));
    }

    /// Create provider instance
    pub fn create_provider(
        &self,
        provider_name: &str,
        config: ProviderConfig,
        whitelist: &[String],
    ) -> Result<Box<dyn LLMProvider>, LLMError> {
        if !whitelist.is_empty() && !whitelist.iter().any(|w| w.eq_ignore_ascii_case(provider_name)) {
            return Err(LLMError::InvalidRequest {
                message: format!("Provider '{}' is not in providers_whitelist", provider_name),
                metadata: None,
            });
        }

        let factory_fn = self
            .providers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(provider_name))
            .map(|(_, factory_fn)| factory_fn)
            .ok_or_else(|| {
                let mut available: Vec<&str> = self.providers.keys().map(String::as_str).collect();
                available.sort_unstable();
                LLMError::InvalidRequest {
                    message: format!(
                        "Unknown provider: {provider_name}. Available providers: {}",
                        available.join(", ")
                    ),
                    metadata: None,
                }
            })?;

        Ok(factory_fn(config))
    }

    /// List available providers
    pub fn list_providers(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Remove a provider registration by name.
    pub fn remove_provider(&mut self, name: &str) {
        self.providers.remove(name);
    }

    /// Determine provider name from model string
    pub fn provider_from_model(&self, model: &str) -> Option<String> {
        heuristic_provider_from_model(model).map(|provider| provider.to_string())
    }
}

/// Infer a [`Provider`] from an optional override and model string.
///
/// Attempts, in order:
/// 1. Parse the override if provided.
/// 2. Parse the model into a [`vtcode_config::models::ModelId`] and return its provider.
/// 3. Fall back to heuristic detection via [`LLMFactory::provider_from_model`].
pub fn infer_provider(override_provider: Option<&str>, model: &str) -> Option<Provider> {
    ModelResolver::resolve_provider(override_provider, model, &[])
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

/// Infer provider from model slug.
///
/// Canonical implementation lives in `vtcode-llm::factory_types` (it needs no
/// vtcode-core state); this re-export keeps the historical
/// `crate::llm::factory::infer_provider_from_model` path stable.
pub use vtcode_llm::factory_types::infer_provider_from_model;

/// Create provider from model name and API key
pub fn create_provider_for_model(
    model: &str,
    api_key: String,
    prompt_cache: Option<PromptCachingConfig>,
    model_behavior: Option<ModelConfig>,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    // Validate model exists in ModelsManager (non-blocking check using local presets)
    if !get_models_manager().model_exists_sync(model) {
        tracing::warn!(model = model, "Model not found in ModelsManager presets, proceeding with factory heuristics");
    }

    let provider_name = infer_provider_from_model(model)
        .map(|provider| provider.to_string())
        .ok_or_else(|| LLMError::InvalidRequest {
            message: format!("Cannot determine provider for model: {model}"),
            metadata: None,
        })?;
    let factory = get_factory().lock().map_err(|_e| LLMError::Provider {
        message: ctx_err!("llm factory", "lock poisoned"),
        metadata: None,
    })?;

    factory.create_provider(
        &provider_name,
        ProviderConfig {
            api_key: Some(api_key),
            openai_chatgpt_auth: None,
            copilot_auth: None,
            base_url: None,
            model: Some(model.to_string()),
            prompt_cache,
            timeouts: None,
            openai: None,
            anthropic: None,
            openrouter: None,
            model_behavior,
            workspace_root: None,
        },
        &[],
    )
}

/// Create provider with full configuration
pub fn create_provider_with_config(
    provider_name: &str,
    config: ProviderConfig,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    let factory = get_factory().lock().map_err(|_e| LLMError::Provider {
        message: ctx_err!("llm factory", "lock poisoned"),
        metadata: None,
    })?;
    factory.create_provider(provider_name, config, &[])
}

/// Register custom OpenAI-compatible providers from config into the global factory.
///
/// This performs a sync/replace: previously registered custom providers are
/// removed first, then the new set is registered. Built-in providers are
/// never touched.
pub fn register_custom_providers(custom_providers: &[vtcode_config::core::CustomProviderConfig]) {
    let Ok(mut factory) = get_factory().lock() else {
        tracing::error!("Failed to lock LLM factory for custom provider registration");
        return;
    };

    // Remove previously registered custom providers (anything not built-in)
    let registered: Vec<String> = factory.list_providers();
    for key in &registered {
        if !BUILTIN_PROVIDER_KEYS.contains(&key.as_str()) {
            factory.remove_provider(key);
        }
    }

    // Register each custom provider
    for cp in custom_providers {
        if let Err(msg) = cp.validate() {
            tracing::warn!("Skipping invalid custom provider: {msg}");
            continue;
        }

        let key = cp.name.to_lowercase();
        let custom_config = cp.clone();
        let display_name = cp.display_name.clone();
        let default_model = cp.model.clone();
        let auth_config = cp.auth.clone();
        let api_key_env = cp.resolved_api_key_env();
        let reg_key = key.clone();

        factory.register_provider(&reg_key, move |config: ProviderConfig| {
            let ProviderConfig {
                api_key,
                base_url,
                model,
                prompt_cache,
                timeouts,
                openai,
                anthropic,
                model_behavior,
                workspace_root,
                ..
            } = config;

            let api_key = resolve_custom_provider_api_key(api_key, auth_config.is_some(), &api_key_env);

            let model = model.filter(|m| !m.trim().is_empty()).unwrap_or_else(|| default_model.clone());
            let base_url = base_url
                .clone()
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| custom_config.base_url.clone());
            let custom_provider_auth = auth_config
                .clone()
                .filter(|_| api_key.is_none())
                .map(|auth| CustomProviderAuthHandle::new(auth, workspace_root.clone()));

            Box::new(CustomProviderBackendRouter::from_config(
                custom_config.clone(),
                api_key,
                Some(model),
                base_url,
                prompt_cache,
                timeouts,
                openai,
                anthropic,
                model_behavior,
                custom_provider_auth,
            ))
        });

        tracing::trace!(provider = key, display_name = display_name, "Registered custom provider");
    }
}

#[cfg(test)]
mod tests;
