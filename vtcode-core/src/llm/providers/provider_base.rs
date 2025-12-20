//! Base trait for LLM providers to eliminate constructor duplication
//!
//! This module provides a unified foundation for all LLM providers to eliminate
//! code duplication across provider implementations while maintaining the Arc<str> optimization.

use crate::config::core::{PromptCachingConfig, ProviderPromptCachingConfig};
use crate::llm::providers::common::{
    extract_prompt_cache_settings, override_base_url, resolve_model,
};
use std::sync::Arc;

/// Common configuration that all providers need
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: Arc<str>,
    pub model: Arc<str>,
    pub base_url: Arc<str>,
    pub prompt_cache_enabled: bool,
    pub prompt_cache_settings: serde_json::Value,
}

/// Trait for providers that can be built from common configuration
pub trait FromProviderConfig: Sized {
    /// Create provider from pre-built configuration
    fn from_provider_config(config: ProviderConfig) -> Self;
}

pub struct ProviderConfigDefaults<T, F>
where
    F: Fn(&ProviderPromptCachingConfig) -> &T,
{
    pub default_model: &'static str,
    pub default_base_url: &'static str,
    pub env_var_base_url: Option<&'static str>,
    pub cache_extractor: F,
    pub cache_validator: fn(&PromptCachingConfig, &T) -> bool,
}

pub struct ProviderConfigParams<T, F>
where
    F: Fn(&ProviderPromptCachingConfig) -> &T,
{
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub prompt_cache: Option<PromptCachingConfig>,
    pub defaults: ProviderConfigDefaults<T, F>,
}

/// Helper function to build provider configuration from common parameters
pub fn build_provider_config<T, F>(params: ProviderConfigParams<T, F>) -> ProviderConfig
where
    F: Fn(&ProviderPromptCachingConfig) -> &T,
    T: Clone + Default + serde::Serialize,
{
    let api_key_value = params.api_key.unwrap_or_default();
    let model_value = resolve_model(params.model, params.defaults.default_model);
    let base_url_value = override_base_url(
        params.defaults.default_base_url,
        params.base_url,
        params.defaults.env_var_base_url,
    );

    let (prompt_cache_enabled, prompt_cache_settings) =
        if let Some(ref prompt_cache) = params.prompt_cache {
            extract_prompt_cache_settings(
                Some(prompt_cache.clone()),
                |providers| (params.defaults.cache_extractor)(providers),
                params.defaults.cache_validator,
            )
        } else {
            (false, T::default())
        };

    ProviderConfig {
        api_key: Arc::from(api_key_value.as_str()),
        model: Arc::from(model_value.as_str()),
        base_url: Arc::from(base_url_value.as_str()),
        prompt_cache_enabled,
        prompt_cache_settings: serde_json::to_value(prompt_cache_settings)
            .unwrap_or(serde_json::Value::Null),
    }
}

/// Macro to generate standard provider constructors
/// This eliminates the 80% code duplication across providers
#[macro_export]
macro_rules! impl_provider_constructors {
    ($provider:ty, $default_model:expr, $default_base_url:expr, $env_var:expr, $cache_extractor:expr, $cache_validator:expr) => {
        impl $provider {
            /// Create a new provider with default model
            pub fn new(api_key: String) -> Self {
                Self::from_config(
                    Some(api_key),
                    None,
                    None,
                    None,
                    None,
                )
            }

            /// Create a new provider with specific model
            pub fn with_model(api_key: String, model: String) -> Self {
                Self::from_config(
                    Some(api_key),
                    Some(model),
                    None,
                    None,
                    None,
                )
            }
        }

        impl $provider {
            /// Create provider from configuration options
            pub fn from_config(
                api_key: Option<String>,
                model: Option<String>,
                base_url: Option<String>,
                prompt_cache: Option<$crate::config::core::PromptCachingConfig>,
                _timeouts: Option<$crate::config::TimeoutsConfig>,
                _anthropic: Option<$crate::config::core::AnthropicConfig>,
            ) -> Self {
                let config = $crate::llm::providers::provider_base::build_provider_config(
                    $crate::llm::providers::provider_base::ProviderConfigParams {
                        api_key,
                        model,
                        base_url,
                        prompt_cache,
                        defaults: $crate::llm::providers::provider_base::ProviderConfigDefaults {
                            default_model: $default_model,
                            default_base_url: $default_base_url,
                            env_var_base_url: $env_var,
                            cache_extractor: $cache_extractor,
                            cache_validator: $cache_validator,
                        },
                    },
                );

                <Self as $crate::llm::providers::provider_base::FromProviderConfig>::from_provider_config(config)
            }
        }
    };
}
