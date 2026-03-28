use super::cgp::{CanBuildProvider, CanDescribeProvider, register_builtin_cgp_providers};
use super::model_resolver::{ModelResolver, heuristic_provider_from_model};
use crate::config::TimeoutsConfig;
use crate::config::core::{AnthropicConfig, ModelConfig, OpenAIConfig, PromptCachingConfig};
use crate::config::models::Provider;
use crate::ctx_err;
use crate::llm::provider::{LLMError, LLMProvider};
use hashbrown::HashMap;
use std::path::PathBuf;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_config::auth::OpenAIChatGptAuthHandle;

type ProviderFactory = Box<dyn Fn(ProviderConfig) -> Box<dyn LLMProvider> + Send + Sync>;

const BUILTIN_PROVIDER_KEYS: &[&str] = &[
    "openai",
    "anthropic",
    "gemini",
    "copilot",
    "deepseek",
    "openrouter",
    "ollama",
    "lmstudio",
    "moonshot",
    "zai",
    "minimax",
    "huggingface",
    "openresponses",
];

/// LLM provider factory and registry
pub struct LLMFactory {
    providers: HashMap<String, ProviderFactory>,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    pub copilot_auth: Option<CopilotAuthConfig>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub prompt_cache: Option<PromptCachingConfig>,
    pub timeouts: Option<TimeoutsConfig>,
    pub openai: Option<OpenAIConfig>,
    pub anthropic: Option<AnthropicConfig>,
    pub model_behavior: Option<ModelConfig>,
    pub workspace_root: Option<PathBuf>,
}

impl LLMFactory {
    pub fn new() -> Self {
        let mut factory = Self {
            providers: HashMap::new(),
        };

        register_builtin_cgp_providers(&mut factory);

        factory
    }

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
/// 2. Parse the model into a [`ModelId`] and return its provider.
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

/// Infer provider from model slug using ModelsManager presets.
///
/// This provides a more accurate provider resolution than heuristic-based
/// `provider_from_model` by checking against known model presets first.
pub fn infer_provider_from_model(model: &str) -> Option<Provider> {
    ModelResolver::resolve_provider(None, model, &[]).or_else(|| {
        let family = crate::models_manager::find_family_for_model(model);
        (family.family != "unknown").then_some(family.provider)
    })
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

    let provider_name = infer_provider_from_model(model)
        .map(|provider| provider.to_string())
        .ok_or_else(|| LLMError::InvalidRequest {
            message: format!("Cannot determine provider for model: {}", model),
            metadata: None,
        })?;
    let factory = get_factory().lock().map_err(|_| LLMError::Provider {
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
            model_behavior,
            workspace_root: None,
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

/// Register custom OpenAI-compatible providers from config into the global factory.
///
/// This performs a sync/replace: previously registered custom providers are
/// removed first, then the new set is registered. Built-in providers are
/// never touched.
pub fn register_custom_providers(custom_providers: &[vtcode_config::core::CustomProviderConfig]) {
    if custom_providers.is_empty() {
        return;
    }

    use crate::llm::providers::OpenAIProvider;

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
        let display_name = cp.display_name.clone();
        let default_base_url = cp.base_url.clone();
        let default_model = cp.model.clone();
        let api_key_env = cp.resolved_api_key_env();
        let reg_key = key.clone();

        factory.register_provider(&reg_key, move |config: ProviderConfig| {
            // Resolve API key: explicit config > env var > empty
            let api_key = config
                .api_key
                .clone()
                .or_else(|| std::env::var(&api_key_env).ok());

            let model = config
                .model
                .clone()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| default_model.clone());

            let base_url = config
                .base_url
                .clone()
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| default_base_url.clone());

            Box::new(OpenAIProvider::from_custom_config(
                key.clone(),
                display_name.clone(),
                api_key,
                Some(model),
                Some(base_url),
                config.prompt_cache,
                config.timeouts,
                config.openai,
                config.model_behavior,
            ))
        });

        tracing::trace!(
            provider = cp.name,
            display_name = cp.display_name,
            "Registered custom OpenAI-compatible provider"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::core::CustomProviderConfig;
    use crate::config::core::{AnthropicConfig, OpenAIConfig};
    use crate::llm::provider_config::{
        AnthropicProviderConfig, GeminiProviderConfig, OpenAIProviderConfig,
    };
    use crate::llm::providers::OllamaProvider;

    #[test]
    fn builtin_cgp_registration_exposes_expected_provider_keys() {
        let factory = LLMFactory::new();
        let mut providers = factory.list_providers();
        providers.sort();

        assert_eq!(
            providers,
            vec![
                "anthropic",
                "copilot",
                "deepseek",
                "gemini",
                "huggingface",
                "lmstudio",
                "minimax",
                "moonshot",
                "ollama",
                "openai",
                "openresponses",
                "openrouter",
                "zai",
            ]
        );
    }

    #[test]
    fn standard_provider_builds_through_cgp_registration() {
        let factory = LLMFactory::new();
        let provider = factory
            .create_provider(
                <GeminiProviderConfig as CanDescribeProvider>::PROVIDER_KEY,
                ProviderConfig {
                    api_key: Some("test-key".to_string()),
                    openai_chatgpt_auth: None,
                    copilot_auth: None,
                    base_url: None,
                    model: Some(
                        crate::config::constants::models::google::GEMINI_3_FLASH_PREVIEW
                            .to_string(),
                    ),
                    prompt_cache: None,
                    timeouts: None,
                    openai: None,
                    anthropic: None,
                    model_behavior: None,
                    workspace_root: None,
                },
            )
            .expect("built-in cgp registration should build");

        assert_eq!(provider.name(), "gemini");
    }

    #[test]
    fn openai_build_preserves_provider_specific_config_path() {
        let factory = LLMFactory::new();
        let provider = factory
            .create_provider(
                <OpenAIProviderConfig as CanDescribeProvider>::PROVIDER_KEY,
                ProviderConfig {
                    api_key: Some("test-key".to_string()),
                    openai_chatgpt_auth: None,
                    copilot_auth: None,
                    base_url: None,
                    model: Some(
                        crate::config::constants::models::openai::DEFAULT_MODEL.to_string(),
                    ),
                    prompt_cache: None,
                    timeouts: None,
                    openai: Some(OpenAIConfig {
                        websocket_mode: true,
                        ..OpenAIConfig::default()
                    }),
                    anthropic: Some(AnthropicConfig::default()),
                    model_behavior: None,
                    workspace_root: None,
                },
            )
            .expect("openai cgp registration should build");

        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn anthropic_build_preserves_provider_specific_config_path() {
        let factory = LLMFactory::new();
        let provider = factory
            .create_provider(
                <AnthropicProviderConfig as CanDescribeProvider>::PROVIDER_KEY,
                ProviderConfig {
                    api_key: Some("test-key".to_string()),
                    openai_chatgpt_auth: None,
                    copilot_auth: None,
                    base_url: None,
                    model: Some(
                        crate::config::constants::models::anthropic::DEFAULT_MODEL.to_string(),
                    ),
                    prompt_cache: None,
                    timeouts: None,
                    openai: None,
                    anthropic: Some(AnthropicConfig {
                        count_tokens_enabled: true,
                        ..AnthropicConfig::default()
                    }),
                    model_behavior: None,
                    workspace_root: None,
                },
            )
            .expect("anthropic cgp registration should build");

        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    fn custom_provider_registration_still_coexists_with_cgp_builtins() {
        let mut factory = LLMFactory::new();
        factory.register_provider("custom-test", |_config| {
            Box::new(OllamaProvider::from_config(
                None,
                Some("gpt-oss:20b".to_string()),
                Some("http://localhost:11434".to_string()),
                None,
                None,
                None,
                None,
            ))
        });

        let custom = factory
            .create_provider(
                "custom-test",
                ProviderConfig {
                    api_key: None,
                    openai_chatgpt_auth: None,
                    copilot_auth: None,
                    base_url: None,
                    model: None,
                    prompt_cache: None,
                    timeouts: None,
                    openai: None,
                    anthropic: None,
                    model_behavior: None,
                    workspace_root: None,
                },
            )
            .expect("custom provider should still register");
        let builtin = factory
            .create_provider(
                "openai",
                ProviderConfig {
                    api_key: Some("test-key".to_string()),
                    openai_chatgpt_auth: None,
                    copilot_auth: None,
                    base_url: None,
                    model: Some(
                        crate::config::constants::models::openai::DEFAULT_MODEL.to_string(),
                    ),
                    prompt_cache: None,
                    timeouts: None,
                    openai: None,
                    anthropic: None,
                    model_behavior: None,
                    workspace_root: None,
                },
            )
            .expect("builtin provider should still build");

        assert_eq!(custom.name(), "ollama");
        assert_eq!(builtin.name(), "openai");
    }

    #[test]
    fn custom_openai_compatible_provider_uses_configured_display_name() {
        register_custom_providers(&[CustomProviderConfig {
            name: "mycorp".to_string(),
            display_name: "MyCorporateName".to_string(),
            base_url: "https://llm.corp.example/v1".to_string(),
            api_key_env: "MYCORP_API_KEY".to_string(),
            model: "gpt-5-mini".to_string(),
        }]);

        let provider = create_provider_with_config(
            "mycorp",
            ProviderConfig {
                api_key: None,
                openai_chatgpt_auth: None,
                copilot_auth: None,
                base_url: None,
                model: Some("gpt-5-mini".to_string()),
                prompt_cache: None,
                timeouts: None,
                openai: Some(OpenAIConfig::default()),
                anthropic: None,
                model_behavior: None,
                workspace_root: None,
            },
        )
        .expect("custom provider should register");

        assert_eq!(provider.name(), "mycorp");
        assert_eq!(provider.supported_models(), vec!["gpt-5-mini".to_string()]);

        if let Ok(mut factory) = get_factory().lock() {
            factory.remove_provider("mycorp");
        }
    }

    #[test]
    fn create_provider_for_bare_minimax_model_uses_minimax_provider() {
        let provider =
            create_provider_for_model("MiniMax-M2.5", "test-key".to_string(), None, None)
                .expect("bare minimax model should resolve to minimax provider");

        assert_eq!(provider.name(), "minimax");
    }

    #[test]
    fn create_provider_for_mistral_model_uses_openrouter_provider() {
        let provider =
            create_provider_for_model("mistral-large", "test-key".to_string(), None, None)
                .expect("mistral models should still resolve through openrouter");

        assert_eq!(provider.name(), "openrouter");
    }

    #[test]
    fn create_provider_for_huggingface_repo_id_uses_huggingface_provider() {
        let provider =
            create_provider_for_model("openai/gpt-oss-20b", "test-key".to_string(), None, None)
                .expect("repo identifiers should preserve huggingface routing");

        assert_eq!(provider.name(), "huggingface");
    }

    #[test]
    fn create_provider_for_unknown_model_returns_error() {
        match create_provider_for_model("totally-unknown-model", "test-key".to_string(), None, None)
        {
            Err(LLMError::InvalidRequest { .. }) => {}
            Err(error) => panic!("expected invalid request error, got {error:?}"),
            Ok(_) => panic!("unknown models should remain rejected"),
        }
    }
}
