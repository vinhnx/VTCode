use super::{
    clear_openai_login, clear_openrouter_login, cli_browser_open_approved, handle_login_command,
    resolve_openai_authorization_code, wait_for_callback_or_cancel,
};
use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};
use anyhow::anyhow;
use serial_test::serial;
use std::future;
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_auth::{
    AuthCallbackOutcome, AuthCredentialsStoreMode, OpenAIAuthConfig, OpenAIChatGptSession, OpenAIResolvedAuth,
    OpenRouterToken, clear_oauth_token_with_mode, clear_openai_chatgpt_session, clear_openai_chatgpt_session_with_mode,
    load_oauth_token_with_mode, load_openai_chatgpt_session_with_mode, resolve_openai_auth, save_oauth_token_with_mode,
    save_openai_chatgpt_session_with_mode,
};

/// Guard that isolates `CODEX_HOME` to an empty temp dir for the duration
/// of a test, preventing the Codex auth.json fallback from picking up a
/// real Codex session from the user's machine.
struct CodexHomeGuard {
    _temp: tempfile::TempDir,
    previous: Option<String>,
}

impl CodexHomeGuard {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create temp codex home");
        let previous = std::env::var("CODEX_HOME").ok();
        vtcode_commons::env_lock::set_var("CODEX_HOME", temp.path());
        Self { _temp: temp, previous }
    }
}

impl Drop for CodexHomeGuard {
    fn drop(&mut self) {
        vtcode_commons::env_lock::lock().restore_var("CODEX_HOME", self.previous.as_deref());
    }
}
use vtcode_config::VTCodeConfig;

fn config_with_storage_mode(mode: AuthCredentialsStoreMode) -> VTCodeConfig {
    let mut config = VTCodeConfig::default();
    config.agent.credential_storage_mode = mode;
    config
}

fn sample_openai_session(api_key: &str) -> OpenAIChatGptSession {
    OpenAIChatGptSession {
        openai_api_key: api_key.to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: "refresh-token".to_string(),
        account_id: Some("account-123".to_string()),
        email: Some("user@example.com".to_string()),
        plan: Some("plus".to_string()),
        obtained_at: 1,
        refreshed_at: 1,
        expires_at: Some(2),
    }
}

fn sample_openrouter_token(api_key: &str) -> OpenRouterToken {
    OpenRouterToken {
        api_key: api_key.to_string(),
        obtained_at: 1,
        expires_at: None,
        label: Some("test-token".to_string()),
    }
}

#[test]
fn cli_browser_open_approved_accepts_yes_only() {
    assert!(cli_browser_open_approved("y"));
    assert!(cli_browser_open_approved("YES"));
    assert!(!cli_browser_open_approved("n"));
    assert!(!cli_browser_open_approved(""));
}

#[tokio::test]
async fn openai_code_resolution_accepts_manual_query_input() {
    let code = resolve_openai_authorization_code(
        future::pending::<Result<AuthCallbackOutcome, anyhow::Error>>(),
        "test-state",
        Some(future::ready(Ok(Some("code=manual-code&state=test-state".to_string())))),
    )
    .await
    .expect("manual input should resolve");
    assert_eq!(code, "manual-code");
}

#[tokio::test]
async fn openai_code_resolution_falls_back_to_manual_when_callback_fails() {
    let code = resolve_openai_authorization_code(
        future::ready(Err(anyhow!("failed to bind localhost callback server"))),
        "test-state",
        Some(future::ready(Ok(Some(
            "http://localhost:1455/auth/callback?code=manual-code&state=test-state".to_string(),
        )))),
    )
    .await
    .expect("manual fallback should resolve");
    assert_eq!(code, "manual-code");
}

#[tokio::test]
async fn openai_code_resolution_rejects_manual_bare_code() {
    let error = resolve_openai_authorization_code(
        future::ready(Err(anyhow!("OAuth flow timed out after 300 seconds"))),
        "test-state",
        Some(future::ready(Ok(Some("manual-code".to_string())))),
    )
    .await
    .expect_err("bare code should fail");
    assert!(error.to_string().contains("full redirect url or query string"));
}

#[tokio::test]
async fn openai_code_resolution_uses_callback_when_manual_input_is_invalid() {
    let code = resolve_openai_authorization_code(
        future::ready(Ok(AuthCallbackOutcome::Code("callback-code".to_string()))),
        "test-state",
        Some(future::ready(Ok(Some("manual-code".to_string())))),
    )
    .await
    .expect("callback code should win after invalid manual input");
    assert_eq!(code, "callback-code");
}

#[tokio::test]
async fn callback_wait_returns_cancelled_when_ctrl_c_is_pending() {
    let ctrl_c_state = Arc::new(CtrlCState::new());
    assert!(matches!(ctrl_c_state.register_signal(), CtrlCSignal::Cancel));
    let ctrl_c_notify = Arc::new(Notify::new());

    let outcome = wait_for_callback_or_cancel(
        future::pending::<Result<AuthCallbackOutcome, anyhow::Error>>(),
        &ctrl_c_state,
        &ctrl_c_notify,
    )
    .await
    .expect("cancelled callback wait");

    assert!(matches!(outcome, AuthCallbackOutcome::Cancelled));
    assert!(!ctrl_c_state.is_cancel_requested());
}

#[tokio::test]
async fn device_code_flag_is_rejected_for_non_codex_provider() {
    let error = handle_login_command(None, "openai", true, false)
        .await
        .expect_err("non-codex device-code login should fail");

    assert!(error.to_string().contains("supported only for `vtcode login codex`"));
}

#[tokio::test]
async fn from_codex_flag_is_rejected_for_non_openai_provider() {
    let error = handle_login_command(None, "openrouter", false, true)
        .await
        .expect_err("non-openai from-codex login should fail");

    assert!(error.to_string().contains("supported only for `vtcode login openai`"));
}

#[test]
#[serial]
fn openai_logout_clears_all_sessions_and_falls_back_to_api_key() {
    let _codex_guard = CodexHomeGuard::new();
    let _ = clear_openai_chatgpt_session();
    let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File);
    let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring);

    let file_session = sample_openai_session("file-api-key");
    let keyring_session = sample_openai_session("keyring-api-key");
    if save_openai_chatgpt_session_with_mode(&file_session, AuthCredentialsStoreMode::File).is_err() {
        return;
    }

    if save_openai_chatgpt_session_with_mode(&keyring_session, AuthCredentialsStoreMode::Keyring).is_err() {
        let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File);
        return;
    }

    let config = config_with_storage_mode(AuthCredentialsStoreMode::File);
    clear_openai_login(Some(&config)).expect("clear openai login");

    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load file session")
            .is_none()
    );
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring)
            .expect("load keyring session")
            .is_none()
    );
    assert!(matches!(
        resolve_openai_auth(
            &OpenAIAuthConfig::default(),
            AuthCredentialsStoreMode::File,
            Some("env-api-key".to_string())
        )
        .expect("resolve auth after logout"),
        OpenAIResolvedAuth::ApiKey { api_key } if api_key == "env-api-key"
    ));

    let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File);
    let _ = clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring);
}

#[test]
#[serial]
fn openrouter_logout_clears_only_configured_storage_mode() {
    let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
    let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);

    let file_token = sample_openrouter_token("file-token");
    let keyring_token = sample_openrouter_token("keyring-token");
    if save_oauth_token_with_mode(&file_token, AuthCredentialsStoreMode::File).is_err() {
        return;
    }

    if save_oauth_token_with_mode(&keyring_token, AuthCredentialsStoreMode::Keyring).is_err() {
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
        return;
    }

    let Some(loaded_keyring_token) =
        load_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring).expect("load keyring token after save")
    else {
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);
        return;
    };
    if loaded_keyring_token.api_key != "keyring-token" {
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
        let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);
        return;
    }

    let config = config_with_storage_mode(AuthCredentialsStoreMode::File);
    clear_openrouter_login(Some(&config)).expect("clear openrouter login");

    assert!(
        load_oauth_token_with_mode(AuthCredentialsStoreMode::File)
            .expect("load file token")
            .is_none()
    );
    assert_eq!(
        load_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring)
            .expect("load keyring token")
            .expect("keyring token should remain")
            .api_key,
        "keyring-token"
    );

    let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::File);
    let _ = clear_oauth_token_with_mode(AuthCredentialsStoreMode::Keyring);
}
