use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::{LspTool, Tool};

#[test]
fn test_lsp_tool_schema() {
    let tool = LspTool::new(PathBuf::from("."));
    let schema = tool.parameter_schema();

    if let Some(schema_obj) = schema {
        let properties = schema_obj.get("properties").expect("Should have properties");
        assert!(properties.get("operation").is_some());
        assert!(properties.get("server_command").is_some());
        assert!(properties.get("file_path").is_some());
    }
}

#[tokio::test]
async fn test_lsp_tool_start_fail() {
    let tool = LspTool::new(PathBuf::from("."));

    // Test starting a non-existent server
    let args = json!({
        "operation": "start",
        "server_command": "non_existent_server_12345"
    });

    let result = tool.execute(args).await;
    assert!(result.is_err());
}
