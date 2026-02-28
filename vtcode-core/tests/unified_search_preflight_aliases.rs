use anyhow::Result;
use serde_json::json;
use tempfile::TempDir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn preflight_infers_action_for_grep_file_alias() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry.preflight_validate_call(
        "grep_file",
        &json!({
            "pattern": "LLMStreamEvent::",
            "path": "."
        }),
    )?;

    assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_SEARCH);
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
async fn preflight_repo_browser_list_alias_still_normalizes_to_unified_search() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let outcome = registry.preflight_validate_call(
        "repo_browser.list_files",
        &json!({
            "path": "."
        }),
    )?;

    assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_SEARCH);
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
