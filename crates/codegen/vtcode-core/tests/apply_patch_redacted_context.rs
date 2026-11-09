#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs, reason = "test assertions")]

use serde_json::json;
use vtcode_core::tools::ToolRegistry;

#[cfg(unix)]
#[tokio::test]
async fn redacted_shell_read_followed_by_patch_produces_safe_diagnosis() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sample.txt");
    let original = "before\npassword=fixture-private-value\nafter\n";
    std::fs::write(&path, original).unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry.allow_all_tools().await.unwrap();
    let read = registry
        .execute_tool("exec_command", json!({"cmd":"sed -n '1,3p' sample.txt", "yield_time_ms":1000}))
        .await
        .unwrap();
    assert_eq!(read["exit_code"], 0, "{read}");
    let visible = read["output"].as_str().unwrap();
    assert!(visible.contains("password=[REDACTED_SECRET]"), "{visible}");
    assert!(!visible.contains("fixture-private-value"));
    let patch = format!(
        "*** Begin Patch\n*** Update File: sample.txt\n@@\n-before\n+changed\n {}\n after\n*** End Patch",
        visible.lines().nth(1).unwrap()
    );
    let result = registry.execute_tool("apply_patch", json!({"input":patch})).await.unwrap();
    assert!(result.to_string().contains("RedactedContext"), "{result}");
    assert!(!result.to_string().contains("fixture-private-value"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[tokio::test]
async fn redacted_context_is_diagnosed_without_disclosing_or_changing_source() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sample.txt");
    let original = "before\npassword=fixture-private-value\nafter\n";
    std::fs::write(&path, original).unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry.allow_all_tools().await.unwrap();

    let patch = "*** Begin Patch\n*** Update File: sample.txt\n@@\n before\n-password=[REDACTED_SECRET]\n+password=replacement\n after\n*** End Patch";
    let result = registry.execute_tool("apply_patch", json!({"input": patch})).await.unwrap();
    let serialized = result.to_string();
    assert!(serialized.contains("RedactedContext"), "{serialized}");
    assert!(serialized.contains("Repeated reads"), "{serialized}");
    assert!(!serialized.contains("fixture-private-value"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

    let anchored = patch.replace("@@\n", "@@ fn imaginary\n");
    let result = registry.execute_tool("apply_patch", json!({"input": anchored})).await.unwrap();
    assert!(result.to_string().contains("RedactedContext"), "{result}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

    let split = "*** Begin Patch\n*** Update File: sample.txt\n@@\n-before\n+changed\n*** End Patch";
    let result = registry.execute_tool("apply_patch", json!({"input": split})).await.unwrap();
    assert_eq!(result["success"], true, "{result}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed\npassword=fixture-private-value\nafter\n");
}

#[tokio::test]
async fn literal_marker_and_unrelated_mismatch_keep_normal_recovery() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("sample.txt"), "before\nmarker=[REDACTED_SECRET]\nafter\n").unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry.allow_all_tools().await.unwrap();
    let patch = "*** Begin Patch\n*** Update File: sample.txt\n@@\n missing\n-marker=[REDACTED_SECRET]\n+changed\n*** End Patch";
    let result = registry.execute_tool("apply_patch", json!({"input": patch})).await.unwrap();
    let serialized = result.to_string();
    assert!(serialized.contains("ContextMismatch"), "{serialized}");
    assert!(!serialized.contains("RedactedContext"), "{serialized}");
    assert!(serialized.contains("Read the affected path once"), "{serialized}");
}

#[tokio::test]
async fn redacted_match_outside_numeric_hunk_range_is_not_a_redaction_failure() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sample.txt");
    let original = "password=fixture-private-value\nother\nlast\n";
    std::fs::write(&path, original).unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry.allow_all_tools().await.unwrap();
    let patch = "*** Begin Patch\n*** Update File: sample.txt\n@@ -2,1 +2,1 @@\n-password=[REDACTED_SECRET]\n+changed\n*** End Patch";
    let result = registry.execute_tool("apply_patch", json!({"input": patch})).await.unwrap();
    assert!(result.to_string().contains("ContextMismatch"), "{result}");
    assert!(!result.to_string().contains("RedactedContext"), "{result}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[tokio::test]
async fn redacted_insertion_anchor_is_diagnosed_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("sample.txt");
    let original = "before\npassword=fixture-private-value\nafter\n";
    std::fs::write(&path, original).unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.initialize_async().await.unwrap();
    registry.allow_all_tools().await.unwrap();
    let patch = "*** Begin Patch\n*** Update File: sample.txt\n@@ password=[REDACTED_SECRET]\n+new line\n*** End Patch";
    let result = registry.execute_tool("apply_patch", json!({"input":patch})).await.unwrap();
    assert!(result.to_string().contains("RedactedContext"), "{result}");
    assert!(!result.to_string().contains("fixture-private-value"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}
