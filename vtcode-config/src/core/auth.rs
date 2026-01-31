//! Authentication configuration for VT Code.
//!
//! This module provides configuration for OAuth-based authentication
//! with LLM providers that support it.

use serde::{Deserialize, Serialize};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// Authentication configuration for all providers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AuthConfig {
    /// OpenRouter OAuth configuration
    #[serde(default)]
    pub openrouter: OpenRouterAuthConfig,
}

/// OpenRouter-specific authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(default)]
pub struct OpenRouterAuthConfig {
    /// Whether to use OAuth instead of API key for authentication.
    /// When enabled, VT Code will prompt for OAuth login if no valid token exists.
    pub use_oauth: bool,

    /// Port for the local OAuth callback server.
    /// The server listens on localhost for the OAuth redirect.
    pub callback_port: u16,

    /// Whether to automatically refresh tokens when they expire.
    /// If false, the user will be prompted to re-authenticate.
    pub auto_refresh: bool,

    /// Timeout in seconds for the OAuth flow.
    /// If the user doesn't complete authentication within this time, the flow is cancelled.
    pub flow_timeout_secs: u64,
}

impl Default for OpenRouterAuthConfig {
    fn default() -> Self {
        Self {
            use_oauth: false,
            callback_port: 8484,
            auto_refresh: true,
            flow_timeout_secs: 300,
        }
    }
}

impl OpenRouterAuthConfig {
    /// Check if OAuth is enabled and should be used.
    pub fn should_use_oauth(&self) -> bool {
        self.use_oauth
    }

    /// Get the callback URL for the OAuth flow.
    pub fn callback_url(&self) -> String {
        format!("http://localhost:{}/callback", self.callback_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OpenRouterAuthConfig::default();
        assert!(!config.use_oauth);
        assert_eq!(config.callback_port, 8484);
        assert!(config.auto_refresh);
        assert_eq!(config.flow_timeout_secs, 300);
    }

    #[test]
    fn test_callback_url() {
        let config = OpenRouterAuthConfig {
            callback_port: 9000,
            ..Default::default()
        };
        assert_eq!(config.callback_url(), "http://localhost:9000/callback");
    }
}
