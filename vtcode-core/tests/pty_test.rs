use serde_json::json;
use std::path::PathBuf;
use vtcode_core::tools::ToolRegistry;

fn exec_session_id(response: &serde_json::Value) -> String {
    response
        .get("process_id")
        .and_then(|value| value.as_str())
        .or_else(|| response.get("session_id").and_then(|value| value.as_str()))
        .expect("session id should be present")
        .to_string()
}

async fn read_session_until_exit(
    registry: &ToolRegistry,
    session_id: &str,
    attempts: usize,
    yield_time_ms: u64,
) -> (String, serde_json::Value) {
    let mut output = String::new();
    let mut last = json!({});

    for attempt in 0..attempts {
        let read = registry
            .execute_tool(
                "read_pty_session",
                json!({
                    "session_id": session_id,
                    "yield_time_ms": yield_time_ms + attempt as u64,
                }),
            )
            .await
            .expect("read pty session");

        output.push_str(read["output"].as_str().unwrap_or_default());
        if read["is_exited"].as_bool().unwrap_or(false) || read.get("error").is_some() {
            return (output, read);
        }
        last = read;
    }

    (output, last)
}

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
    assert!(response["session_id"].as_str().is_some());
    assert!(
        response["command"]
            .as_str()
            .unwrap_or_default()
            .contains("ls")
    );
    assert!(
        response["working_directory"].is_string() || response.get("working_directory").is_none()
    );
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
    // Check for exit_code field (may be "code" or "exit_code" depending on implementation)
    let exit_code = response["exit_code"]
        .as_i64()
        .or_else(|| response["code"].as_i64());
    assert_eq!(exit_code, Some(1));
}

#[cfg(unix)]
#[tokio::test]
async fn test_pty_run_returns_live_session_after_yield_window() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let start = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": ["bash", "-lc", "sleep 0.75; echo done"],
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start sleep result");
    let sid = exec_session_id(&start);

    assert_eq!(start["success"], true);
    assert_eq!(start["is_exited"].as_bool(), Some(false));
    assert!(
        start
            .get("session_id")
            .and_then(|value| value.as_str())
            .is_some()
    );
    assert!(start.get("exit_code").is_none());

    let (output, read) = read_session_until_exit(&registry, sid.as_str(), 20, 250).await;

    assert_eq!(read["success"], true);
    let exit_code = read["exit_code"].as_i64().or_else(|| read["code"].as_i64());
    assert_eq!(exit_code, Some(0));
    assert!(read["is_exited"].as_bool().unwrap_or(false));
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
                "command": "echo $0",
                "shell": "/bin/sh",
            }),
        )
        .await
        .expect("create session result");

    assert_eq!(create_result["success"], true);
    let output = create_result["output"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(output.contains("sh"));
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
    // Check for exit_code field (may be "code" or "exit_code" depending on implementation)
    let exit_code = response["exit_code"]
        .as_i64()
        .or_else(|| response["code"].as_i64());
    assert_eq!(exit_code, Some(127));

    // Check that we have error information in message or output
    let message = response["message"].as_str().unwrap_or_default();
    let output = response["output"].as_str().unwrap_or_default();
    let combined = format!("{} {}", message, output).to_lowercase();

    // Should indicate command not found in some way
    assert!(
        combined.contains("not found")
            || combined.contains("not exist")
            || combined.contains("127")
            || output.contains("this_command_definitely_does_not_exist_12345"),
        "Should indicate command not found. message='{}', output='{}'",
        message,
        output
    );
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

    let sid = exec_session_id(&start);

    let read = registry
        .execute_tool(
            "read_pty_session",
            json!({
                "session_id": sid.as_str(),
                "yield_time_ms": 10
            }),
        )
        .await
        .expect("read pty session");

    assert_eq!(read["success"], true);
    assert_eq!(read["session_id"].as_str(), Some(sid.as_str()));
    assert!(
        read["command"]
            .as_str()
            .unwrap_or_default()
            .contains("sleep 1")
    );
    assert!(read["working_directory"].is_string() || read.get("working_directory").is_none());
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

#[cfg(unix)]
#[tokio::test]
async fn test_inspect_does_not_drain_session_output() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let start = registry
        .execute_tool(
            "unified_exec",
            json!({
                "command": "bash -lc 'sleep 0.4; printf \"<alpha>\\n\"; sleep 1'",
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start delayed output command");

    let sid = exec_session_id(&start);

    let mut inspect = json!({});
    for attempt in 0..8 {
        inspect = registry
            .execute_tool(
                "unified_exec",
                json!({
                    "action": "inspect",
                    "session_id": sid.as_str(),
                    "yield_time_ms": 500 + (attempt * 250),
                    "head_lines": 5,
                    "tail_lines": 5,
                }),
            )
            .await
            .expect("inspect session output");
        if inspect["output"]
            .as_str()
            .unwrap_or_default()
            .contains("<alpha>")
        {
            break;
        }
    }

    assert_eq!(inspect["success"], true);

    let read = registry
        .execute_tool(
            "unified_exec",
            json!({
                "action": "poll",
                "session_id": sid.as_str(),
                "yield_time_ms": 10,
            }),
        )
        .await
        .expect("poll after inspect");

    assert_eq!(read["success"], true);
    assert!(
        inspect["output"]
            .as_str()
            .unwrap_or_default()
            .contains("<alpha>")
    );
    assert!(
        read["output"]
            .as_str()
            .unwrap_or_default()
            .contains("<alpha>")
    );

    let _ = registry
        .execute_tool(
            "close_pty_session",
            json!({
                "session_id": sid.as_str()
            }),
        )
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_exited_sessions_are_pruned_after_final_poll() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let start = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": ["bash", "-lc", "sleep 0.4; echo done"],
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start delayed exit command");

    let sid = exec_session_id(&start);

    let (_, read) = read_session_until_exit(&registry, sid.as_str(), 20, 200).await;

    assert_eq!(read["success"], true);
    assert_eq!(
        read["exit_code"].as_i64().or_else(|| read["code"].as_i64()),
        Some(0)
    );
    assert_eq!(read["is_exited"].as_bool(), Some(true));

    let sessions = registry
        .execute_tool("list_pty_sessions", json!({}))
        .await
        .expect("list sessions");

    let active_sessions = sessions["sessions"]
        .as_array()
        .expect("sessions list should be an array");
    assert!(!active_sessions.iter().any(|session| {
        session.get("id").and_then(|value| value.as_str()) == Some(sid.as_str())
    }));

    let reread = registry
        .execute_tool(
            "read_pty_session",
            json!({
                "session_id": sid.as_str(),
            }),
        )
        .await
        .expect("re-read should return a structured error");
    assert!(reread.get("error").is_some());

    let close = registry
        .execute_tool(
            "close_pty_session",
            json!({
                "session_id": sid.as_str(),
            }),
        )
        .await
        .expect("close after prune should return a structured error");
    assert!(close.get("error").is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn test_unified_exec_write_preserves_whitespace() {
    let registry = ToolRegistry::new(PathBuf::from(".")).await;
    registry.allow_all_tools().await.ok();

    let start = registry
        .execute_tool(
            "run_pty_cmd",
            json!({
                "mode": "pty",
                "command": ["bash", "-lc", "IFS= read -r line; printf '<%s>' \"$line\""],
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start read session");
    let sid = exec_session_id(&start);

    let write = registry
        .execute_tool(
            "send_pty_input",
            json!({
                "session_id": sid.as_str(),
                "chars": "  keep  \n",
                "yield_time_ms": 50,
            }),
        )
        .await
        .expect("write exact bytes");

    assert_eq!(write["success"], true);
    let (tail_output, final_read) = if write["is_exited"].as_bool().unwrap_or(false) {
        (String::new(), write.clone())
    } else {
        read_session_until_exit(&registry, sid.as_str(), 8, 100).await
    };
    assert_eq!(final_read["success"], true);
    let combined_output = format!(
        "{}{}",
        write["output"].as_str().unwrap_or_default(),
        tail_output
    );
    assert!(
        combined_output.contains("<  keep  >"),
        "combined output was: {combined_output:?}"
    );
}
