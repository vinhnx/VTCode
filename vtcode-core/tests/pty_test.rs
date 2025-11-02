use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn test_pty_functionality() {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    // Run an allow-listed command and verify output is captured
    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": "ls",
                "args": ["Cargo.toml"],
            }),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    assert_eq!(response["success"], true);
    let output = response["output"].as_str().unwrap_or_default();
    assert!(output.contains("Cargo.toml"));
}

#[tokio::test]
async fn test_pty_functionality_with_exit_code() {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    // Run an allow-listed command that exits with a non-zero code
    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": "ls",
                "args": ["this_file_does_not_exist"],
            }),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    // The command should execute successfully (no error in execution)
    // but the exit code should be 1
    assert_eq!(response["success"], true);
    assert_eq!(response["code"].as_i64(), Some(1));
}

#[cfg(unix)]
#[tokio::test]
async fn test_pty_shell_option_runs_through_requested_shell() {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "shell": "sh",
                "command": "echo shell-check",
            }),
        )
        .await
        .expect("shell run result");

    assert_eq!(result["success"], true);
    let output = result["output"].as_str().unwrap_or_default();
    assert!(output.contains("shell-check"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_create_pty_session_uses_requested_shell() {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let create_result = registry
        .execute_tool(
            "create_pty_session",
            json!({
                "session_id": "shell-session",
                "command": "bash",
                "shell": "/bin/sh"
            }),
        )
        .await
        .expect("create session result");

    assert_eq!(create_result["success"], true);
    assert_eq!(create_result["session_id"], "shell-session");
    let command = create_result["command"].as_str().unwrap_or_default();
    assert!(command.contains("sh"));

    registry
        .execute_tool(
            "close_pty_session",
            json!({
                "session_id": "shell-session"
            }),
        )
        .await
        .expect("close session result");
}

#[tokio::test]
async fn test_pty_output_has_no_ansi_codes() {
    let mut registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": "ls",
                "args": ["-a"],
            }),
        )
        .await
        .expect("ls result");

    assert_eq!(result["success"], true);
    let output = result["output"].as_str().unwrap_or_default();

    // Check that output doesn't contain ANSI escape sequences
    assert!(
        !output.contains("\x1b["),
        "Output should not contain ANSI escape codes"
    );
    assert!(
        !output.contains("\u{001b}["),
        "Output should not contain ANSI escape codes"
    );

    // Verify we got actual file names
    assert!(
        output.contains("Cargo.toml") || output.contains("cargo") || output.len() > 10,
        "Output should contain actual filenames, not just escape codes"
    );
}
