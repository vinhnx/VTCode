//! Shared authentication and OAuth flows for VT Code.

mod config;
pub mod credentials;
pub mod oauth_server;
pub mod openai_chatgpt_oauth;
pub mod openrouter_oauth;
pub mod pkce;
mod storage_paths;

pub use config::{AuthConfig, CopilotAuthConfig, OpenAIAuthConfig, OpenAIPreferredMethod};
pub use credentials::{
    AuthCredentialsStoreMode, CredentialStorage, CustomApiKeyStorage, clear_custom_api_keys,
    load_custom_api_keys, migrate_custom_api_keys_to_keyring,
};
pub use oauth_server::{
    AuthCallbackOutcome, OAuthCallbackPage, OAuthProvider, run_auth_code_callback_server,
};
pub use openai_chatgpt_oauth::{
    OpenAIChatGptAuthHandle, OpenAIChatGptAuthStatus, OpenAIChatGptSession,
    OpenAICredentialOverview, OpenAIResolvedAuth, OpenAIResolvedAuthSource,
    clear_openai_chatgpt_session, clear_openai_chatgpt_session_with_mode,
    exchange_openai_chatgpt_code_for_tokens, generate_openai_oauth_state,
    get_openai_chatgpt_auth_status, get_openai_chatgpt_auth_status_with_mode,
    get_openai_chatgpt_auth_url, load_openai_chatgpt_session,
    load_openai_chatgpt_session_with_mode, parse_openai_chatgpt_manual_callback_input,
    refresh_openai_chatgpt_session_with_mode, resolve_openai_auth, save_openai_chatgpt_session,
    save_openai_chatgpt_session_with_mode, summarize_openai_credentials,
};
pub use openrouter_oauth::{
    AuthStatus, OpenRouterOAuthConfig, OpenRouterToken, clear_oauth_token,
    clear_oauth_token_with_mode, exchange_code_for_token, get_auth_status,
    get_auth_status_with_mode, get_auth_url, load_oauth_token, load_oauth_token_with_mode,
    save_oauth_token, save_oauth_token_with_mode,
};
pub use pkce::{PkceChallenge, generate_pkce_challenge};
