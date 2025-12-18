use serde_json::json;
use tempfile::TempDir;
use vtcode_core::tools::ToolRegistry;
use std::fs;

async fn setup_registry(root: &std::path::Path) -> ToolRegistry {
    let mut registry = ToolRegistry::new(root.to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry
}

#[tokio::test]
async fn test_multiple_chunks_precision() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("multi_chunk.txt");
    let original_content = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\n";
    fs::write(&file_path, original_content).unwrap();

    let patch_text = r#"*** Begin Patch
*** Update File: multi_chunk.txt
@@
 line 1
-line 2
+line 2 modified
 line 3
@@
 line 8
-line 9
+line 9 modified
 line 10
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["success"].as_bool().unwrap_or(false), "Tool failed: {:?}", result);

    let new_content = fs::read_to_string(&file_path).unwrap();
    let expected_content = "line 1\nline 2 modified\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9 modified\nline 10\n";
    assert_eq!(new_content, expected_content);
}

#[tokio::test]
async fn test_fuzzy_matching_whitespace() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("fuzzy.txt");
    // Original has some trailing spaces and different indentation
    let original_content = "  indented line\nline with trailing space   \nlast line\n";
    fs::write(&file_path, original_content).unwrap();

    // Patch has normalized whitespace
    let patch_text = r#"*** Begin Patch
*** Update File: fuzzy.txt
@@
-  indented line
+  indented line modified
-line with trailing space
+line with trailing space modified
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["success"].as_bool().unwrap_or(false), "Tool failed: {:?}", result);

    let new_content = fs::read_to_string(&file_path).unwrap();
    // Note: The current implementation might preserve or normalize based on how matcher.rs works.
    // Let's see what happens.
    assert!(new_content.contains("indented line modified"));
    assert!(new_content.contains("line with trailing space modified"));
}

#[tokio::test]
async fn test_delete_file_operation() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("to_delete.txt");
    fs::write(&file_path, "delete me\n").unwrap();

    let patch_text = r#"*** Begin Patch
*** Delete File: to_delete.txt
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["success"].as_bool().unwrap_or(false), "Tool failed: {:?}", result);
    assert!(!file_path.exists());
}

#[tokio::test]
async fn test_mixed_operations() {
    let temp_dir = TempDir::new().unwrap();

    let update_path = temp_dir.path().join("update.txt");
    fs::write(&update_path, "original\n").unwrap();

    let delete_path = temp_dir.path().join("delete.txt");
    fs::write(&delete_path, "gone\n").unwrap();

    let patch_text = r#"*** Begin Patch
*** Add File: new.txt
+brand new
*** Delete File: delete.txt
*** Update File: update.txt
@@
-original
+updated
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    // We need to skip confirmations because we are deleting and adding in the same patch
    unsafe {
        std::env::set_var("VTCODE_SKIP_CONFIRMATIONS", "true");
    }

    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["success"].as_bool().unwrap_or(false), "Tool failed: {:?}", result);

    assert!(temp_dir.path().join("new.txt").exists());
    assert_eq!(fs::read_to_string(temp_dir.path().join("new.txt")).unwrap(), "brand new\n");
    assert!(!delete_path.exists());
    assert_eq!(fs::read_to_string(&update_path).unwrap(), "updated\n");
}

#[tokio::test]
async fn test_eof_handling_no_newline() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("eof.txt");
    fs::write(&file_path, "line 1\nline 2").unwrap(); // No trailing newline

    let patch_text = r#"*** Begin Patch
*** Update File: eof.txt
@@
 line 1
-line 2
+line 2 modified
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["success"].as_bool().unwrap_or(false), "Tool failed: {:?}", result);

    let new_content = fs::read_to_string(&file_path).unwrap();
    // The tool should ideally preserve the missing trailing newline if it was missing,
    // or at least handle it gracefully.
    assert_eq!(new_content, "line 1\nline 2 modified");
}

#[tokio::test]
async fn test_context_not_found_error() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("error.txt");
    fs::write(&file_path, "actual content\n").unwrap();

    let patch_text = r#"*** Begin Patch
*** Update File: error.txt
@@
-wrong content
+should fail
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["error"].is_object(), "Expected error object, got: {:?}", result);
    // Check if it's a SegmentNotFound error
    let error_msg = result["error"]["message"].as_str().unwrap();
    assert!(error_msg.contains("expected lines") || error_msg.contains("context"), "Unexpected error message: {}", error_msg);
}

#[tokio::test]
async fn test_empty_patch_error() {
    let temp_dir = TempDir::new().unwrap();
    let patch_text = "";

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["error"].is_object());
    assert!(result["error"]["message"].as_str().unwrap().contains("empty"));
}

#[tokio::test]
async fn test_invalid_format_error() {
    let temp_dir = TempDir::new().unwrap();
    let patch_text = "not a patch";

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["error"].is_object());
    assert!(result["error"]["message"].as_str().unwrap().contains("invalid patch format"));
}

#[tokio::test]
async fn test_missing_file_for_update_error() {
    let temp_dir = TempDir::new().unwrap();
    let patch_text = r#"*** Begin Patch
*** Update File: missing.txt
@@
-anything
+something
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["error"].is_object());
    assert!(result["error"]["message"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_add_existing_file_error() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("exists.txt");
    fs::write(&file_path, "already here\n").unwrap();

    let patch_text = r#"*** Begin Patch
*** Add File: exists.txt
+new content
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["error"].is_object());
    assert!(result["error"]["message"].as_str().unwrap().contains("invalid patch operation"));
}

#[tokio::test]
async fn test_crlf_handling() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("crlf.txt");
    fs::write(&file_path, "line 1\r\nline 2\r\n").unwrap();

    let patch_text = r#"*** Begin Patch
*** Update File: crlf.txt
@@
-line 1
+line 1 modified
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["success"].as_bool().unwrap_or(false), "Tool failed: {:?}", result);

    let new_content = fs::read_to_string(&file_path).unwrap();
    // Now we preserve CRLF!
    assert!(new_content.contains("line 1 modified\r\n"));
    assert!(new_content.contains("line 2\r\n"));
}

#[tokio::test]
async fn test_diff_preview_correctness() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("preview.txt");
    fs::write(&file_path, "line 1\nline 2\n").unwrap();

    let patch_text = r#"*** Begin Patch
*** Update File: preview.txt
@@
-line 1
+line 1 modified
*** End Patch"#;

    let mut registry = setup_registry(temp_dir.path()).await;
    let result = registry
        .execute_tool("apply_patch", json!({ "input": patch_text }))
        .await
        .unwrap();

    assert!(result["success"].as_bool().unwrap_or(false));

    let diff_preview = result["diff_preview"]["content"].as_str().unwrap();
    assert!(diff_preview.contains("-line 1"));
    assert!(diff_preview.contains("+line 1 modified"));
    assert!(diff_preview.contains("@@"));
}


