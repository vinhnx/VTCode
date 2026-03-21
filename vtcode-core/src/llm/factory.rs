use super::cgp::{CanBuildProvider, CanDescribeProvider, register_builtin_cgp_providers};
use crate::config::TimeoutsConfig;
use crate::config::core::{AnthropicConfig, ModelConfig, OpenAIConfig, PromptCachingConfig};
use crate::config::models::{ModelId, Provider};
use crate::ctx_err;
use crate::llm::provider::{LLMError, LLMProvider};
use hashbrown::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_config::auth::OpenAIChatGptAuthHandle;

type ProviderFactory = Box<dyn Fn(ProviderConfig) -> Box<dyn LLMProvider> + Send + Sync>;

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
        if m.starts_with("gpt-oss-")
            || m.starts_with("gpt-")
            || m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
            || m.starts_with("codex")
        {
            Some("openai".to_owned())
        } else if m == "copilot" || m.starts_with("copilot-") {
            Some("copilot".to_owned())
        } else if m.starts_with("claude-") {
            Some("anthropic".to_owned())
        } else if m.starts_with("deepseek-") {
            Some("deepseek".to_owned())
        } else if m.contains("gemini") || m.starts_with("palm") {
            Some("gemini".to_owned())
        } else if m.starts_with("glm-") {
            Some("zai".to_owned())
        } else if m.starts_with("litellm/") {
            Some("litellm".to_owned())
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

    // Preserve the legacy heuristic mapping for ad-hoc model strings such as
    // OpenRouter, LM Studio, or Hugging Face repo identifiers.
    if let Ok(factory) = get_factory().lock()
        && let Some(provider_name) = factory.provider_from_model(model)
        && let Ok(provider) = Provider::from_str(&provider_name)
    {
        return Some(provider);
    }

    // Fall back to ModelFamily detection for newer slugs that are not covered
    // by the historical heuristics.
    let family = crate::models_manager::find_family_for_model(model);
    (family.family != "unknown").then_some(family.provider)
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

#[cfg(test)]
mod tests {
    use super::*;
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
                "litellm",
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
