use super::*;
use serial_test::serial;
use vtcode_config::auth::AuthCredentialsStoreMode;

/// Hermetic secure-storage scope: File backend rooted at a temp
/// `VTCODE_CONFIG` with provider env vars removed, so resolution observes
/// only keys stored during the test. Restores the environment on drop.
struct HermeticSecretStorage {
    _temp: tempfile::TempDir,
    _env: vtcode_commons::env_lock::EnvGuard,
    previous_config: Option<std::ffi::OsString>,
    removed: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl HermeticSecretStorage {
    fn new(env_vars: &[&'static str]) -> Self {
        let temp = tempfile::tempdir().expect("auth config dir");
        let env = vtcode_commons::env_lock::lock();
        let previous_config = std::env::var_os("VTCODE_CONFIG");
        env.set_var("VTCODE_CONFIG", temp.path());
        let mut removed = Vec::with_capacity(env_vars.len());
        for var in env_vars {
            removed.push((*var, std::env::var_os(var)));
            env.remove_var(var);
        }
        Self { _temp: temp, _env: env, previous_config, removed }
    }

    fn path(&self) -> &Path {
        self._temp.path()
    }
}

impl Drop for HermeticSecretStorage {
    fn drop(&mut self) {
        self._env.restore_var("VTCODE_CONFIG", self.previous_config.clone());
        for (var, previous) in &self.removed {
            self._env.restore_var(var, previous.clone());
        }
    }
}

fn secret_store_request(provider: &str, api_key: &str) -> Request {
    Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("secret-store-test".into()),
        method: protocol::method::CONFIG_SECRET_STORE.into(),
        params: serde_json::to_value(protocol::SecretStoreParams {
            provider: provider.to_string(),
            api_key: api_key.to_string(),
        })
        .expect("secret params"),
    }
}

fn file_mode_context(workspace: &Path) -> StartupContext {
    let mut startup = test_startup_context(workspace);
    startup.config.agent.credential_storage_mode = AuthCredentialsStoreMode::File;
    startup
}

fn restore_dir_writable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("restore config dir");
    }
    #[cfg(not(unix))]
    {
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(path, permissions).expect("restore config dir");
    }
}

#[tokio::test]
async fn secret_store_rejects_invalid_requests_without_touching_storage() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let sentinel = "sk-test-sentinel-key-that-must-never-echo-001";
    for (provider, key, expected) in [
        ("", sentinel, "Provider must not be empty."),
        ("  ", sentinel, "Provider must not be empty."),
        ("gemini", "", "API key must not be empty."),
        ("gemini", "   ", "API key must not be empty."),
        ("ollama", sentinel, "does not use an API key"),
        ("copilot", sentinel, "vtcode login copilot"),
        ("vtcode-test-unknown-provider", sentinel, "Unknown provider"),
    ] {
        let error = handle_request(&context, &secret_store_request(provider, key))
            .await
            .expect_err("invalid secret store must fail");
        let message = format!("{error:#}");
        assert!(message.contains(expected), "provider {provider:?}: {message}");
        assert!(!message.contains(sentinel), "key leaked into error: {message}");
    }
    // Length bounds reject just past the limit and accept exactly at it (the
    // overlong provider and key below fail validation; the at-limit pair
    // reaches provider resolution instead).
    let overlong_provider = "p".repeat(129);
    let error = handle_request(&context, &secret_store_request(&overlong_provider, sentinel))
        .await
        .expect_err("overlong provider must fail");
    assert!(format!("{error:#}").contains("too long"));
    let overlong_key = "k".repeat(8 * 1024 + 1);
    let error = handle_request(&context, &secret_store_request("gemini", &overlong_key))
        .await
        .expect_err("overlong key must fail");
    assert!(format!("{error:#}").contains("8 KiB"));
    let at_limit_provider = "p".repeat(128);
    let error = handle_request(&context, &secret_store_request(&at_limit_provider, sentinel))
        .await
        .expect_err("unknown provider must fail");
    assert!(format!("{error:#}").contains("Unknown provider"));
    let at_limit_key = "k".repeat(8 * 1024);
    let error = handle_request(&context, &secret_store_request("vtcode-test-unknown-provider", &at_limit_key))
        .await
        .expect_err("unknown provider must fail");
    assert!(format!("{error:#}").contains("Unknown provider"));

    let state = context.state.lock().await;
    assert!(state.secret_store_auth.is_none(), "rejected saves must not record auth");
    assert!(state.model_override_auth.is_none());
}

#[tokio::test]
#[serial]
async fn secret_store_repairs_boot_provider_without_restart() {
    // Gemini also resolves GOOGLE_API_KEY; remove it so only the stored key counts.
    let _storage = HermeticSecretStorage::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
    let sentinel = "sk-test-hermetic-boot-repair-key-002";
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let mut startup = file_mode_context(&workspace);
    startup.provider_auth_error = Some("API key not found for provider 'gemini'.".to_string());
    startup.agent_config.api_key.clear();
    // A stale workspace `.env` entry must be purged so it cannot shadow storage.
    std::fs::write(workspace.join(".env"), "GEMINI_API_KEY=stale-workspace-value\n").expect("seed .env");
    let (context, _) = test_service_context(startup, workspace.clone());
    context.state.lock().await.initialized = true;

    let (result, shutdown) = handle_request(&context, &secret_store_request("  Gemini  ", sentinel))
        .await
        .expect("secret store");
    assert!(!shutdown);
    let stored: protocol::SecretStoreResult = serde_json::from_value(result).expect("result");
    assert!(stored.session_reset);
    assert!(stored.configuration.authenticated);
    assert_eq!(stored.configuration.provider, "gemini");
    assert_eq!(
        stored.configuration.secret_store,
        Some(protocol::SecretStoreInfo {
            provider: "gemini".into(),
            env_var: "GEMINI_API_KEY".into()
        })
    );
    assert!(
        !serde_json::to_string(&stored).expect("render").contains(sentinel),
        "response must not echo the key"
    );

    let resolved = vtcode_config::api_keys::resolve_credential_with_mode(
        "gemini",
        "GEMINI_API_KEY",
        Some(&workspace),
        AuthCredentialsStoreMode::File,
    )
    .expect("resolve")
    .and_then(|credential| credential.secret);
    assert_eq!(resolved.as_deref(), Some(sentinel));
    assert!(
        vtcode_config::workspace_env::read_workspace_env_value(&workspace, "GEMINI_API_KEY")
            .expect("read env")
            .is_none(),
        "stale .env entry must be purged"
    );
    assert_eq!(active_agent_config(&context).await.api_key, sentinel);
}

#[tokio::test]
#[serial]
async fn secret_store_storage_failure_leaves_state_untouched() {
    let storage = HermeticSecretStorage::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
    // A read-only config dir makes secure storage fail closed.
    let mut permissions = std::fs::metadata(storage.path()).expect("metadata").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(storage.path(), permissions).expect("read-only config dir");
    let sentinel = "sk-test-hermetic-storage-failure-key-006";
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let mut startup = file_mode_context(&workspace);
    startup.provider_auth_error = Some("API key not found for provider 'gemini'.".to_string());
    startup.agent_config.api_key.clear();
    let (context, _) = test_service_context(startup, workspace);
    context.state.lock().await.initialized = true;

    let error = handle_request(&context, &secret_store_request("gemini", sentinel))
        .await
        .expect_err("storage failure must fail the save");
    let message = format!("{error:#}");
    assert!(message.contains("Failed to store"), "{message}");
    assert!(!message.contains(sentinel), "key leaked into error: {message}");
    {
        let state = context.state.lock().await;
        assert!(state.secret_store_auth.is_none(), "failed saves must not record auth");
    }
    assert_eq!(active_agent_config(&context).await.api_key, "");

    restore_dir_writable(storage.path());
}

#[tokio::test]
#[serial]
async fn secret_store_rotation_preserves_session_and_auth() {
    // Gemini also resolves GOOGLE_API_KEY; remove it so only the stored key counts.
    let _storage = HermeticSecretStorage::new(&["GEMINI_API_KEY", "GOOGLE_API_KEY"]);
    let sentinel = "sk-test-hermetic-rotation-key-003";
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(file_mode_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    context.state.lock().await.session_id = Some("live-session".to_string());

    let (result, _) = handle_request(&context, &secret_store_request("gemini", sentinel))
        .await
        .expect("secret store");
    let stored: protocol::SecretStoreResult = serde_json::from_value(result).expect("result");
    assert!(!stored.session_reset, "rotation must not discard the live session");
    assert!(stored.configuration.authenticated);
    assert_eq!(context.state.lock().await.session_id.as_deref(), Some("live-session"));
    assert_eq!(active_agent_config(&context).await.api_key, sentinel);
}

#[tokio::test]
#[serial]
async fn secret_store_for_inactive_provider_leaves_live_state_untouched() {
    let _storage = HermeticSecretStorage::new(&["ANTHROPIC_API_KEY"]);
    let sentinel = "sk-test-hermetic-inactive-key-004";
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(file_mode_context(&workspace), workspace.clone());
    context.state.lock().await.initialized = true;

    let (result, _) = handle_request(&context, &secret_store_request("anthropic", sentinel))
        .await
        .expect("secret store");
    let stored: protocol::SecretStoreResult = serde_json::from_value(result).expect("result");
    assert!(!stored.session_reset);
    assert!(stored.configuration.authenticated);
    assert_eq!(stored.configuration.provider, "gemini");
    {
        let state = context.state.lock().await;
        assert!(state.secret_store_auth.is_none());
        assert!(state.model_override_auth.is_none());
    }

    let resolved = vtcode_config::api_keys::resolve_credential_with_mode(
        "anthropic",
        "ANTHROPIC_API_KEY",
        Some(&workspace),
        AuthCredentialsStoreMode::File,
    )
    .expect("resolve")
    .and_then(|credential| credential.secret);
    assert_eq!(resolved.as_deref(), Some(sentinel));
    assert_eq!(active_agent_config(&context).await.api_key, "test-key");
}

#[tokio::test]
#[serial]
async fn secret_store_refreshes_failed_override_without_boot_side_effects() {
    let _storage = HermeticSecretStorage::new(&["ANTHROPIC_API_KEY"]);
    let sentinel = "sk-test-hermetic-override-key-005";
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(file_mode_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    {
        let mut state = context.state.lock().await;
        state.model_override = Some(ModelSelectParams {
            provider: "anthropic".to_string(),
            model: "test-model".to_string(),
        });
        state.model_override_auth = Some(catalog::OverrideAuth {
            api_key: String::new(),
            openai_chatgpt_auth: None,
            error: Some("API key not found for provider 'anthropic'.".to_string()),
        });
    }

    let (result, _) = handle_request(&context, &secret_store_request("anthropic", sentinel))
        .await
        .expect("secret store");
    let stored: protocol::SecretStoreResult = serde_json::from_value(result).expect("result");
    assert!(stored.session_reset);
    assert!(stored.configuration.authenticated);
    assert_eq!(stored.configuration.provider, "anthropic");
    {
        let state = context.state.lock().await;
        assert!(state.secret_store_auth.is_none(), "override saves must not touch boot auth");
        assert!(state.model_override_auth.as_ref().is_some_and(|auth| auth.error.is_none()));
    }
    assert_eq!(active_agent_config(&context).await.api_key, sentinel);
}
