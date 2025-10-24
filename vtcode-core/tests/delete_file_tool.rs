use serde_json::json;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn delete_file_tool_removes_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let file_path = tmp.path().join("to_delete.txt");
    tokio::fs::write(&file_path, b"hello").await.unwrap();

    let mut registry = ToolRegistry::new(tmp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();

    // Ensure file exists
    assert!(file_path.exists());

    // Call delete_file tool
    let args = json!({ "path": "to_delete.txt" });
    let val = registry.execute_tool("delete_file", args).await.unwrap();
    assert_eq!(val.get("success").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(val.get("deleted").and_then(|v| v.as_bool()), Some(true));

    // Verify removal
    assert!(!file_path.exists());
}
