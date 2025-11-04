use assert_fs::prelude::*;
use assert_fs::TempDir;
use serde_json::json;
use std::fs;
use vtcode_core::ToolRegistry;
use vtcode_core::config::constants::tools;

#[tokio::test]
async fn list_files_pagination_and_default_response_format() {
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_path_buf();

    // Create workspace files
    fs::create_dir_all(ws.join("src")).unwrap();
    fs::write(ws.join("src/a.rs"), "fn a() {}\n").unwrap();
    fs::write(ws.join("src/b.rs"), "fn b() {}\n").unwrap();

    // Workspace policy with constraints
    let vtcode_dir = ws.join(".vtcode");
    fs::create_dir_all(&vtcode_dir).unwrap();
    fs::write(
        vtcode_dir.join("tool-policy.json"),
        json!({
            "version": 1,
            "available_tools": [tools::LIST_FILES],
            "policies": { tools::LIST_FILES: "allow" },
            "constraints": { tools::LIST_FILES: { "max_items_per_call": 10, "default_response_format": "concise" } }
        }).to_string(),
    ).unwrap();

    let mut registry = ToolRegistry::new(ws.clone()).await;
    registry.allow_all_tools().await.unwrap_or_else(|err| {
        panic!("tool policy should be available for test: {}", err);
    });
    let out = registry
        .execute_tool(
            tools::LIST_FILES,
            json!({
                "path": "src",
                "page": 1,
                "per_page": 1
            }),
        )
        .await
        .unwrap();

    assert_eq!(out["response_format"], "concise");
    assert_eq!(out["page"], 1);
    assert_eq!(out["per_page"], 1);
}

#[tokio::test]
#[ignore]
async fn grep_file_default_concise_and_cap() {
    // Skip if ripgrep is not available
    if std::process::Command::new("rg")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping grep_file_default_concise_and_cap: ripgrep not installed");
        return;
    }
    let dir = tempdir().unwrap();
    let ws = dir.path().to_path_buf();
    fs::write(ws.join("file.txt"), "TODO: one\nTODO: two\n").unwrap();

    // Minimal policy that allows grep and caps results
    let vtcode_dir = ws.join(".vtcode");
    fs::create_dir_all(&vtcode_dir).unwrap();
    fs::write(
        vtcode_dir.join("tool-policy.json"),
        json!({
            "version": 1,
            "available_tools": [tools::GREP_FILE],
            "policies": { tools::GREP_FILE: "allow" },
            "constraints": { tools::GREP_FILE: { "max_results_per_call": 1, "default_response_format": "concise" } }
        }).to_string(),
    ).unwrap();

    let mut registry = ToolRegistry::new(ws.clone()).await;
    let out = registry
        .execute_tool(
            tools::GREP_FILE,
            json!({
                "pattern": "TODO",
                "path": ".",
                "max_results": 1000
            }),
        )
        .await
        .unwrap();

    // Defaulted to concise
    assert_eq!(out["response_format"], "concise");
    // Cap applied and guidance may be present
    assert!(out["matches"].as_array().unwrap().len() <= 1);
}
