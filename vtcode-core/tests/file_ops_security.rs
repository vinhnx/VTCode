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

#[tokio::test]
async fn move_file_rejects_workspace_escape_source() {
    let workspace = TempDir::new().expect("temp workspace");
    let outside = workspace
        .path()
        .parent()
        .expect("temp dir parent")
        .join("outside-move-source.txt");
    tokio::fs::write(&outside, "keep-me")
        .await
        .expect("outside source file should be created");

    let grep_manager = Arc::new(GrepSearchManager::new(workspace.path().to_path_buf()));
    let file_tool = FileOpsTool::new(workspace.path().to_path_buf(), grep_manager);

    let args = json!({
        "path": "../outside-move-source.txt",
        "destination": "moved.txt"
    });
    let error = file_tool
        .move_file(args)
        .await
        .expect_err("move_file should reject source path traversal")
        .to_string();

    assert!(error.contains("outside the workspace"));
    assert!(
        outside.exists(),
        "source file outside workspace should remain untouched"
    );
    assert!(
        !workspace.path().join("moved.txt").exists(),
        "destination should not be created for blocked move"
    );
}

#[tokio::test]
async fn copy_file_rejects_workspace_escape_destination() {
    let workspace = TempDir::new().expect("temp workspace");
    let inside = workspace.path().join("inside-copy-source.txt");
    tokio::fs::write(&inside, "copy-me")
        .await
        .expect("inside source file should be created");
    let outside = workspace
        .path()
        .parent()
        .expect("temp dir parent")
        .join("outside-copy-destination.txt");
    if outside.exists() {
        let _ = tokio::fs::remove_file(&outside).await;
    }

    let grep_manager = Arc::new(GrepSearchManager::new(workspace.path().to_path_buf()));
    let file_tool = FileOpsTool::new(workspace.path().to_path_buf(), grep_manager);

    let args = json!({
        "path": "inside-copy-source.txt",
        "destination": "../outside-copy-destination.txt"
    });
    let error = file_tool
        .copy_file(args)
        .await
        .expect_err("copy_file should reject destination path traversal")
        .to_string();

    assert!(error.contains("outside the workspace"));
    assert!(
        !outside.exists(),
        "destination outside workspace should not be created"
    );
    assert!(
        inside.exists(),
        "source file should remain in workspace after blocked copy"
    );
}
