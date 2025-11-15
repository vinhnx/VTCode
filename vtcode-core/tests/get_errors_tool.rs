use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::registry::ToolRegistry;

#[tokio::test]
async fn test_get_errors_tool_exists_and_runs() {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut registry = ToolRegistry::new(root).await;
    let has = registry
        .has_tool(vtcode_core::config::constants::tools::GET_ERRORS)
        .await;
    assert!(has, "get_errors tool should be registered");
    let result = registry
        .execute_tool(
            vtcode_core::config::constants::tools::GET_ERRORS,
            json!({"limit": 1}),
        )
        .await;
    assert!(result.is_ok(), "Executing get_errors should not error");
}
