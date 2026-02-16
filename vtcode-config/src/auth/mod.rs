//! Authentication utilities for VT Code.
//!
//! This module provides:
//! - Generic credential storage with OS keyring and file backends
//! - OAuth PKCE support for OpenRouter and other providers
//!
//! ## Credential Storage
//!
//! Credentials are stored using OS-specific secure storage (keyring) by default,
//! with fallback to AES-256-GCM encrypted files if the keyring is unavailable.

pub mod credentials;
pub mod openrouter_oauth;
pub mod pkce;

pub use credentials::{
    AuthCredentialsStoreMode, CredentialStorage, CustomApiKeyStorage, clear_custom_api_keys,
    load_custom_api_keys, migrate_custom_api_keys_to_keyring,
};
pub use openrouter_oauth::{
    AuthStatus, OpenRouterOAuthConfig, OpenRouterToken, clear_oauth_token, exchange_code_for_token,
    get_auth_status, get_auth_url, load_oauth_token, load_oauth_token_with_mode, save_oauth_token,
    save_oauth_token_with_mode,
};
pub use pkce::{PkceChallenge, generate_pkce_challenge};
