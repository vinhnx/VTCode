use crate::cgp::{CanDescribeProvider, ProviderMetadataProvider};
use crate::factory_types::{self, ProviderConfig as FactoryProviderConfig};
use crate::provider::{LLMError, LLMProvider};
use crate::provider_builder::ProviderConfig as LegacyProviderConfig;
use vtcode_config::TimeoutsConfig;
use vtcode_config::core::{GeminiPromptCacheSettings, PromptCachingConfig};

macro_rules! define_provider_config {
    ($name:ident, $key:literal, $display:literal, $default_model:expr, $api_base:expr, $env_var:expr, $prompt_cache_settings:ty) => {
        pub struct $name;

        impl ProviderMetadataProvider<$name> for $name {
            const PROVIDER_KEY: &'static str = $key;
            const DISPLAY_NAME: &'static str = $display;
            const DEFAULT_MODEL: &'static str = $default_model;
            const API_BASE_URL: &'static str = $api_base;
            const BASE_URL_ENV_VAR: Option<&'static str> = $env_var;
        }

        impl LegacyProviderConfig for $name {
            const PROVIDER_KEY: &'static str = <Self as CanDescribeProvider>::PROVIDER_KEY;
            const DISPLAY_NAME: &'static str = <Self as CanDescribeProvider>::DISPLAY_NAME;
            const DEFAULT_MODEL: &'static str = <Self as CanDescribeProvider>::DEFAULT_MODEL;
            const API_BASE_URL: &'static str = <Self as CanDescribeProvider>::API_BASE_URL;
            const BASE_URL_ENV_VAR: Option<&'static str> =
                <Self as CanDescribeProvider>::BASE_URL_ENV_VAR;

            type PromptCacheSettings = $prompt_cache_settings;
        }
    };
}

define_provider_config!(
    GeminiProviderConfig,
    "gemini",
    "Gemini",
    vtcode_config::constants::models::google::GEMINI_3_FLASH_PREVIEW,
    vtcode_config::constants::urls::GEMINI_API_BASE,
    Some(vtcode_config::constants::env_vars::GEMINI_BASE_URL),
    GeminiPromptCacheSettings
);
define_provider_config!(
    AnthropicProviderConfig,
    "anthropic",
    "Anthropic",
    vtcode_config::constants::models::anthropic::DEFAULT_MODEL,
    vtcode_config::constants::urls::ANTHROPIC_API_BASE,
    Some(vtcode_config::constants::env_vars::ANTHROPIC_BASE_URL),
    ()
);
define_provider_config!(
    CopilotProviderConfig,
    "copilot",
    "GitHub Copilot",
    vtcode_config::constants::models::copilot::DEFAULT_MODEL,
    "",
    None,
    ()
);
define_provider_config!(
    OpenAIProviderConfig,
    "openai",
    "OpenAI",
    vtcode_config::constants::models::openai::DEFAULT_MODEL,
    vtcode_config::constants::urls::OPENAI_API_BASE,
    Some(vtcode_config::constants::env_vars::OPENAI_BASE_URL),
    ()
);
define_provider_config!(
    HuggingFaceProviderConfig,
    "huggingface",
    "HuggingFace",
    vtcode_config::constants::models::huggingface::DEFAULT_MODEL,
    vtcode_config::constants::urls::HUGGINGFACE_API_BASE,
    Some(vtcode_config::constants::env_vars::HUGGINGFACE_BASE_URL),
    ()
);
define_provider_config!(
    DeepSeekProviderConfig,
    "deepseek",
    "DeepSeek",
    vtcode_config::constants::models::deepseek::DEEPSEEK_V4_PRO,
    vtcode_config::constants::urls::DEEPSEEK_API_BASE,
    Some(vtcode_config::constants::env_vars::DEEPSEEK_BASE_URL),
    ()
);
define_provider_config!(
    MistralProviderConfig,
    "mistral",
    "Mistral",
    vtcode_config::constants::models::mistral::MISTRAL_LARGE_3,
    vtcode_config::constants::urls::MISTRAL_API_BASE,
    Some(vtcode_config::constants::env_vars::MISTRAL_BASE_URL),
    ()
);
define_provider_config!(
    MoonshotProviderConfig,
    "moonshot",
    "Moonshot",
    vtcode_config::constants::models::moonshot::DEFAULT_MODEL,
    vtcode_config::constants::urls::MOONSHOT_API_BASE,
    Some(vtcode_config::constants::env_vars::MOONSHOT_BASE_URL),
    ()
);
define_provider_config!(
    ZAIProviderConfig,
    "zai",
    "Z.AI",
    vtcode_config::constants::models::zai::DEFAULT_MODEL,
    vtcode_config::constants::urls::Z_AI_API_BASE,
    Some(vtcode_config::constants::env_vars::ZAI_BASE_URL),
    ()
);
define_provider_config!(
    OpenRouterProviderConfig,
    "openrouter",
    "OpenRouter",
    "openrouter/auto",
    vtcode_config::constants::urls::OPENROUTER_API_BASE,
    Some(vtcode_config::constants::env_vars::OPENROUTER_BASE_URL),
    ()
);
define_provider_config!(
    OpenResponsesProviderConfig,
    "openresponses",
    "OpenResponses",
    vtcode_config::constants::models::openresponses::DEFAULT_MODEL,
    vtcode_config::constants::urls::OPENRESPONSES_API_BASE,
    Some(vtcode_config::constants::env_vars::OPENRESPONSES_BASE_URL),
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
    vtcode_config::constants::models::lmstudio::DEFAULT_MODEL,
    vtcode_config::constants::urls::LMSTUDIO_API_BASE,
    Some(vtcode_config::constants::env_vars::LMSTUDIO_BASE_URL),
    ()
);
define_provider_config!(
    LlamaCppProviderConfig,
    "llamacpp",
    "llama.cpp",
    vtcode_config::constants::models::llamacpp::DEFAULT_MODEL,
    vtcode_config::constants::urls::LLAMACPP_API_BASE,
    Some(vtcode_config::constants::env_vars::LLAMACPP_BASE_URL),
    ()
);
define_provider_config!(
    MiMoProviderConfig,
    "mimo",
    "Xiaomi MiMo",
    vtcode_config::constants::models::mimo::DEFAULT_MODEL,
    vtcode_config::constants::urls::MIMO_API_BASE,
    Some(vtcode_config::constants::env_vars::MIMO_BASE_URL),
    ()
);
define_provider_config!(
    MinimaxProviderConfig,
    "minimax",
    "Minimax",
    vtcode_config::constants::models::minimax::DEFAULT_MODEL,
    vtcode_config::constants::urls::MINIMAX_API_BASE,
    Some(vtcode_config::constants::env_vars::MINIMAX_BASE_URL),
    ()
);
define_provider_config!(
    OpenCodeZenProviderConfig,
    "opencode-zen",
    "OpenCode Zen",
    vtcode_config::constants::models::opencode_zen::DEFAULT_MODEL,
    vtcode_config::constants::urls::OPENCODE_ZEN_API_BASE,
    Some(vtcode_config::constants::env_vars::OPENCODE_ZEN_BASE_URL),
    ()
);
define_provider_config!(
    OpenCodeGoProviderConfig,
    "opencode-go",
    "OpenCode Go",
    vtcode_config::constants::models::opencode_go::DEFAULT_MODEL,
    vtcode_config::constants::urls::OPENCODE_GO_API_BASE,
    Some(vtcode_config::constants::env_vars::OPENCODE_GO_BASE_URL),
    ()
);
define_provider_config!(
    QwenProviderConfig,
    "qwen",
    "Qwen",
    vtcode_config::constants::models::qwen::DEFAULT_MODEL,
    vtcode_config::constants::urls::QWEN_API_BASE,
    Some(vtcode_config::constants::env_vars::QWEN_BASE_URL),
    ()
);
define_provider_config!(
    StepFunProviderConfig,
    "stepfun",
    "StepFun",
    vtcode_config::constants::models::stepfun::DEFAULT_MODEL,
    vtcode_config::constants::urls::STEPFUN_API_BASE,
    Some(vtcode_config::constants::env_vars::STEPFUN_BASE_URL),
    ()
);
define_provider_config!(
    EvolinkProviderConfig,
    "evolink",
    "Evolink",
    vtcode_config::constants::models::evolink::DEFAULT_MODEL,
    vtcode_config::constants::urls::EVOLINK_API_BASE,
    Some(vtcode_config::constants::env_vars::EVOLINK_BASE_URL),
    ()
);
define_provider_config!(
    PoolsideProviderConfig,
    "poolside",
    "Poolside",
    vtcode_config::constants::models::poolside::DEFAULT_MODEL,
    vtcode_config::constants::urls::POOLSIDE_API_BASE,
    Some(vtcode_config::constants::env_vars::POOLSIDE_BASE_URL),
    ()
);

/// Macro kept for source compatibility with older builder-based call sites.
#[macro_export]
macro_rules! create_provider_builder {
    ($config_type:ty) => {
        $crate::provider_builder::ProviderBuilder::<$config_type>::new()
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
            openai_chatgpt_auth: None,
            copilot_auth: None,
            base_url: non_empty(base_url),
            model: non_empty(model),
            prompt_cache,
            timeouts,
            openai: None,
            anthropic: None,
            model_behavior: None,
            workspace_root: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_builder::ProviderBuilder;

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
            .model(vtcode_config::constants::models::openai::DEFAULT_MODEL.to_string())
            .try_build()
            .expect("builder shim should resolve via the factory");

        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn legacy_provider_config_create_provider_routes_through_factory() {
        let provider =
            <OpenAIProviderConfig as crate::provider_builder::ProviderConfig>::create_provider(
                "test-key".to_string(),
                vtcode_config::constants::models::openai::DEFAULT_MODEL.to_string(),
                vtcode_config::constants::urls::OPENAI_API_BASE.to_string(),
                false,
                (),
                TimeoutsConfig::default(),
            );

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
                openai_chatgpt_auth: None,
                copilot_auth: None,
                base_url: Some("http://localhost:11434".to_string()),
                model: Some("gpt-oss:20b".to_string()),
                prompt_cache: None,
                timeouts: Some(TimeoutsConfig::default()),
                openai: None,
                anthropic: None,
                model_behavior: None,
                workspace_root: None,
            },
        )
        .expect("factory provider should build");

        assert_eq!(shim.name(), factory.name());
        assert_eq!(shim.supported_models(), factory.supported_models());
    }
}
