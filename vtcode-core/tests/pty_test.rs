use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn test_pty_functionality() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
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
    assert!(response["id"].as_str().is_some());
    assert!(
        response["command"]
            .as_str()
            .unwrap_or_default()
            .contains("ls")
    );
    assert!(response["working_directory"].is_string() || response["working_directory"].is_null());
    assert!(response["rows"].is_number());
    assert!(response["cols"].is_number());
    assert!(response["is_exited"].is_boolean());
}

#[tokio::test]
async fn test_pty_functionality_with_exit_code() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
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
async fn test_pty_waits_for_completion_over_yield() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": "sleep 1; echo done",
                "yield_time_ms": 50,
            }),
        )
        .await
        .expect("sleep result");

    assert_eq!(result["success"], true);
    assert_eq!(result.get("process_id"), None);
    assert_eq!(result["exit_code"].as_i64(), Some(0));
    assert!(result["id"].as_str().is_some());
    assert!(result["is_exited"].as_bool().unwrap_or(false));
    let output = result["output"].as_str().unwrap_or_default();
    assert!(output.contains("done"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_pty_shell_option_runs_through_requested_shell() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
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
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
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
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
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

#[tokio::test]
async fn test_pty_command_not_found_handling() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    // Run a command that definitely doesn't exist
    let result = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": "this_command_definitely_does_not_exist_12345",
                // Force login shell to test robust extraction logic (shell -l -c ...)
                "login": true
            }),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    assert_eq!(response["success"], true);
    assert_eq!(response["exit_code"].as_i64(), Some(127));

    // Check that we have the helpful message
    let message = response["message"].as_str().unwrap_or_default();
    assert!(message.contains("Command not found"));
    assert!(message.contains("this_command_definitely_does_not_exist_12345"));

    // Check that the output is NOT replaced (it should be the shell error)
    let output = response["output"].as_str().unwrap_or_default();
    assert!(!output.contains("EXIT CODE 127 IS FINAL"));

    // Check critical note is present
    let critical_note = response["critical_note"].as_str().unwrap_or_default();
    assert!(critical_note.contains("EXIT CODE 127 IS FINAL"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_read_pty_session_includes_command_context_fields() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let start = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": "sleep 1",
                "yield_time_ms": 10
            }),
        )
        .await
        .expect("start sleep command");

    let sid = start
        .get("process_id")
        .and_then(|v| v.as_str())
        .or_else(|| start.get("id").and_then(|v| v.as_str()))
        .expect("session id should be present")
        .to_string();

    let read = registry
        .execute_tool(
            "read_pty_session",
            json!({
                "session_id": sid,
                "yield_time_ms": 10
            }),
        )
        .await
        .expect("read pty session");

    assert_eq!(read["success"], true);
    assert_eq!(read["id"].as_str(), Some(sid.as_str()));
    assert!(
        read["command"]
            .as_str()
            .unwrap_or_default()
            .contains("sleep 1")
    );
    assert!(read["working_directory"].is_string() || read["working_directory"].is_null());
    assert!(read["rows"].is_number());
    assert!(read["cols"].is_number());
    assert!(read["is_exited"].is_boolean());

    let _ = registry
        .execute_tool(
            "close_pty_session",
            json!({
                "session_id": sid
            }),
        )
        .await;
}
