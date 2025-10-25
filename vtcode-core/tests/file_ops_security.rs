use assert_fs::TempDir;
use serde_json::json;
use std::sync::Arc;
use vtcode_core::tools::file_ops::FileOpsTool;
use vtcode_core::tools::grep_file::GrepSearchManager;

#[tokio::test]
async fn create_file_rejects_workspace_escape() {
    let workspace = TempDir::new().expect("temp workspace");
    let outside = workspace
        .path()
        .parent()
        .expect("temp dir parent")
        .join("outside.txt");
    if outside.exists() {
        let _ = tokio::fs::remove_file(&outside).await;
    }

    let grep_manager = Arc::new(GrepSearchManager::new(workspace.path().to_path_buf()));
    let file_tool = FileOpsTool::new(workspace.path().to_path_buf(), grep_manager);

    let args = json!({
        "path": "../outside.txt",
        "content": "blocked"
    });
    let value = file_tool
        .create_file(args)
        .await
        .expect_err("tool should reject escapes");

    let message = value.to_string();
    assert!(
        message.contains("outside the workspace"),
        "expected workspace guard in error, got: {}",
        message
    );
    assert!(
        !outside.exists(),
        "create_file must not materialize escaped paths"
    );
}
