use super::*;

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
fn resolve_openai_auth_api_key_mode_survives_unreadable_session_store() {
    let temp = TempDir::new().expect("temp dir");
    let blocker = temp.path().join("blocker");
    fs::write(&blocker, b"not a directory").expect("write blocker");
    let previous = crate::storage_paths::auth_storage_dir_override_for_tests().expect("read auth dir override");
    crate::storage_paths::set_auth_storage_dir_override_for_tests(Some(blocker.join("sub")))
        .expect("set unreadable auth dir override");
    let resolved = resolve_openai_auth(
        &OpenAIAuthConfig {
            preferred_method: OpenAIPreferredMethod::ApiKey,
            ..OpenAIAuthConfig::default()
        },
        AuthCredentialsStoreMode::File,
        Some("api-key".to_string()),
    );
    crate::storage_paths::set_auth_storage_dir_override_for_tests(previous).expect("restore auth dir override");
    assert!(matches!(
        resolved.expect("apikey must not touch the session store"),
        OpenAIResolvedAuth::ApiKey { .. }
    ));
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
