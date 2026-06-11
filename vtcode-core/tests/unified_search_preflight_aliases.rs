use anyhow::Result;
use serde_json::json;
use std::fs;
use tempfile::TempDir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn preflight_accepts_grep_file_alias_and_normalizes_to_unified_search() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry
        .preflight_validate_call(
            tools::GREP_FILE,
            &json!({
                "pattern": "LLMStreamEvent::",
                "path": "."
            }),
        )
        .expect("grep_file alias should resolve");
    assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_SEARCH);
    assert!(outcome.readonly_classification);
    Ok(())
}

#[tokio::test]
async fn preflight_infers_action_for_humanized_search_text_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry.preflight_validate_call(
        "Search text",
        &json!({
            "pattern": "ReasoningStage",
            "path": "."
        }),
    )?;

    assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_SEARCH);
    Ok(())
}

#[tokio::test]
async fn preflight_infers_structural_action_for_structural_search_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry.preflight_validate_call(
        "structural search",
        &json!({
            "pattern": "fn $NAME() {}",
            "path": ".",
            "lang": "rust"
        }),
    )?;

    assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_SEARCH);
    Ok(())
}

#[tokio::test]
async fn preflight_rejects_removed_repo_browser_list_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let err = registry
        .preflight_validate_call(
            "repo_browser.list_files",
            &json!({
                "path": "."
            }),
        )
        .expect_err("repo_browser.list_files alias should be rejected");
    assert!(err.to_string().contains("Unknown tool"));
    Ok(())
}

#[tokio::test]
async fn preflight_rejects_unified_search_when_action_cannot_be_inferred() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let err = registry
        .preflight_validate_call(
            tools::UNIFIED_SEARCH,
            &json!({
                "max_results": 10
            }),
        )
        .expect_err("preflight should reject when unified_search action is missing");

    let err_text = err.to_string();
    assert!(err_text.contains("Invalid arguments for tool 'unified_search'"));
    assert!(err_text.contains("action"));
    Ok(())
}

#[tokio::test]
async fn preflight_accepts_unified_search_with_case_variant_keys() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry.preflight_validate_call(
        tools::UNIFIED_SEARCH,
        &json!({
            "Pattern": "ReasoningStage",
            "Path": "."
        }),
    )?;

    assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_SEARCH);
    Ok(())
}

#[tokio::test]
async fn unified_search_list_with_blank_mode_executes_default_list() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::create_dir_all(temp_dir.path().join("src"))?;
    fs::write(temp_dir.path().join("src/lib.rs"), "pub fn lib() {}\n")?;
    fs::write(temp_dir.path().join("src/readme.md"), "# readme\n")?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let args = json!({
        "action": "list",
        "mode": "",
        "path": "src",
        "pattern": "*.rs"
    });

    let outcome = registry.preflight_validate_call(tools::UNIFIED_SEARCH, &args)?;
    assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_SEARCH);

    let result = registry
        .execute_tool_ref(tools::UNIFIED_SEARCH, &args)
        .await?;
    assert_eq!(result["mode"], json!("list"));
    assert_eq!(result["pattern"], json!("*.rs"));

    let items = result["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["path"], json!("src/lib.rs"));
    Ok(())
}
