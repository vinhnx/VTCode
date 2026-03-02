use serde_json::json;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn delete_file_tool_removes_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file_path = tmp.path().join("to_delete.txt");
    tokio::fs::write(&file_path, b"hello").await.unwrap();

    let registry = ToolRegistry::new(tmp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();

    // Ensure file exists
    assert!(file_path.exists());

    // Call unified_file with action="delete" (delete_file is an alias)
    let args = json!({ "action": "delete", "path": "to_delete.txt" });
    let val = registry.execute_tool("unified_file", args).await.unwrap();
    assert_eq!(val.get("success").and_then(|v| v.as_bool()), Some(true));
    // Check for deleted field or kind field indicating file was removed
    let deleted = val.get("deleted").and_then(|v| v.as_bool()).or_else(|| {
        val.get("kind")
            .and_then(|v| v.as_str())
            .map(|k| k == "file")
    });
    assert_eq!(deleted, Some(true));

    // Verify removal
    assert!(!file_path.exists());
}

#[tokio::test]
async fn delete_file_tool_removes_directory_recursively() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir_path = tmp.path().join("nested");
    let child_path = dir_path.join("file.txt");
    tokio::fs::create_dir_all(&dir_path).await.unwrap();
    tokio::fs::write(&child_path, b"hi").await.unwrap();

    let registry = ToolRegistry::new(tmp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();

    let args = json!({ "action": "delete", "path": "nested", "recursive": true });
    let val = registry.execute_tool("unified_file", args).await.unwrap();

    assert_eq!(val.get("success").and_then(|v| v.as_bool()), Some(true));
    // Check for deleted field or kind field indicating directory was removed
    let deleted = val.get("deleted").and_then(|v| v.as_bool()).or_else(|| {
        val.get("kind")
            .and_then(|v| v.as_str())
            .map(|k| k == "directory")
    });
    assert_eq!(deleted, Some(true));
    assert!(!dir_path.exists());
}
