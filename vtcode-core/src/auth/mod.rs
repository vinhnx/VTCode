//! Authentication module for VT Code.
//!
//! This module provides OAuth flows and authentication management
//! for LLM providers that support OAuth-based authentication.

#[cfg(feature = "a2a-server")]
pub mod oauth_server;

#[cfg(feature = "a2a-server")]
pub use oauth_server::{OAuthResult, run_oauth_callback_server};

// Re-export config auth types for convenience
pub use vtcode_config::auth::{
    AuthStatus, OpenRouterOAuthConfig, OpenRouterToken, PkceChallenge, clear_oauth_token,
    exchange_code_for_token, generate_pkce_challenge, get_auth_status, get_auth_url,
    load_oauth_token, save_oauth_token,
};
