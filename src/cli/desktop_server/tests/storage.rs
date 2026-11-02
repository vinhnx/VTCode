use super::*;
use serial_test::serial;

/// Isolated session-store scope: `VT_SESSION_DIR` rooted at a temp dir.
/// Restores the environment on drop.
struct IsolatedSessionStore {
    _temp: tempfile::TempDir,
    _env: vtcode_commons::env_lock::EnvGuard,
    previous: Option<std::ffi::OsString>,
}

impl IsolatedSessionStore {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("sessions dir");
        let env = vtcode_commons::env_lock::lock();
        let previous = std::env::var_os(vtcode_core::utils::session_archive::SESSION_DIR_ENV);
        env.set_var(vtcode_core::utils::session_archive::SESSION_DIR_ENV, temp.path());
        Self { _temp: temp, _env: env, previous }
    }

    fn path(&self) -> &Path {
        self._temp.path()
    }
}

impl Drop for IsolatedSessionStore {
    fn drop(&mut self) {
        self._env
            .restore_var(vtcode_core::utils::session_archive::SESSION_DIR_ENV, self.previous.clone());
    }
}

fn storage_request(id: &str, method: &str, params: Value) -> Request {
    Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String(id.into()),
        method: method.into(),
        params,
    }
}

async fn initialized_service_context(workspace: &Path) -> ServiceContext {
    let (context, _) = test_service_context(test_startup_context(workspace), workspace.to_path_buf());
    context.state.lock().await.initialized = true;
    context
}

fn write_sized_file(path: &Path, size: usize) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("fixture dir");
    }
    std::fs::write(path, vec![0u8; size]).expect("fixture file");
}

#[tokio::test]
#[serial]
async fn storage_summary_accounts_isolated_stores() {
    let sessions = IsolatedSessionStore::new();
    write_sized_file(&sessions.path().join("a.json"), 10);
    write_sized_file(&sessions.path().join("b.json"), 20);
    let workspace = tempfile::tempdir().expect("workspace");
    write_sized_file(&workspace.path().join(".vtcode/worktrees/wt1/file.bin"), 5);
    write_sized_file(&workspace.path().join(".vtcode/sessions/s1/events.jsonl"), 7);

    let context = initialized_service_context(workspace.path()).await;
    let request = storage_request("summary", protocol::method::STORAGE_SUMMARY, json!({}));
    let (result, shutdown) = handle_request(&context, &request).await.expect("storage summary");
    assert!(!shutdown);
    let summary: protocol::StorageSummary = serde_json::from_value(result).expect("summary");
    assert_eq!(summary.sessions_bytes, 30);
    assert_eq!(summary.sessions_count, 2);
    assert_eq!(summary.worktrees_bytes, 5);
    assert_eq!(summary.worktrees_count, 1);
    assert_eq!(summary.memory_bytes, 7);
    assert_eq!(summary.total_bytes, 30 + 5 + 7 + summary.other_bytes);
    assert_eq!(summary.workspace, workspace.path().display().to_string());
    assert!(!summary.state_dir.is_empty());
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn storage_summary_skips_symlinked_content() {
    let sessions = IsolatedSessionStore::new();
    let workspace = tempfile::tempdir().expect("workspace");
    let memory_dir = workspace.path().join(".vtcode/sessions/s1");
    write_sized_file(&memory_dir.join("events.jsonl"), 7);
    let outside = sessions.path().join("outside.bin");
    write_sized_file(&outside, 100);
    std::os::unix::fs::symlink(&outside, memory_dir.join("linked.bin")).expect("fixture symlink");

    let context = initialized_service_context(workspace.path()).await;
    let request = storage_request("summary-links", protocol::method::STORAGE_SUMMARY, json!({}));
    let (result, _) = handle_request(&context, &request).await.expect("storage summary");
    let summary: protocol::StorageSummary = serde_json::from_value(result).expect("summary");
    assert_eq!(summary.memory_bytes, 7);
}

#[tokio::test]
#[serial]
async fn storage_summary_bounds_walk_depth() {
    let _sessions = IsolatedSessionStore::new();
    let workspace = tempfile::tempdir().expect("workspace");
    let mut deep = workspace.path().join(".vtcode/sessions");
    for level in 0..80 {
        deep = deep.join(format!("nested-{level}"));
    }
    write_sized_file(&deep.join("deep.bin"), 11);
    write_sized_file(&workspace.path().join(".vtcode/sessions/s1/events.jsonl"), 7);

    let context = initialized_service_context(workspace.path()).await;
    let request = storage_request("summary-depth", protocol::method::STORAGE_SUMMARY, json!({}));
    let (result, _) = handle_request(&context, &request).await.expect("storage summary");
    let summary: protocol::StorageSummary = serde_json::from_value(result).expect("summary");
    assert_eq!(summary.memory_bytes, 7);
}

#[tokio::test]
#[serial]
async fn session_delete_removes_unselected_archives() {
    let sessions = IsolatedSessionStore::new();
    write_sized_file(&sessions.path().join("gone.json"), 4);
    let workspace = tempfile::tempdir().expect("workspace");
    let context = initialized_service_context(workspace.path()).await;

    let request = storage_request("delete", protocol::method::SESSION_DELETE, json!({"sessionId": "gone"}));
    let (result, shutdown) = handle_request(&context, &request).await.expect("delete session");
    assert!(!shutdown);
    let outcome: protocol::SessionDeleteResult = serde_json::from_value(result).expect("delete result");
    assert_eq!(outcome.session_id, "gone");
    assert!(outcome.deleted);
    assert!(!sessions.path().join("gone.json").exists());

    let request = storage_request("delete-again", protocol::method::SESSION_DELETE, json!({"sessionId": "gone"}));
    let (result, _) = handle_request(&context, &request).await.expect("delete missing session");
    let outcome: protocol::SessionDeleteResult = serde_json::from_value(result).expect("delete result");
    assert!(!outcome.deleted);
}

#[tokio::test]
async fn session_delete_refuses_the_selected_session() {
    let workspace = tempfile::tempdir().expect("workspace");
    let context = initialized_service_context(workspace.path()).await;
    context.state.lock().await.session_id = Some("live".into());

    let request = storage_request("delete-live", protocol::method::SESSION_DELETE, json!({"sessionId": "live"}));
    let error = handle_request(&context, &request).await.expect_err("refuse selected session");
    assert!(error.to_string().contains("selected"), "{error}");
}

#[tokio::test]
async fn worktree_list_is_empty_without_managed_dir() {
    let workspace = tempfile::tempdir().expect("workspace");
    let context = initialized_service_context(workspace.path()).await;

    let request = storage_request("worktrees", protocol::method::WORKTREE_LIST, json!({}));
    let (result, shutdown) = handle_request(&context, &request).await.expect("list worktrees");
    assert!(!shutdown);
    let entries: Vec<protocol::WorktreeEntry> = serde_json::from_value(result).expect("worktree entries");
    assert!(entries.is_empty());
}

#[tokio::test]
async fn worktree_remove_missing_reports_not_found() {
    let workspace = tempfile::tempdir().expect("workspace");
    let context = initialized_service_context(workspace.path()).await;

    let request = storage_request("remove-missing", protocol::method::WORKTREE_REMOVE, json!({"name": "nope"}));
    let error = handle_request(&context, &request).await.expect_err("remove missing worktree");
    assert!(error.to_string().contains("does not exist"), "{error}");
}
