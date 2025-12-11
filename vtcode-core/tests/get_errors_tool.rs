use std::path::PathBuf;
use vtcode_core::tools::registry::ToolRegistry;

#[tokio::test]
async fn test_get_errors_tool_not_registered() {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let registry = ToolRegistry::new(root).await;
    let has = registry.has_tool("get_errors").await;
    assert!(
        !has,
        "get_errors tool should not be registered after deprecation"
    );
}
