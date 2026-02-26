use crate::config::TimeoutsConfig;
use crate::config::core::PromptCachingConfig;
use crate::llm::provider::LLMProvider;
use std::marker::PhantomData;

/// Generic provider builder to eliminate duplicate provider creation patterns
pub struct ProviderBuilder<T> {
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    prompt_cache: Option<PromptCachingConfig>,
    timeouts: Option<TimeoutsConfig>,
    _phantom: PhantomData<T>,
}

impl<T> Default for ProviderBuilder<T> {
    fn default() -> Self {
        Self {
            api_key: None,
            model: None,
            base_url: None,
            prompt_cache: None,
            timeouts: None,
            _phantom: PhantomData,
        }
    }
}

impl<T> ProviderBuilder<T>
where
    T: ProviderConfig,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn api_key(mut self, api_key: String) -> Self {
        self.api_key = Some(api_key);
        self
    }

    pub fn model(mut self, model: String) -> Self {
        self.model = Some(model);
        self
    }

    pub fn base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(base_url);
        self
    }

    pub fn prompt_cache(mut self, prompt_cache: PromptCachingConfig) -> Self {
        self.prompt_cache = Some(prompt_cache);
        self
    }

    pub fn timeouts(mut self, timeouts: TimeoutsConfig) -> Self {
        self.timeouts = Some(timeouts);
        self
    }

    pub fn build(self) -> Box<dyn LLMProvider> {
        let api_key = self.api_key.unwrap_or_default();
        let model = crate::llm::providers::common::resolve_model(self.model, T::DEFAULT_MODEL);
        let timeouts = self.timeouts.unwrap_or_default();

        let (prompt_cache_enabled, _) =
            crate::llm::providers::common::extract_prompt_cache_settings_default(
                self.prompt_cache,
                T::PROVIDER_KEY,
            );
        let prompt_cache_settings = T::PromptCacheSettings::default();

        let base_url = crate::llm::providers::common::override_base_url(
            T::API_BASE_URL,
            self.base_url,
            T::BASE_URL_ENV_VAR,
        );

        T::create_provider(
            api_key,
            model,
            base_url,
            prompt_cache_enabled,
            prompt_cache_settings,
            timeouts,
        )
    }
}

/// Trait for provider-specific configuration and creation
pub trait ProviderConfig {
    const PROVIDER_KEY: &'static str;
    const DISPLAY_NAME: &'static str;
    const DEFAULT_MODEL: &'static str;
    const API_BASE_URL: &'static str;
    const BASE_URL_ENV_VAR: Option<&'static str>;

    fn create_provider(
        api_key: String,
        model: String,
        base_url: String,
        prompt_cache_enabled: bool,
        prompt_cache_settings: Self::PromptCacheSettings,
        timeouts: TimeoutsConfig,
    ) -> Box<dyn LLMProvider>
    where
        Self::PromptCacheSettings: Send + Sync + 'static;

    type PromptCacheSettings: Clone + Default + Send + Sync + 'static;
}

/// HTTP client pool to avoid creating new clients for each provider
mod http_client_pool {
    use crate::config::TimeoutsConfig;
    use once_cell::sync::Lazy;
    use reqwest::Client as HttpClient;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    type HttpClientPool = Arc<RwLock<HashMap<String, Arc<HttpClient>>>>;

    static CLIENT_POOL: Lazy<HttpClientPool> = Lazy::new(|| {
        let mut pool = HashMap::new();

        // Default client
        pool.insert("default".to_string(), Arc::new(HttpClient::new()));

        // Timeout-configured clients
        pool.insert(
            "timeout_30s".to_string(),
            Arc::new(
                HttpClient::builder()
                    .timeout(Duration::from_secs(30))
                    .build()
                    .expect("Failed to build HTTP client"),
            ),
        );

        pool.insert(
            "timeout_120s".to_string(),
            Arc::new(
                HttpClient::builder()
                    .timeout(Duration::from_secs(120))
                    .build()
                    .expect("Failed to build HTTP client"),
            ),
        );

        Arc::new(RwLock::new(pool))
    });

    pub fn get_http_client(key: &str) -> Arc<HttpClient> {
        let pool_guard = CLIENT_POOL.read();
        let pool = match pool_guard {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("HTTP client pool poisoned; continuing with recovered state");
                poisoned.into_inner()
            }
        };

        if let Some(client) = pool.get(key).cloned() {
            return client;
        }

        if let Some(default_client) = pool.get("default").cloned() {
            return default_client;
        }

        tracing::warn!("HTTP client pool missing default client; constructing transient client");
        Arc::new(HttpClient::new())
    }

    pub fn get_http_client_for_timeouts(timeouts: &TimeoutsConfig) -> Arc<HttpClient> {
        let key = if timeouts.default_ceiling_seconds >= 120 {
            "timeout_120s"
        } else if timeouts.default_ceiling_seconds >= 30 {
            "timeout_30s"
        } else {
            "default"
        };
        get_http_client(key)
    }
}

pub use http_client_pool::{get_http_client, get_http_client_for_timeouts};
