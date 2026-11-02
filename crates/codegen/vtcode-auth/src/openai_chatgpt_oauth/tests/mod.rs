pub(super) use super::*;
pub(super) use crate::AuthCallbackOutcome;
pub(super) use crate::generate_pkce_challenge;
pub(super) use assert_fs::TempDir;
pub(super) use async_trait::async_trait;
pub(super) use serial_test::serial;
pub(super) use std::path::PathBuf;
pub(super) use std::sync::Arc;

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

mod auth_flows;
mod session_merge;
