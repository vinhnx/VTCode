//! Centralized HTTP client factory for LLM providers.
//!
//! Provides consistent timeout configuration and client creation
//! across all providers based on the application's TimeoutsConfig.

use reqwest::Client;
use std::time::Duration;

use vtcode_config::TimeoutsConfig;

/// Factory for creating pre-configured HTTP clients.
pub struct HttpClientFactory;

impl HttpClientFactory {
    /// Create an HTTP client for standard LLM API requests.
    ///
    /// The request budget comes from `default_ceiling_seconds`; the connect phase
    /// comes from `http_connect_seconds`, so a slow or high-latency link can be
    /// tuned independently of how long a completion may run.
    pub fn for_llm(config: &TimeoutsConfig) -> Client {
        vtcode_commons::http::create_client_with_timeouts(Self::connect_timeout(config), Self::llm_timeout(config))
    }

    pub(crate) fn for_llm_without_redirects(config: &TimeoutsConfig) -> Result<Client, reqwest::Error> {
        vtcode_commons::http::create_client_without_redirects(Self::connect_timeout(config), Self::llm_timeout(config))
    }

    fn llm_timeout(config: &TimeoutsConfig) -> Duration {
        let timeout_secs = if config.default_ceiling_seconds > 0 {
            config.default_ceiling_seconds
        } else {
            180 // Fallback if disabled
        };

        Duration::from_secs(timeout_secs)
    }

    /// Connect budget from `[timeouts] http_connect_seconds`, falling back to the
    /// historical 30s when the key is set to `0` (disabled), because reqwest needs
    /// a concrete duration at this call site.
    fn connect_timeout(config: &TimeoutsConfig) -> Duration {
        match config.ceiling_duration(config.http_connect_seconds) {
            Some(connect) => connect,
            None => Duration::from_secs(30),
        }
    }

    /// Create an HTTP client optimized for streaming requests.
    ///
    /// Uses the `streaming_ceiling_seconds` from TimeoutsConfig.
    /// Streaming requests typically take longer as they wait for incremental output.
    pub fn for_streaming(config: &TimeoutsConfig) -> Client {
        let timeout_secs = if config.streaming_ceiling_seconds > 0 {
            config.streaming_ceiling_seconds
        } else {
            600 // 10 minutes fallback for streaming
        };

        Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .connect_timeout(Self::connect_timeout(config))
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .pool_max_idle_per_host(1)
            .build()
            .unwrap_or_else(|_| vtcode_commons::http::create_streaming_client())
    }

    /// Create an HTTP client with explicit timeout values.
    ///
    /// Use this when providers need custom timeout handling.
    pub(crate) fn with_timeouts(request_timeout: Duration, connect_timeout: Duration) -> Client {
        vtcode_commons::http::create_client_with_timeouts(connect_timeout, request_timeout)
    }

    /// Create a default HTTP client with reasonable defaults.
    ///
    /// Uses 180s request timeout and 30s connect timeout.
    pub fn default_client() -> Client {
        vtcode_commons::http::create_client_with_timeouts(Duration::from_secs(30), Duration::from_secs(180))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_client_is_created() {
        let client = HttpClientFactory::default_client();
        // Just verify it doesn't panic
        drop(client);
    }

    #[test]
    fn for_llm_uses_config_timeout() {
        let config = TimeoutsConfig::default();
        let client = HttpClientFactory::for_llm(&config);
        drop(client);
    }

    #[test]
    fn for_streaming_uses_longer_timeout() {
        let config = TimeoutsConfig::default();
        let client = HttpClientFactory::for_streaming(&config);
        drop(client);
    }

    #[test]
    fn connect_timeout_follows_the_configured_http_key() {
        let config = TimeoutsConfig { http_connect_seconds: 45, ..Default::default() };
        assert_eq!(HttpClientFactory::connect_timeout(&config), Duration::from_secs(45));
    }

    #[test]
    fn connect_timeout_falls_back_when_disabled() {
        // `0` means "no limit" in `[timeouts]`, but reqwest still needs a
        // concrete connect duration here, so the historical 30s applies.
        let config = TimeoutsConfig { http_connect_seconds: 0, ..Default::default() };
        assert_eq!(HttpClientFactory::connect_timeout(&config), Duration::from_secs(30));
    }
}
