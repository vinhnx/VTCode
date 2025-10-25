use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn test_pty_functionality() {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;

    // Test a simple echo command
    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "command": "echo",
                "args": ["hello world"]
            }),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    assert_eq!(response["success"], true);
    let output = response["output"].as_str().unwrap();
    assert!(output.contains("hello world"));
}

#[tokio::test]
async fn test_pty_functionality_with_exit_code() {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;

    // Test a command that exits with code 1
    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "command": "sh",
                "args": ["-c", "exit 1"]
            }),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    // The command should execute successfully (no error in execution)
    // but the exit code should be 1
    assert_eq!(response["success"], true);
}
