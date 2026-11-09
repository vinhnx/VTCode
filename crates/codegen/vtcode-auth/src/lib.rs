#![allow(
    clippy::unreachable,
    clippy::map_err_ignore,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
#![allow(
    missing_docs,
    dead_code,
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
//! Shared authentication and OAuth flows for VT Code.

mod auth_service;
pub mod codex_auth_import;
mod config;
pub mod credentials;
pub mod mcp_oauth;
pub mod oauth_server;
pub mod openai_chatgpt_oauth;
mod openai_refresh_policy;
mod openai_session_storage;
pub mod openrouter_oauth;
mod openrouter_token_storage;
pub mod pkce;
mod storage_paths;

pub use auth_service::OpenAIAccountAuthService;
pub use codex_auth_import::{
    CodexAuthJsonRefresher, codex_auth_json_exists, codex_auth_json_path, codex_auth_json_refresher, codex_home_dir,
    try_load_codex_chatgpt_session,
};
pub use config::{AuthConfig, CopilotAuthConfig, OpenAIAuthConfig, OpenAIPreferredMethod};
pub use credentials::{
    AuthCredentialsStoreMode, CredentialIdentity, CredentialStorage, CustomApiKeyStorage, clear_custom_api_keys,
    load_custom_api_keys, migrate_custom_api_keys,
};
pub use mcp_oauth::{
    McpOAuthConfig, McpOAuthLoginCompletion, McpOAuthPreparedLogin, McpOAuthService, McpOAuthStatus, McpOAuthToken,
};
pub use oauth_server::{
    AuthCallbackOutcome, AuthCodeCallbackServer, OAuthCallbackPage, OAuthProvider, run_auth_code_callback_server,
    start_auth_code_callback_server,
};
pub use openai_chatgpt_oauth::{
    OpenAIChatGptAuthHandle, OpenAIChatGptAuthStatus, OpenAIChatGptSession, OpenAIChatGptSessionProvenance,
    OpenAIChatGptSessionRefresher, OpenAICredentialOverview, OpenAIResolvedAuth, OpenAIResolvedAuthSource,
    clear_openai_chatgpt_session, clear_openai_chatgpt_session_with_mode, exchange_openai_chatgpt_code_for_tokens,
    generate_openai_oauth_state, get_openai_chatgpt_auth_status, get_openai_chatgpt_auth_status_with_mode,
    get_openai_chatgpt_auth_url, load_openai_chatgpt_session, load_openai_chatgpt_session_with_mode,
    parse_openai_chatgpt_manual_callback_input, refresh_openai_chatgpt_session_with_mode, resolve_openai_auth,
    save_openai_chatgpt_session, save_openai_chatgpt_session_with_mode, summarize_openai_credentials,
};
pub use openrouter_oauth::{
    AuthStatus, OpenRouterOAuthConfig, OpenRouterToken, clear_oauth_token, clear_oauth_token_with_mode,
    exchange_code_for_token, get_auth_status, get_auth_status_with_mode, get_auth_url, load_oauth_token,
    load_oauth_token_with_mode, save_oauth_token, save_oauth_token_with_mode,
};
pub use pkce::{PkceChallenge, generate_pkce_challenge};

/// Retry budget for OAuth token POSTs (code exchange, refresh, API-key
/// exchange): one extra attempt when no response was received at all. Token
/// endpoints are not idempotent — a code consumed by a 5xx must fail fast —
/// so callers pair this with `send_with_retry_transport_only`, never the
/// status-retrying variant.
pub(crate) fn oauth_token_retry_policy() -> vtcode_commons::retry::RetryPolicy {
    vtcode_commons::retry::RetryPolicy::from_retries(
        1,
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(10),
        2.0,
    )
    .with_jitter(vtcode_commons::retry::RetryPolicy::DEFAULT_JITTER)
}
