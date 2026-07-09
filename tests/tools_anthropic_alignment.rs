#![allow(missing_docs)]
use assert_fs::TempDir;
use serde_json::json;
use std::fs;
use vtcode_core::ToolRegistry;
use vtcode_core::config::constants::tools;

#[tokio::test]
async fn search_dispatch_list_action_is_not_public() {
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_path_buf();

    // Create workspace files
    fs::create_dir_all(ws.join("src")).unwrap();
    fs::write(ws.join("src/a.rs"), "fn a() {}\n").unwrap();
    fs::write(ws.join("src/b.rs"), "fn b() {}\n").unwrap();

    let registry = ToolRegistry::new(ws.clone()).await;
    registry.allow_all_tools().await.unwrap_or_else(|err| {
        panic!("tool policy should be available for test: {err}");
    });
    let err = registry
        .execute_public_tool_ref(
            tools::UNIFIED_SEARCH,
            &json!({
                "action": "list",
                "path": "src",
                "page": 1,
                "per_page": 1
            }),
        )
        .await
        .expect_err("search_dispatch_internal should not be public");

    assert!(err.to_string().contains("Unknown tool"));
}

#[tokio::test]
async fn search_dispatch_grep_action_is_not_public() {
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_path_buf();
    fs::write(ws.join("file.txt"), "TODO: one\nTODO: two\n").unwrap();

    let registry = ToolRegistry::new(ws.clone()).await;
    let err = registry
        .execute_public_tool_ref(
            tools::UNIFIED_SEARCH,
            &json!({
                "action": "grep",
                "pattern": "TODO",
                "path": ".",
                "max_results": 1000
            }),
        )
        .await
        .expect_err("search_dispatch_internal should not be public");

    assert!(err.to_string().contains("Unknown tool"));
}
