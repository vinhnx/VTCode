use crate::config::TimeoutsConfig;
use crate::config::core::{GeminiPromptCacheSettings, PromptCachingConfig};
use crate::llm::factory::{self, ProviderConfig as FactoryProviderConfig};
use crate::llm::provider::{LLMError, LLMProvider};
use crate::llm::provider_builder::ProviderConfig;

macro_rules! define_provider_config {
    ($name:ident, $key:literal, $display:literal, $default_model:expr, $api_base:expr, $env_var:expr, $prompt_cache_settings:ty) => {
        pub struct $name;

        impl ProviderConfig for $name {
            const PROVIDER_KEY: &'static str = $key;
            const DISPLAY_NAME: &'static str = $display;
            const DEFAULT_MODEL: &'static str = $default_model;
            const API_BASE_URL: &'static str = $api_base;
            const BASE_URL_ENV_VAR: Option<&'static str> = $env_var;

            type PromptCacheSettings = $prompt_cache_settings;
        }
    };
}

define_provider_config!(
    GeminiProviderConfig,
    "gemini",
    "Gemini",
    crate::config::constants::models::google::GEMINI_3_FLASH_PREVIEW,
    crate::config::constants::urls::GEMINI_API_BASE,
    Some(crate::config::constants::env_vars::GEMINI_BASE_URL),
    GeminiPromptCacheSettings
);
define_provider_config!(
    AnthropicProviderConfig,
    "anthropic",
    "Anthropic",
    crate::config::constants::models::anthropic::DEFAULT_MODEL,
    crate::config::constants::urls::ANTHROPIC_API_BASE,
    Some(crate::config::constants::env_vars::ANTHROPIC_BASE_URL),
    ()
);
define_provider_config!(
    OpenAIProviderConfig,
    "openai",
    "OpenAI",
    crate::config::constants::models::openai::DEFAULT_MODEL,
    crate::config::constants::urls::OPENAI_API_BASE,
    Some(crate::config::constants::env_vars::OPENAI_BASE_URL),
    ()
);
define_provider_config!(
    DeepSeekProviderConfig,
    "deepseek",
    "DeepSeek",
    crate::config::constants::models::deepseek::DEEPSEEK_CHAT,
    crate::config::constants::urls::DEEPSEEK_API_BASE,
    Some(crate::config::constants::env_vars::DEEPSEEK_BASE_URL),
    ()
);
define_provider_config!(
    MoonshotProviderConfig,
    "moonshot",
    "Moonshot",
    "kimi-latest",
    crate::config::constants::urls::MOONSHOT_API_BASE,
    Some(crate::config::constants::env_vars::MOONSHOT_BASE_URL),
    ()
);
define_provider_config!(
    ZAIProviderConfig,
    "zai",
    "Z.AI",
    crate::config::constants::models::zai::DEFAULT_MODEL,
    crate::config::constants::urls::Z_AI_API_BASE,
    Some(crate::config::constants::env_vars::ZAI_BASE_URL),
    ()
);
define_provider_config!(
    OpenRouterProviderConfig,
    "openrouter",
    "OpenRouter",
    "openrouter/auto",
    crate::config::constants::urls::OPENROUTER_API_BASE,
    Some(crate::config::constants::env_vars::OPENROUTER_BASE_URL),
    ()
);
define_provider_config!(
    OllamaProviderConfig,
    "ollama",
    "Ollama",
    "gpt-oss:20b",
    "http://localhost:11434",
    None,
    ()
);
define_provider_config!(
    LmStudioProviderConfig,
    "lmstudio",
    "LM Studio",
    "local-model",
    "http://localhost:1234",
    None,
    ()
);
define_provider_config!(
    MinimaxProviderConfig,
    "minimax",
    "Minimax",
    crate::config::constants::models::minimax::DEFAULT_MODEL,
    crate::config::constants::urls::MINIMAX_API_BASE,
    Some(crate::config::constants::env_vars::MINIMAX_BASE_URL),
    ()
);

/// Macro kept for source compatibility with older builder-based call sites.
#[macro_export]
macro_rules! create_provider_builder {
    ($config_type:ty) => {
        $crate::llm::provider_builder::ProviderBuilder::<$config_type>::new()
    };
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else if trimmed.len() == value.len() {
            Some(value)
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Compatibility shim that forwards legacy builder/config callers into the
/// canonical provider factory.
pub fn create_provider_unified(
    provider_name: &str,
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
    timeouts: Option<TimeoutsConfig>,
) -> Result<Box<dyn LLMProvider>, LLMError> {
    factory::create_provider_with_config(
        provider_name,
        FactoryProviderConfig {
            api_key: non_empty(api_key),
            base_url: non_empty(base_url),
            model: non_empty(model),
            prompt_cache,
            timeouts,
            openai: None,
            anthropic: None,
            model_behavior: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider_builder::ProviderBuilder;

    #[test]
    fn unified_provider_creation_supports_huggingface_through_factory() {
        let provider = create_provider_unified(
            "huggingface",
            Some("test-key".to_string()),
            Some("openai/gpt-oss-20b".to_string()),
            None,
            None,
            None,
        )
        .expect("shim should route through the factory");

        assert_eq!(provider.name(), "huggingface");
    }

    #[test]
    fn builder_shim_routes_through_factory() {
        let provider = ProviderBuilder::<OpenAIProviderConfig>::new()
            .api_key("test-key".to_string())
            .model(crate::config::constants::models::openai::DEFAULT_MODEL.to_string())
            .try_build()
            .expect("builder shim should resolve via the factory");

        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn unified_provider_creation_matches_factory_behavior() {
        let shim = create_provider_unified(
            "ollama",
            None,
            Some("gpt-oss:20b".to_string()),
            Some("http://localhost:11434".to_string()),
            None,
            Some(TimeoutsConfig::default()),
        )
        .expect("shim provider should build");

        let factory = factory::create_provider_with_config(
            "ollama",
            FactoryProviderConfig {
                api_key: None,
                base_url: Some("http://localhost:11434".to_string()),
                model: Some("gpt-oss:20b".to_string()),
                prompt_cache: None,
                timeouts: Some(TimeoutsConfig::default()),
                openai: None,
                anthropic: None,
                model_behavior: None,
            },
        )
        .expect("factory provider should build");

        assert_eq!(shim.name(), factory.name());
        assert_eq!(shim.supported_models(), factory.supported_models());
    }
}
