use crate::config::TimeoutsConfig;
use crate::config::core::{GeminiPromptCacheSettings, PromptCachingConfig};
use crate::llm::provider::LLMProvider;
use crate::llm::provider_builder::ProviderConfig;
use std::str::FromStr;

/// Gemini provider configuration
pub struct GeminiProviderConfig;

impl ProviderConfig for GeminiProviderConfig {
    const PROVIDER_KEY: &'static str = "gemini";
    const DISPLAY_NAME: &'static str = "Gemini";
    const DEFAULT_MODEL: &'static str =
        crate::config::constants::models::google::GEMINI_3_FLASH_PREVIEW;
    const API_BASE_URL: &'static str = crate::config::constants::urls::GEMINI_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::GEMINI_BASE_URL);

    type PromptCacheSettings = GeminiPromptCacheSettings;

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        prompt_cache_enabled: bool,
        prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::gemini::GeminiProvider;

        Box::new(GeminiProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
            prompt_cache_enabled,
            prompt_cache_settings,
        ))
    }
}

/// Anthropic provider configuration
pub struct AnthropicProviderConfig;

impl ProviderConfig for AnthropicProviderConfig {
    const PROVIDER_KEY: &'static str = "anthropic";
    const DISPLAY_NAME: &'static str = "Anthropic";
    const DEFAULT_MODEL: &'static str = crate::config::constants::models::anthropic::DEFAULT_MODEL;
    const API_BASE_URL: &'static str = crate::config::constants::urls::ANTHROPIC_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::ANTHROPIC_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::anthropic::AnthropicProvider;
        use crate::llm::providers::common::get_http_client_for_timeouts;

        Box::new(AnthropicProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// OpenAI provider configuration
pub struct OpenAIProviderConfig;

impl ProviderConfig for OpenAIProviderConfig {
    const PROVIDER_KEY: &'static str = "openai";
    const DISPLAY_NAME: &'static str = "OpenAI";
    const DEFAULT_MODEL: &'static str = crate::config::constants::models::openai::DEFAULT_MODEL;
    const API_BASE_URL: &'static str = crate::config::constants::urls::OPENAI_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::OPENAI_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::openai::OpenAIProvider;

        Box::new(OpenAIProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// DeepSeek provider configuration
pub struct DeepSeekProviderConfig;

impl ProviderConfig for DeepSeekProviderConfig {
    const PROVIDER_KEY: &'static str = "deepseek";
    const DISPLAY_NAME: &'static str = "DeepSeek";
    const DEFAULT_MODEL: &'static str = crate::config::constants::models::deepseek::DEEPSEEK_CHAT;
    const API_BASE_URL: &'static str = crate::config::constants::urls::DEEPSEEK_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::DEEPSEEK_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::deepseek::DeepSeekProvider;

        Box::new(DeepSeekProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// Moonshot provider configuration
pub struct MoonshotProviderConfig;

impl ProviderConfig for MoonshotProviderConfig {
    const PROVIDER_KEY: &'static str = "moonshot";
    const DISPLAY_NAME: &'static str = "Moonshot";
    const DEFAULT_MODEL: &'static str = "kimi-latest"; // Deprecated: use OpenRouter models instead
    const API_BASE_URL: &'static str = crate::config::constants::urls::MOONSHOT_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::MOONSHOT_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::moonshot::MoonshotProvider;

        Box::new(MoonshotProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// XAI provider configuration
pub struct XAIProviderConfig;

impl ProviderConfig for XAIProviderConfig {
    const PROVIDER_KEY: &'static str = "xai";
    const DISPLAY_NAME: &'static str = "XAI";
    const DEFAULT_MODEL: &'static str = crate::config::constants::models::xai::DEFAULT_MODEL;
    const API_BASE_URL: &'static str = crate::config::constants::urls::XAI_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::XAI_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::xai::XAIProvider;

        Box::new(XAIProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// ZAI provider configuration
pub struct ZAIProviderConfig;

impl ProviderConfig for ZAIProviderConfig {
    const PROVIDER_KEY: &'static str = "zai";
    const DISPLAY_NAME: &'static str = "Z.AI";
    const DEFAULT_MODEL: &'static str = crate::config::constants::models::zai::DEFAULT_MODEL;
    const API_BASE_URL: &'static str = crate::config::constants::urls::Z_AI_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::ZAI_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::zai::ZAIProvider;

        Box::new(ZAIProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// OpenRouter provider configuration
pub struct OpenRouterProviderConfig;

impl ProviderConfig for OpenRouterProviderConfig {
    const PROVIDER_KEY: &'static str = "openrouter";
    const DISPLAY_NAME: &'static str = "OpenRouter";
    const DEFAULT_MODEL: &'static str = "openrouter/auto";
    const API_BASE_URL: &'static str = crate::config::constants::urls::OPENROUTER_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::OPENROUTER_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::openrouter::OpenRouterProvider;

        Box::new(OpenRouterProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// Ollama provider configuration
pub struct OllamaProviderConfig;

impl ProviderConfig for OllamaProviderConfig {
    const PROVIDER_KEY: &'static str = "ollama";
    const DISPLAY_NAME: &'static str = "Ollama";
    const DEFAULT_MODEL: &'static str = "llama3.1";
    const API_BASE_URL: &'static str = "http://localhost:11434";
    const BASE_URL_ENV_VAR: Option<&'static str> = None;

    type PromptCacheSettings = ();

    fn create_provider(
        _api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::ollama::OllamaProvider;

        Box::new(OllamaProvider::new_with_client(
            _api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// LM Studio provider configuration
pub struct LmStudioProviderConfig;

impl ProviderConfig for LmStudioProviderConfig {
    const PROVIDER_KEY: &'static str = "lmstudio";
    const DISPLAY_NAME: &'static str = "LM Studio";
    const DEFAULT_MODEL: &'static str = "local-model";
    const API_BASE_URL: &'static str = "http://localhost:1234";
    const BASE_URL_ENV_VAR: Option<&'static str> = None;

    type PromptCacheSettings = ();

    fn create_provider(
        _api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::lmstudio::LmStudioProvider;

        Box::new(LmStudioProvider::new_with_client(
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// Minimax provider configuration
pub struct MinimaxProviderConfig;

impl ProviderConfig for MinimaxProviderConfig {
    const PROVIDER_KEY: &'static str = "minimax";
    const DISPLAY_NAME: &'static str = "Minimax";
    const DEFAULT_MODEL: &'static str = "abab5.5-chat";
    const API_BASE_URL: &'static str = crate::config::constants::urls::MINIMAX_API_BASE;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        Some(crate::config::constants::env_vars::MINIMAX_BASE_URL);

    type PromptCacheSettings = ();

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        _prompt_cache_enabled: bool,
        _prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider> {
        use crate::llm::providers::common::get_http_client_for_timeouts;
        use crate::llm::providers::minimax::MinimaxProvider;

        Box::new(MinimaxProvider::new_with_client(
            api_key,
            model,
            get_http_client_for_timeouts(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(timeouts.default_ceiling_seconds),
            ),
            base_url,
            timeouts,
        ))
    }
}

/// Macro to create provider builders easily
#[macro_export]
macro_rules! create_provider_builder {
    ($config_type:ty) => {
        $crate::llm::provider_builder::ProviderBuilder::<$config_type>::new()
    };
}

/// Unified provider creation function that eliminates duplication
pub fn create_provider_unified(
    provider_name: &str,
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
    timeouts: Option<TimeoutsConfig>,
) -> Result<Box<dyn LLMProvider>, crate::llm::provider::LLMError> {
    use crate::config::models::Provider;

    let provider = Provider::from_str(provider_name).map_err(|_| {
        crate::llm::provider::LLMError::InvalidRequest {
            message: format!("Unknown provider: {}", provider_name),
            metadata: None,
        }
    })?;

    match provider {
        Provider::Gemini => {
            let mut builder = create_provider_builder!(GeminiProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(cache) = prompt_cache {
                builder = builder.prompt_cache(cache);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::Anthropic => {
            let mut builder = create_provider_builder!(AnthropicProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::OpenAI => {
            let mut builder = create_provider_builder!(OpenAIProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::DeepSeek => {
            let mut builder = create_provider_builder!(DeepSeekProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::Moonshot => {
            let mut builder = create_provider_builder!(MoonshotProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::XAI => {
            let mut builder = create_provider_builder!(XAIProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::ZAI => {
            let mut builder = create_provider_builder!(ZAIProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::OpenRouter => {
            let mut builder = create_provider_builder!(OpenRouterProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::Ollama => {
            let mut builder = create_provider_builder!(OllamaProviderConfig);
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::LmStudio => {
            let mut builder = create_provider_builder!(LmStudioProviderConfig);
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::Minimax => {
            let mut builder = create_provider_builder!(MinimaxProviderConfig);
            if let Some(key) = api_key {
                builder = builder.api_key(key);
            }
            if let Some(m) = model {
                builder = builder.model(m);
            }
            if let Some(url) = base_url {
                builder = builder.base_url(url);
            }
            if let Some(t) = timeouts {
                builder = builder.timeouts(t);
            }
            Ok(builder.build())
        }
        Provider::HuggingFace => {
            // Placeholder for HuggingFace
            Err(crate::llm::provider::LLMError::Provider {
                message: "HuggingFace provider is not fully implemented in create_provider_unified"
                    .to_string(),
                metadata: None,
            })
        }
    }
}
