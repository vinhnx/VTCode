use serde_json::json;
use tempfile::TempDir;
use vtcode_core::tools::ToolRegistry;

const SAMPLE_PATCH: &str =
    "*** Begin Patch\n*** Add File: hello.txt\n+Hello from patch alias!\n*** End Patch\n";

#[tokio::test]
async fn apply_patch_supports_patch_alias() {
    let temp_dir = TempDir::new().unwrap();
    let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();

    let result = registry
        .execute_tool("apply_patch", json!({ "patch": SAMPLE_PATCH }))
        .await
        .expect("apply_patch should succeed with alias");

    assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
    let applied = result
        .get("applied")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(applied.len(), 1);

    let created = temp_dir.path().join("hello.txt");
    assert!(created.exists(), "patch should create new file");
    let contents = tokio::fs::read_to_string(created)
        .await
        .expect("created file should be readable");
    assert_eq!(contents, "Hello from patch alias!\n");
}

#[tokio::test]
async fn apply_patch_supports_diff_alias() {
    let temp_dir = TempDir::new().unwrap();
    let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();

    let result = registry
        .execute_tool("apply_patch", json!({ "diff": SAMPLE_PATCH }))
        .await
        .expect("apply_patch should succeed with diff alias");

    assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
}
