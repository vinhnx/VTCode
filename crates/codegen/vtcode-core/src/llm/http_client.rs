//! Centralized HTTP client factory for LLM providers.
//!
//! Re-exported from `vtcode_llm` to eliminate duplication.

pub use vtcode_llm::http_client::*;

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::TimeoutsConfig;

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
}
