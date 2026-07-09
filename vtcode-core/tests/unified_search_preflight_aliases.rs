#![allow(missing_docs)]

use anyhow::Result;
use serde_json::json;
use tempfile::TempDir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn preflight_rejects_removed_unified_search_aliases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    for alias in [
        tools::UNIFIED_SEARCH,
        tools::GREP_FILE,
        tools::LIST_FILES,
        tools::GET_ERRORS,
        tools::SEARCH_TOOLS,
        tools::GREP,
        "Search text",
        "structural search",
        "repo_browser.list_files",
        "repo_browser.grep_file",
        "fetch",
        "errors",
        "tool_discovery",
    ] {
        let err = registry
            .preflight_validate_call(
                alias,
                &json!({
                    "pattern": "ReasoningStage",
                    "path": ".",
                    "max_results": 10
                }),
            )
            .expect_err("removed unified_search alias should not resolve");
        assert!(
            err.to_string().contains("Unknown tool"),
            "{alias} should be rejected before schema validation: {err}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn separate_web_skill_and_discovery_names_do_not_route_to_unified_search() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    for (name, args) in [
        (tools::WEB_SEARCH, json!({"query": "rust ast-grep"})),
        (tools::WEB_FETCH, json!({"url": "https://example.com"})),
        (tools::FETCH_URL, json!({"url": "https://example.com"})),
        ("web", json!({"url": "https://example.com"})),
        (tools::LIST_SKILLS, json!({})),
        (tools::LOAD_SKILL, json!({"name": "example"})),
        (tools::MCP_SEARCH_TOOLS, json!({"query": "database"})),
    ] {
        match registry.preflight_validate_call(name, &args) {
            Ok(outcome) => assert_ne!(
                outcome.normalized_tool_name,
                tools::UNIFIED_SEARCH,
                "{name} must remain a separate affordance"
            ),
            Err(err) => assert!(
                err.to_string().contains("Unknown tool"),
                "{name} may be deferred, but must not fail through unified_search schema: {err}"
            ),
        }
    }
    Ok(())
}

#[tokio::test]
async fn code_search_rejects_text_grep_action() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let err = registry
        .preflight_validate_call(
            tools::CODE_SEARCH,
            &json!({
                "action": "grep",
                "pattern": "ReasoningStage",
                "path": "."
            }),
        )
        .expect_err("text grep belongs in exec_command.cmd with rg");

    let text = err.to_string();
    assert!(text.contains("Invalid arguments for tool 'code_search'"));
    assert!(text.contains("grep"));
    Ok(())
}

#[tokio::test]
async fn code_search_accepts_structural_and_outline_actions() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let structural = registry.preflight_validate_call(
        tools::CODE_SEARCH,
        &json!({
            "action": "structural",
            "pattern": "fn $NAME() {}",
            "path": ".",
            "lang": "rust",
            "max_results": 5
        }),
    )?;
    assert_eq!(structural.normalized_tool_name, tools::CODE_SEARCH);
    assert!(structural.readonly_classification);

    let outline = registry.preflight_validate_call(
        tools::CODE_SEARCH,
        &json!({
            "action": "outline",
            "path": ".",
            "view": "digest"
        }),
    )?;
    assert_eq!(outline.normalized_tool_name, tools::CODE_SEARCH);
    assert!(outline.readonly_classification);
    Ok(())
}
