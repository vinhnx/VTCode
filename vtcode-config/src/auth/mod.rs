//! Authentication utilities for VT Code.
//!
//! This module provides OAuth PKCE support for OpenRouter and other providers
//! that use OAuth-based authentication flows.

pub mod openrouter_oauth;
pub mod pkce;

pub use openrouter_oauth::{
    AuthStatus, OpenRouterOAuthConfig, OpenRouterToken, clear_oauth_token, exchange_code_for_token,
    get_auth_status, get_auth_url, load_oauth_token, save_oauth_token,
};
pub use pkce::{PkceChallenge, generate_pkce_challenge};
