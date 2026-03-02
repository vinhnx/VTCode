use serde_json::json;
use tempfile::TempDir;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn create_file_succeeds_for_new_path() {
    let temp_dir = TempDir::new().unwrap();
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();

    // Use unified_file with action="write" and a new path (create_file is an alias)
    let args = json!({
        "action": "write",
        "path": "src/lib.rs",
        "content": "fn main() {}\n"
    });
    let result = registry
        .execute_tool("unified_file", args)
        .await
        .expect("tool execution should succeed");

    assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
    // Check for created field or successful write indication
    let created = result.get("created").and_then(|v| v.as_bool())
        .or_else(|| result.get("success").and_then(|v| v.as_bool()));
    assert_eq!(created, Some(true));

    let created_path = temp_dir.path().join("src/lib.rs");
    assert!(created_path.exists(), "file should be created on disk");
    let contents = tokio::fs::read_to_string(created_path)
        .await
        .expect("should read file content");
    assert_eq!(contents, "fn main() {}\n");
}

#[tokio::test]
async fn create_file_fails_when_file_exists() {
    let temp_dir = TempDir::new().unwrap();
    let existing_path = temp_dir.path().join("main.rs");
    tokio::fs::write(&existing_path, b"initial").await.unwrap();

    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();

    // Use unified_file with action="write" on existing file
    let args = json!({
        "action": "write",
        "path": "main.rs",
        "content": "replaced"
    });
    let value = registry
        .execute_tool("unified_file", args)
        .await
        .expect("tool execution should return error payload");

    // Should have error or not success
    let has_error = value.get("error").is_some()
        || value.get("success").and_then(|v| v.as_bool()) == Some(false);
    assert!(has_error, "expect error payload or success=false");

    let persisted = tokio::fs::read_to_string(existing_path)
        .await
        .expect("existing file should remain unchanged");
    assert_eq!(persisted, "initial");
}
