use super::*;
use crate::AuthCallbackOutcome;
use crate::generate_pkce_challenge;
use assert_fs::TempDir;
use serial_test::serial;
use std::path::PathBuf;
use std::sync::Arc;

struct ExternalRefresher;

#[async_trait]
impl OpenAIChatGptSessionRefresher for ExternalRefresher {
    async fn refresh_session(&self, current: &OpenAIChatGptSession) -> Result<OpenAIChatGptSession> {
        let mut refreshed = current.clone();
        refreshed.access_token = "oauth-access-refreshed".to_string();
        refreshed.refreshed_at = current.refreshed_at.saturating_add(1);
        refreshed.expires_at = Some(now_secs() + 3600);
        Ok(refreshed)
    }
}

struct TestAuthDirGuard {
    temp_dir: Option<TempDir>,
    codex_temp_dir: Option<TempDir>,
    previous: Option<PathBuf>,
    previous_codex_home: Option<String>,
}

impl TestAuthDirGuard {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("create temp auth dir");
        let previous = crate::storage_paths::auth_storage_dir_override_for_tests().expect("read auth dir override");
        crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(temp_dir.path().to_path_buf()))
            .expect("set temp auth dir override");

        // Isolate CODEX_HOME so the Codex auth.json fallback doesn't pick
        // up a real Codex session from the user's machine during tests.
        let codex_temp_dir = TempDir::new().expect("create temp codex home");
        let previous_codex_home = std::env::var("CODEX_HOME").ok();
        vtcode_commons::env_lock::set_var("CODEX_HOME", codex_temp_dir.path());

        Self {
            temp_dir: Some(temp_dir),
            codex_temp_dir: Some(codex_temp_dir),
            previous,
            previous_codex_home,
        }
    }
}

impl Drop for TestAuthDirGuard {
    fn drop(&mut self) {
        crate::storage_paths::set_auth_storage_dir_override_for_tests(self.previous.clone())
            .expect("restore auth dir override");
        if let Some(temp_dir) = self.temp_dir.take() {
            temp_dir.close().expect("remove temp auth dir");
        }
        vtcode_commons::env_lock::lock().restore_var("CODEX_HOME", self.previous_codex_home.as_deref());
        if let Some(codex_temp_dir) = self.codex_temp_dir.take() {
            codex_temp_dir.close().expect("remove temp codex home");
        }
    }
}

fn sample_session() -> OpenAIChatGptSession {
    OpenAIChatGptSession {
        openai_api_key: "api-key".to_string(),
        id_token: "aGVhZGVy.eyJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjXzEyMyIsImNoYXRncHRfcGxhbl90eXBlIjoicGx1cyJ9fQ.sig".to_string(),
        access_token: "oauth-access".to_string(),
        refresh_token: "refresh-token".to_string(),
        account_id: Some("acc_123".to_string()),
        email: Some("test@example.com".to_string()),
        plan: Some("plus".to_string()),
        obtained_at: 10,
        refreshed_at: 10,
        expires_at: Some(now_secs() + 3600),
    }
}

#[test]
fn auth_url_contains_expected_openai_parameters() {
    // RAII guard locks env and restores both vars on drop (panic-safe).
    let env = OauthEnvGuard::new();
    env.remove_client_id();
    env.remove_originator();

    let challenge = PkceChallenge {
        code_verifier: "verifier".to_string(),
        code_challenge: "challenge".to_string(),
        code_challenge_method: "S256".to_string(),
    };

    let url = get_openai_chatgpt_auth_url(&challenge, 1455, "test-state").expect("auth url");
    assert!(url.starts_with(OPENAI_AUTH_URL));
    assert!(url.contains("client_id=app_EMoamEEZ73f0CkXaXp7hrann"));
    assert!(url.contains("code_challenge=challenge"));
    assert!(url.contains("codex_cli_simplified_flow=true"));
    assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
    assert!(url.contains("state=test-state"));
}

#[test]
fn auth_url_honors_custom_client_id_env_override() {
    // RAII guard locks env and restores both vars on drop (panic-safe).
    let env = OauthEnvGuard::new();
    env.set_client_id("app_custom_override");
    env.set_originator("vtcode_custom");

    let challenge = PkceChallenge {
        code_verifier: "verifier".to_string(),
        code_challenge: "challenge".to_string(),
        code_challenge_method: "S256".to_string(),
    };
    let url = get_openai_chatgpt_auth_url(&challenge, 1455, "test-state").expect("auth url");
    assert!(url.contains("client_id=app_custom_override"), "custom client_id not used: {url}");
    assert!(url.contains("originator=vtcode_custom"), "custom originator not used: {url}");
    assert!(!url.contains("app_EMoamEEZ73f0CkXaXp7hrann"), "default client_id leaked through override: {url}");
}

/// RAII guard that locks the environment and restores both OAuth identity
/// env vars on drop — even if the test panics. This ensures parallel
/// tests don't leak env mutations to each other.
struct OauthEnvGuard {
    env: vtcode_commons::env_lock::EnvGuard,
    prev_client_id: Option<std::ffi::OsString>,
    prev_originator: Option<std::ffi::OsString>,
}

impl OauthEnvGuard {
    fn new() -> Self {
        let env = vtcode_commons::env_lock::lock();
        let prev_client_id = std::env::var_os("VTCODE_OPENAI_OAUTH_CLIENT_ID");
        let prev_originator = std::env::var_os("VTCODE_OPENAI_OAUTH_ORIGINATOR");
        Self { env, prev_client_id, prev_originator }
    }

    fn set_client_id(&self, value: &str) {
        self.env.set_var("VTCODE_OPENAI_OAUTH_CLIENT_ID", value);
    }

    fn set_originator(&self, value: &str) {
        self.env.set_var("VTCODE_OPENAI_OAUTH_ORIGINATOR", value);
    }

    fn remove_client_id(&self) {
        self.env.remove_var("VTCODE_OPENAI_OAUTH_CLIENT_ID");
    }

    fn remove_originator(&self) {
        self.env.remove_var("VTCODE_OPENAI_OAUTH_ORIGINATOR");
    }
}

impl Drop for OauthEnvGuard {
    fn drop(&mut self) {
        self.env
            .restore_var("VTCODE_OPENAI_OAUTH_CLIENT_ID", self.prev_client_id.take());
        self.env
            .restore_var("VTCODE_OPENAI_OAUTH_ORIGINATOR", self.prev_originator.take());
    }
}

#[test]
fn resolve_oauth_client_identity_both_defaults() {
    let env = OauthEnvGuard::new();
    env.remove_client_id();
    env.remove_originator();

    let identity = resolve_oauth_client_identity().expect("defaults");
    assert_eq!(identity.client_id, DEFAULT_OPENAI_CLIENT_ID);
    assert_eq!(identity.originator, DEFAULT_OPENAI_ORIGINATOR);
}

#[test]
fn resolve_oauth_client_identity_both_custom() {
    let env = OauthEnvGuard::new();
    env.set_client_id("app_custom");
    env.set_originator("my_originator");

    let identity = resolve_oauth_client_identity().expect("custom pair");
    assert_eq!(identity.client_id, "app_custom");
    assert_eq!(identity.originator, "my_originator");
}

#[test]
fn resolve_oauth_client_identity_only_client_id_is_error() {
    let env = OauthEnvGuard::new();
    env.set_client_id("app_custom");
    env.remove_originator();

    let err = resolve_oauth_client_identity().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("VTCODE_OPENAI_OAUTH_CLIENT_ID"), "error should name the set var: {msg}");
    assert!(msg.contains("VTCODE_OPENAI_OAUTH_ORIGINATOR"), "error should name the missing var: {msg}");
}

#[test]
fn resolve_oauth_client_identity_only_originator_is_error() {
    let env = OauthEnvGuard::new();
    env.remove_client_id();
    env.set_originator("my_originator");

    let err = resolve_oauth_client_identity().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("VTCODE_OPENAI_OAUTH_ORIGINATOR"), "error should name the set var: {msg}");
    assert!(msg.contains("VTCODE_OPENAI_OAUTH_CLIENT_ID"), "error should name the missing var: {msg}");
}

#[test]
fn resolve_oauth_client_identity_blank_values_treated_as_unset() {
    let env = OauthEnvGuard::new();
    env.set_client_id("   ");
    env.set_originator("  ");

    // Blank values are treated as unset → defaults used.
    let identity = resolve_oauth_client_identity().expect("defaults from blank");
    assert_eq!(identity.client_id, DEFAULT_OPENAI_CLIENT_ID);
    assert_eq!(identity.originator, DEFAULT_OPENAI_ORIGINATOR);
}

#[test]
fn parse_jwt_claims_extracts_openai_claims() {
    let claims = parse_jwt_claims(
        "aGVhZGVy.eyJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20iLCJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjXzEyMyIsImNoYXRncHRfcGxhbl90eXBlIjoicGx1cyJ9fQ.sig",
    )
    .expect("claims");
    assert_eq!(claims.email.as_deref(), Some("test@example.com"));
    assert_eq!(claims.account_id.as_deref(), Some("acc_123"));
    assert_eq!(claims.plan.as_deref(), Some("plus"));
}

#[test]
fn session_refresh_due_uses_expiry_and_age() {
    let mut session = sample_session();
    let now = now_secs();
    session.obtained_at = now;
    session.refreshed_at = now;
    session.expires_at = Some(now + 3600);
    assert!(!session.is_refresh_due());
    session.expires_at = Some(now);
    assert!(session.is_refresh_due());
}

#[tokio::test]
#[serial]
async fn external_auth_handle_refreshes_without_persisting_session() {
    let _guard = TestAuthDirGuard::new();
    let mut session = sample_session();
    session.openai_api_key.clear();
    session.expires_at = Some(now_secs().saturating_sub(1));
    let handle = OpenAIChatGptAuthHandle::new_external(session, true, Arc::new(ExternalRefresher));

    assert!(handle.using_external_tokens());
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load session")
            .is_none()
    );

    handle.force_refresh().await.expect("force refresh");

    assert_eq!(handle.current_api_key().expect("current api key"), "oauth-access-refreshed");
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load session")
            .is_none()
    );
}

struct CountingExternalRefresher {
    calls: Arc<Mutex<usize>>,
}

#[async_trait]
impl OpenAIChatGptSessionRefresher for CountingExternalRefresher {
    async fn refresh_session(&self, current: &OpenAIChatGptSession) -> Result<OpenAIChatGptSession> {
        let mut calls = self.calls.lock().expect("refresh calls mutex should lock");
        *calls += 1;
        drop(calls);

        let mut refreshed = current.clone();
        refreshed.access_token = "oauth-access-refreshed".to_string();
        refreshed.refreshed_at = now_secs();
        refreshed.expires_at = Some(now_secs() + 3600);
        Ok(refreshed)
    }
}

#[tokio::test]
async fn refresh_if_needed_serializes_external_refreshes() {
    let mut session = sample_session();
    session.openai_api_key.clear();
    session.expires_at = Some(now_secs().saturating_sub(1));
    let calls = Arc::new(Mutex::new(0usize));
    let handle = OpenAIChatGptAuthHandle::new_external(
        session,
        true,
        Arc::new(CountingExternalRefresher { calls: Arc::clone(&calls) }),
    );

    let first = handle.clone();
    let second = handle.clone();
    let (first_result, second_result) = tokio::join!(first.refresh_if_needed(), second.refresh_if_needed());

    first_result.expect("first refresh should succeed");
    second_result.expect("second refresh should succeed");
    assert_eq!(
        *calls.lock().expect("refresh calls mutex should lock"),
        1,
        "concurrent refresh_if_needed calls should share one refresh"
    );
    assert_eq!(handle.current_api_key().expect("current api key"), "oauth-access-refreshed");
}

#[test]
#[serial]
fn resolve_openai_auth_prefers_chatgpt_in_auto_permission() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save session");
    let resolved =
        resolve_openai_auth(&OpenAIAuthConfig::default(), AuthCredentialsStoreMode::File, Some("api-key".to_string()))
            .expect("resolved auth");
    assert!(resolved.using_chatgpt());
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
}

#[test]
#[serial]
#[cfg(unix)]
fn file_storage_uses_private_permissions() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let _guard = TestAuthDirGuard::new();
    let session = sample_session();

    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save session");

    let metadata = fs::metadata(OpenAiSessionStorage::new().current_file_path().expect("session path"))
        .expect("read session metadata");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

#[test]
#[serial]
fn legacy_file_session_migrates_to_shared_storage() {
    use std::fs;

    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    let encrypted = encrypt_session(&session).expect("encrypt legacy session");
    let legacy_path = get_session_path().expect("legacy session path");
    fs::write(&legacy_path, serde_json::to_vec(&encrypted).expect("serialize legacy session"))
        .expect("write legacy session");

    let loaded = load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
        .expect("load migrated session")
        .expect("session should be present");

    assert_eq!(loaded.account_id, session.account_id);
    assert!(legacy_path.exists(), "legacy session should remain as a rollback source after migration");
    assert!(
        OpenAiSessionStorage::new()
            .current_file_path()
            .expect("shared session path")
            .exists()
    );
}

#[test]
#[serial]
fn resolve_openai_auth_auto_falls_back_to_api_key_without_session() {
    let _guard = TestAuthDirGuard::new();
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
    let resolved =
        resolve_openai_auth(&OpenAIAuthConfig::default(), AuthCredentialsStoreMode::File, Some("api-key".to_string()))
            .expect("resolved auth");
    assert!(matches!(resolved, OpenAIResolvedAuth::ApiKey { .. }));
}

#[test]
#[serial]
fn resolve_openai_auth_auto_rejects_blank_api_key_without_session() {
    let _guard = TestAuthDirGuard::new();
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
    let error =
        resolve_openai_auth(&OpenAIAuthConfig::default(), AuthCredentialsStoreMode::File, Some("   ".to_string()))
            .expect_err("blank api key should fail");
    assert!(error.to_string().contains("OpenAI API key not found"));
}

#[test]
#[serial]
fn resolve_openai_auth_api_key_mode_ignores_stored_chatgpt_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save session");
    let resolved = resolve_openai_auth(
        &OpenAIAuthConfig {
            preferred_method: OpenAIPreferredMethod::ApiKey,
            ..OpenAIAuthConfig::default()
        },
        AuthCredentialsStoreMode::File,
        Some("api-key".to_string()),
    )
    .expect("resolved auth");
    assert!(matches!(resolved, OpenAIResolvedAuth::ApiKey { .. }));
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
}

#[test]
#[serial]
fn resolve_openai_auth_chatgpt_mode_requires_stored_session() {
    let _guard = TestAuthDirGuard::new();
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
    let error = resolve_openai_auth(
        &OpenAIAuthConfig {
            preferred_method: OpenAIPreferredMethod::Chatgpt,
            ..OpenAIAuthConfig::default()
        },
        AuthCredentialsStoreMode::File,
        Some("api-key".to_string()),
    )
    .expect_err("chatgpt mode should require a stored session");
    assert!(error.to_string().contains("vtcode login openai"));
}

#[test]
#[serial]
fn summarize_openai_credentials_reports_dual_source_notice() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save session");
    let overview = summarize_openai_credentials(
        &OpenAIAuthConfig::default(),
        AuthCredentialsStoreMode::File,
        Some("api-key".to_string()),
    )
    .expect("overview");
    assert_eq!(overview.active_source, Some(OpenAIResolvedAuthSource::ChatGpt));
    assert!(overview.notice.is_some());
    assert!(overview.recommendation.is_some());
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
}

#[test]
#[serial]
fn summarize_openai_credentials_respects_api_key_preference() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save session");
    let overview = summarize_openai_credentials(
        &OpenAIAuthConfig {
            preferred_method: OpenAIPreferredMethod::ApiKey,
            ..OpenAIAuthConfig::default()
        },
        AuthCredentialsStoreMode::File,
        Some("api-key".to_string()),
    )
    .expect("overview");
    assert_eq!(overview.active_source, Some(OpenAIResolvedAuthSource::ApiKey));
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
}

#[test]
fn encrypted_file_round_trip_restores_session() {
    let session = sample_session();
    let encrypted = encrypt_session(&session).expect("encrypt");
    let decrypted = decrypt_session(&encrypted).expect("decrypt");
    assert_eq!(decrypted.account_id, session.account_id);
    assert_eq!(decrypted.email, session.email);
    assert_eq!(decrypted.plan, session.plan);
}

#[test]
#[serial]
fn default_loader_falls_back_to_file_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save session");

    let loaded = load_openai_chatgpt_session()
        .expect("load session")
        .expect("stored session should be found");

    assert_eq!(loaded.account_id, session.account_id);
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
}

#[test]
#[serial]
fn keyring_mode_loader_falls_back_to_file_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save session");

    let loaded = load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::Keyring)
        .expect("load session")
        .expect("stored session should be found");

    assert_eq!(loaded.email, session.email);
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear session");
}

#[test]
#[serial]
fn clear_openai_chatgpt_session_removes_file_and_keyring_sessions() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save file session");

    if save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::Keyring).is_err() {
        clear_openai_chatgpt_session().expect("clear session");
        assert!(
            load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
                .expect("load file session")
                .is_none()
        );
        return;
    }

    clear_openai_chatgpt_session().expect("clear session");
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
}

#[test]
fn active_api_bearer_token_falls_back_to_access_token() {
    let mut session = sample_session();
    session.openai_api_key.clear();

    assert_eq!(active_api_bearer_token(&session), "oauth-access");
}

#[test]
fn parse_manual_callback_input_accepts_full_redirect_url() {
    let code = parse_openai_chatgpt_manual_callback_input(
        "http://localhost:1455/auth/callback?code=auth-code&state=test-state",
        "test-state",
    )
    .expect("manual input should parse");
    assert_eq!(code, "auth-code");
}

#[test]
fn parse_manual_callback_input_accepts_query_string() {
    let code = parse_openai_chatgpt_manual_callback_input("code=auth-code&state=test-state", "test-state")
        .expect("manual input should parse");
    assert_eq!(code, "auth-code");
}

#[test]
fn parse_manual_callback_input_rejects_bare_code() {
    let error = parse_openai_chatgpt_manual_callback_input("auth-code", "test-state")
        .expect_err("bare code should be rejected");
    assert!(error.to_string().contains("full redirect url or query string"));
}

#[test]
fn parse_manual_callback_input_rejects_state_mismatch() {
    let error = parse_openai_chatgpt_manual_callback_input("code=auth-code&state=wrong-state", "test-state")
        .expect_err("state mismatch should fail");
    assert!(error.to_string().contains("state mismatch"));
}

#[tokio::test]
#[serial]
async fn refresh_lock_serializes_parallel_acquisition() {
    let _guard = TestAuthDirGuard::new();
    let first = tokio::spawn(async {
        let _lock = acquire_refresh_lock().await.expect("first lock");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let start = std::time::Instant::now();
    let second = tokio::spawn(async {
        let _lock = acquire_refresh_lock().await.expect("second lock");
    });

    first.await.expect("first task");
    second.await.expect("second task");
    assert!(start.elapsed() >= std::time::Duration::from_millis(100));
}

// ── Debug redaction tests ──

#[test]
fn debug_impl_redacts_all_token_fields() {
    let session = sample_session();
    let debug_str = format!("{session:?}");
    // None of the secret values may appear in the Debug output.
    assert!(!debug_str.contains("api-key"), "openai_api_key leaked: {debug_str}");
    assert!(!debug_str.contains("oauth-access"), "access_token leaked: {debug_str}");
    assert!(!debug_str.contains("refresh-token"), "refresh_token leaked: {debug_str}");
    // The id_token JWT body is long; check a distinctive substring.
    assert!(!debug_str.contains("eyJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20i"), "id_token leaked: {debug_str}");
    // Non-secret metadata should still be present.
    assert!(debug_str.contains("test@example.com"), "email should be visible: {debug_str}");
    assert!(debug_str.contains("plus"), "plan should be visible: {debug_str}");
}

#[test]
fn debug_impl_redacts_resolved_auth_api_key() {
    let resolved = OpenAIResolvedAuth::ApiKey { api_key: "sk-secret-key".to_string() };
    let debug_str = format!("{resolved:?}");
    assert!(!debug_str.contains("sk-secret-key"), "api_key leaked: {debug_str}");
}

#[test]
fn debug_impl_redacts_resolved_auth_chatgpt_handle() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    let handle = OpenAIChatGptAuthHandle::new(session, OpenAIAuthConfig::default(), AuthCredentialsStoreMode::File);
    let resolved = OpenAIResolvedAuth::ChatGpt { api_key: "sk-secret-bearer".to_string(), handle };
    let debug_str = format!("{resolved:?}");
    assert!(!debug_str.contains("sk-secret-bearer"), "bearer leaked: {debug_str}");
}

#[test]
fn credential_overview_carries_no_token_fields() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");
    let overview = summarize_openai_credentials(
        &OpenAIAuthConfig::default(),
        AuthCredentialsStoreMode::File,
        Some("sk-overview-key".to_string()),
    )
    .expect("overview");
    // The overview is a display struct — verify it only carries metadata,
    // not the raw session or token strings.
    let debug_str = format!("{overview:?}");
    assert!(!debug_str.contains("oauth-access"), "access_token leaked: {debug_str}");
    assert!(!debug_str.contains("refresh-token"), "refresh_token leaked: {debug_str}");
    assert!(!debug_str.contains("api-key"), "openai_api_key leaked: {debug_str}");
    // The overview should not contain the raw API key value either.
    assert!(!debug_str.contains("sk-overview-key"), "api key value leaked: {debug_str}");
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear");
}

// ── Partial refresh response merge tests ──
//
// These test merge_refresh_response directly with resp.id_token = None,
// which skips the HTTP API-key exchange (has_new_id_token = false).
// This lets us verify field-preservation behavior without network access.

#[tokio::test]
async fn merge_preserves_omitted_access_token() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: Some("new-refresh".to_string()),
        expires_in: Some(3600),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, current.access_token, "omitted access_token should be preserved");
    assert_eq!(merged.refresh_token, "new-refresh");
}

#[tokio::test]
async fn merge_preserves_omitted_refresh_token() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("new-access".to_string()),
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.refresh_token, current.refresh_token, "omitted refresh_token should be preserved");
    assert_eq!(merged.access_token, "new-access");
}

#[tokio::test]
async fn merge_preserves_omitted_id_token_and_api_key() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("new-access".to_string()),
        refresh_token: None,
        expires_in: Some(1800),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // No new id_token → old id_token and api_key preserved, no HTTP exchange.
    assert_eq!(merged.id_token, current.id_token, "omitted id_token should be preserved");
    assert_eq!(merged.openai_api_key, current.openai_api_key, "api_key should be preserved without new id_token");
    // Email/plan/account_id also preserved without a new id_token.
    assert_eq!(merged.email, current.email);
    assert_eq!(merged.plan, current.plan);
}

#[tokio::test]
async fn merge_all_omitted_preserves_everything() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.id_token, current.id_token);
    assert_eq!(merged.access_token, current.access_token);
    assert_eq!(merged.refresh_token, current.refresh_token);
    assert_eq!(merged.openai_api_key, current.openai_api_key);
    assert_eq!(merged.email, current.email);
    // expires_in is None and no new access_token → old expiry preserved.
    assert_eq!(merged.expires_at, current.expires_at);
}

#[tokio::test]
async fn merge_new_access_token_updates_bearer_without_id_token() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("replaced-access".to_string()),
        refresh_token: None,
        expires_in: Some(7200),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, "replaced-access");
    // active_api_bearer_token should use the exchanged api_key (unchanged)
    // because no new id_token was provided.
    assert_eq!(active_api_bearer_token(&merged), current.openai_api_key);
}

// ── Blank-string refresh field tests ──
//
// Some token endpoints return empty strings for omitted fields rather than
// leaving them out. These verify that blank strings are treated as omitted.

#[tokio::test]
async fn merge_treats_blank_access_token_as_omitted() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("   ".to_string()),
        refresh_token: Some("new-refresh".to_string()),
        expires_in: Some(3600),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, current.access_token, "blank access_token should be treated as omitted");
    assert_eq!(merged.refresh_token, "new-refresh");
}

#[tokio::test]
async fn merge_treats_blank_refresh_token_as_omitted() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("new-access".to_string()),
        refresh_token: Some(String::new()),
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.refresh_token, current.refresh_token, "blank refresh_token should be treated as omitted");
    assert_eq!(merged.access_token, "new-access");
}

#[tokio::test]
async fn merge_treats_blank_id_token_as_omitted() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: Some("  ".to_string()),
        access_token: Some("new-access".to_string()),
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // Blank id_token → treated as omitted → no HTTP exchange, old preserved.
    assert_eq!(merged.id_token, current.id_token, "blank id_token should be treated as omitted");
    assert_eq!(merged.openai_api_key, current.openai_api_key, "api_key preserved when id_token is blank");
}

// ── Opaque access token expiry tests ──

#[tokio::test]
async fn merge_changed_opaque_access_token_clears_stale_expiry() {
    let mut current = sample_session();
    current.expires_at = Some(now_secs() + 3600); // old expiry for old token

    // New access token without expires_in and without a parseable JWT exp.
    // "opaque-new-token" is not a JWT, so parse_jwt_exp returns None.
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("opaque-new-token".to_string()),
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, "opaque-new-token");
    // Changed token without expires_in or JWT exp → expiry must be None,
    // NOT the old token's expiry.
    assert_eq!(merged.expires_at, None, "changed opaque access token must not inherit old token's expiry");
}

#[tokio::test]
async fn merge_repeated_opaque_access_token_preserves_expiry() {
    let old_expiry = now_secs() + 3600;
    let mut current = sample_session();
    current.access_token = "opaque-same-token".to_string();
    current.expires_at = Some(old_expiry);

    // The endpoint repeats the SAME opaque access token without expires_in.
    // Since the token didn't change, the old expiry is still valid.
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("opaque-same-token".to_string()),
        refresh_token: Some("new-refresh".to_string()),
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, "opaque-same-token");
    assert_eq!(merged.expires_at, Some(old_expiry), "repeated same opaque access token must preserve old expiry");
}

#[tokio::test]
async fn merge_omitted_access_token_preserves_old_expiry() {
    let old_expiry = now_secs() + 3600;
    let mut current = sample_session();
    current.expires_at = Some(old_expiry);

    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: Some("new-refresh".to_string()),
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // No new access_token and no expires_in → old expiry preserved.
    assert_eq!(merged.expires_at, Some(old_expiry), "omitted access_token should preserve old expiry");
}

#[tokio::test]
async fn merge_expires_in_overrides_for_omitted_access_token() {
    let old_expiry = now_secs() + 3600;
    let mut current = sample_session();
    current.expires_at = Some(old_expiry);

    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: None,
        expires_in: Some(1800),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // expires_in takes priority over old expiry even without a new access_token.
    assert_ne!(merged.expires_at, Some(old_expiry));
    assert!(merged.expires_at.is_some());
}

// ── Error classification tests (pure, no network) ──

#[test]
fn extract_error_code_flat_form() {
    assert_eq!(extract_error_code(r#"{"error": "invalid_grant"}"#), "invalid_grant");
}

#[test]
fn extract_error_code_nested_form() {
    let body = r#"{"error": {"code": "refresh_token_expired", "message": "token expired"}}"#;
    assert_eq!(extract_error_code(body), "refresh_token_expired");
}

#[test]
fn extract_error_code_nested_form_does_not_fall_back_to_message() {
    // A descriptive message that happens to contain a terminal code string
    // must NOT be treated as a structured error code. Only `code` and
    // `type` fields are recognized — `message` is free-text.
    let body = r#"{"error": {"message": "invalid_grant"}}"#;
    assert_eq!(extract_error_code(body), "", "message field should not be used as error code: {body}");
}

#[test]
fn extract_error_code_empty_body() {
    assert_eq!(extract_error_code(""), "");
}

#[test]
fn extract_error_code_non_json_body() {
    assert_eq!(extract_error_code("Internal Server Error"), "");
}

#[test]
fn extract_error_code_blank_error_field() {
    assert_eq!(extract_error_code(r#"{"error": "  "}"#), "");
}

#[test]
fn classify_refresh_status_error_invalid_grant_clears_session() {
    let _guard = TestAuthDirGuard::new();
    // Store a session so we can verify it gets cleared.
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some()
    );

    let err = classify_refresh_status_error(reqwest::StatusCode::BAD_REQUEST, r#"{"error": "invalid_grant"}"#);
    assert!(err.to_string().contains("session expired"));

    // Session should have been cleared.
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_none()
    );
}

#[test]
fn classify_refresh_status_error_nested_refresh_token_expired_clears_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"code": "refresh_token_expired", "message": "The refresh token has expired"}}"#,
    );
    assert!(err.to_string().contains("session expired"));

    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_none()
    );
}

#[test]
fn classify_refresh_status_error_unauthorized_preserves_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err = classify_refresh_status_error(reqwest::StatusCode::UNAUTHORIZED, "");
    assert!(err.to_string().contains("HTTP 401"), "should report HTTP 401: {err}");
    // 401 without a confirmed terminal code preserves the session.
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some(),
        "session should be preserved on ambiguous 401"
    );
}

#[test]
fn classify_refresh_status_error_server_error_does_not_clear_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err =
        classify_refresh_status_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, r#"{"error": "internal_error"}"#);
    assert!(err.to_string().contains("HTTP 500"));
    // Transient error → session preserved for retry.
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some()
    );
}

#[test]
fn classify_refresh_status_error_never_includes_raw_body() {
    let _guard = TestAuthDirGuard::new();
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_grant", "sensitive_data": "secret-leak-attempt"}"#,
    );
    assert!(!err.to_string().contains("secret-leak-attempt"), "raw body leaked into error: {err}");
}

// ── Table-driven classification matrix (status × body shape × expected) ──

/// Expected outcome of `classify_refresh_status_error`.
#[derive(Debug, PartialEq)]
enum ClassifyOutcome {
    /// Session was cleared (terminal grant error).
    Terminal,
    /// Session was preserved; error message contains this fragment.
    Preserved(&'static str),
}

fn run_classify_matrix(status: reqwest::StatusCode, body: &str, expected: ClassifyOutcome) {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err = classify_refresh_status_error(status, body);
    let msg = err.to_string();
    let loaded = load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("load");

    match expected {
        ClassifyOutcome::Terminal => {
            assert!(msg.contains("session expired"), "terminal error should say 'session expired': {msg}");
            assert!(loaded.is_none(), "session should be cleared for terminal: {status} {body}");
        }
        ClassifyOutcome::Preserved(frag) => {
            assert!(!msg.contains("session expired"), "non-terminal should not say 'session expired': {msg}");
            assert!(loaded.is_some(), "session should be preserved for: {status} {body}");
            if !frag.is_empty() {
                assert!(msg.contains(frag), "error should contain '{frag}': {msg}");
            }
        }
    }
}

#[test]
fn classify_matrix_terminal_400_flat() {
    run_classify_matrix(reqwest::StatusCode::BAD_REQUEST, r#"{"error": "invalid_grant"}"#, ClassifyOutcome::Terminal);
}

#[test]
fn classify_matrix_terminal_400_nested_code() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"code": "invalid_grant", "message": "..."}}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_400_nested_type() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"type": "invalid_grant", "message": "..."}}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_400_toplevel_code() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"code": "refresh_token_revoked", "message": "..."}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_401_flat() {
    // Terminal codes on 401 should also clear the session.
    run_classify_matrix(reqwest::StatusCode::UNAUTHORIZED, r#"{"error": "invalid_token"}"#, ClassifyOutcome::Terminal);
}

#[test]
fn classify_matrix_terminal_401_nested() {
    run_classify_matrix(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error": {"code": "refresh_token_expired"}}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_401_refresh_token_invalidated() {
    run_classify_matrix(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error": "refresh_token_invalidated"}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_400_toplevel_refresh_token_reused() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"code": "refresh_token_reused", "message": "..."}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_message_only_does_not_clear_session() {
    // A body with only a free-text "message" field (no structured code/type)
    // must NOT be treated as terminal, even if the message text happens to
    // contain a terminal code string. This prevents false-positive session
    // clearing from descriptive error messages.
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"message": "invalid_grant"}}"#,
        ClassifyOutcome::Preserved("HTTP 400"),
    );
}

#[test]
fn classify_matrix_invalid_client_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_client"}"#,
        ClassifyOutcome::Preserved("invalid_client"),
    );
}

#[test]
fn classify_matrix_invalid_client_401_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error": "invalid_client"}"#,
        ClassifyOutcome::Preserved("invalid_client"),
    );
}

#[test]
fn classify_matrix_429_throttling_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        r#"{"error": "rate_limited"}"#,
        ClassifyOutcome::Preserved("rate-limited"),
    );
}

#[test]
fn classify_matrix_500_server_error_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        r#"{"error": "internal_error"}"#,
        ClassifyOutcome::Preserved("HTTP 500"),
    );
}

#[test]
fn classify_matrix_502_bad_gateway_preserves() {
    run_classify_matrix(reqwest::StatusCode::BAD_GATEWAY, "", ClassifyOutcome::Preserved("HTTP 502"));
}

#[test]
fn classify_matrix_503_service_unavailable_preserves() {
    run_classify_matrix(reqwest::StatusCode::SERVICE_UNAVAILABLE, "", ClassifyOutcome::Preserved("HTTP 503"));
}

#[test]
fn classify_matrix_ambiguous_401_empty_body_preserves() {
    run_classify_matrix(reqwest::StatusCode::UNAUTHORIZED, "", ClassifyOutcome::Preserved("HTTP 401"));
}

#[test]
fn classify_matrix_400_nonterminal_code_preserves() {
    // A 400 with a code that is NOT in the terminal list should preserve.
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "some_unknown_error"}"#,
        ClassifyOutcome::Preserved("HTTP 400"),
    );
}

#[test]
fn classify_matrix_never_leaks_body_in_any_branch() {
    // Terminal branch
    let _guard = TestAuthDirGuard::new();
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_grant", "leak": "TERMINAL_LEAK"}"#,
    );
    assert!(!err.to_string().contains("TERMINAL_LEAK"));

    // Preserved branch (invalid_client)
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_client", "leak": "CLIENT_LEAK"}"#,
    );
    assert!(!err.to_string().contains("CLIENT_LEAK"));

    // Preserved branch (429)
    let err = classify_refresh_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        r#"{"error": "rate_limited", "leak": "THROTTLE_LEAK"}"#,
    );
    assert!(!err.to_string().contains("THROTTLE_LEAK"));
}

// ── Error code extraction shape tests ──

#[test]
fn extract_error_code_nested_with_type_field() {
    let body = r#"{"error": {"type": "invalid_grant", "message": "..."}}"#;
    assert_eq!(extract_error_code(body), "invalid_grant");
}

#[test]
fn extract_error_code_toplevel_code_field() {
    let body = r#"{"code": "refresh_token_expired", "message": "..."}"#;
    assert_eq!(extract_error_code(body), "refresh_token_expired");
}

#[test]
fn extract_error_code_case_insensitive_normalization() {
    assert_eq!(extract_error_code(r#"{"error": "INVALID_GRANT"}"#), "invalid_grant");
}

#[test]
fn extract_error_code_no_substring_matching() {
    // "invalid_grant_really" is not a terminal code — extract_error_code
    // returns it verbatim, but classify won't match it as terminal.
    let code = extract_error_code(r#"{"error": "invalid_grant_really_not_a_real_code"}"#);
    assert_eq!(code, "invalid_grant_really_not_a_real_code");
    // Verify classify does NOT treat this as terminal.
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_grant_really_not_a_real_code"}"#,
    );
    assert!(!err.to_string().contains("session expired"));
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some()
    );
}

// ── PKCE and callback Debug redaction tests ──

#[test]
fn pkce_challenge_debug_redacts_verifier() {
    let challenge = generate_pkce_challenge().expect("generate pkce");
    let debug_str = format!("{challenge:?}");
    assert!(!debug_str.contains(&challenge.code_verifier), "code_verifier leaked: {debug_str}");
    assert!(debug_str.contains("<redacted>"), "verifier should be redacted: {debug_str}");
    // Challenge and method are safe to display.
    assert!(debug_str.contains(&challenge.code_challenge), "code_challenge should be visible: {debug_str}");
}

#[test]
fn auth_callback_outcome_debug_redacts_code() {
    let outcome = AuthCallbackOutcome::Code("super-secret-auth-code".to_string());
    let debug_str = format!("{outcome:?}");
    assert!(!debug_str.contains("super-secret-auth-code"), "authorization code leaked: {debug_str}");
    assert!(debug_str.contains("<redacted>"), "code should be redacted: {debug_str}");
}

#[test]
fn auth_callback_outcome_debug_shows_cancelled_and_redacts_error() {
    let cancelled = format!("{:?}", AuthCallbackOutcome::Cancelled);
    assert!(cancelled.contains("Cancelled"));

    // Error messages from OAuth callbacks are untrusted query parameters
    // that may contain sensitive values — Debug must redact them.
    let error = format!("{:?}", AuthCallbackOutcome::Error("access_denied".to_string()));
    assert!(!error.contains("access_denied"), "error message leaked through Debug: {error}");
    assert!(error.contains("<redacted>"), "error should be redacted: {error}");
}
