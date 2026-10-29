use super::*;

#[tokio::test]
async fn test_pty_functionality() {
    let (_temp, registry) = temp_registry().await;

    // Run an allow-listed command and verify output is captured
    let result = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "ls Cargo.toml",
                "tty": true,
            }),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    assert_eq!(response["success"], true);
    let output = response["output"].as_str().unwrap_or_default();
    assert!(output.contains("Cargo.toml"));
    assert!(response["session_id"].as_str().is_some());
    assert!(response["command"].as_str().unwrap_or_default().contains("ls"));
    assert!(response["working_directory"].is_string() || response.get("working_directory").is_none());
    assert!(response["rows"].is_number());
    assert!(response["cols"].is_number());
    assert!(response["is_exited"].is_boolean());
}

#[tokio::test]
async fn test_pty_functionality_with_exit_code() {
    let (_temp, registry) = temp_registry().await;

    // Run an allow-listed command that exits with a non-zero code
    let result = registry
        .execute_tool(
            "exec_command",
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
    // but the exit code should be non-zero. Different `ls` implementations
    // use different codes for a missing operand/path.
    assert_eq!(response["success"], true);
    // Check for exit_code field (may be "code" or "exit_code" depending on implementation)
    let exit_code = response["exit_code"].as_i64().or_else(|| response["code"].as_i64());
    assert!(
        exit_code.is_some_and(|code| code != 0),
        "expected present non-zero exit code, got {exit_code:?}; response={response:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_pty_run_returns_live_session_after_yield_window() {
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
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
    assert!(start.get("session_id").and_then(|value| value.as_str()).is_some());
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
    let (_temp, registry) = temp_registry().await;

    let result = registry
        .execute_tool(
            "exec_command",
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
async fn test_exec_command_small_pipe_output_stays_inline_without_spool_reference() {
    let (_temp, registry) = temp_registry_with_config(Some(
        r#"[context.dynamic]
enabled = true
tool_output_threshold = 64
"#,
    ))
    .await;

    let result = registry
        .execute_tool(
            "exec_command",
            json!({
                "tty": false,
                "cmd": "printf 'small output\\n'",
            }),
        )
        .await
        .expect("small pipe command result");

    assert_eq!(result["success"], true);
    assert_eq!(result["output"].as_str(), Some("small output\n"));
    assert!(result.get("spool_path").is_none(), "small output leaked spool path: {result:?}");
    assert!(result.get("output_spooled").is_none(), "small output leaked spool marker: {result:?}");
    assert!(result.get("spool_complete").is_none(), "small output leaked spool lifecycle: {result:?}");
    assert!(result.get("spool_pending").is_none(), "small output leaked pending metadata: {result:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn test_create_pty_session_uses_requested_shell() {
    let (_temp, registry) = temp_registry().await;

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
    let output = create_result["output"].as_str().unwrap_or_default().to_ascii_lowercase();
    assert!(output.contains("sh"));
}

#[tokio::test]
async fn test_pty_output_has_no_ansi_codes() {
    let (_temp, registry) = temp_registry().await;

    let result = registry
        .execute_tool(
            "exec_command",
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
    assert!(!output.contains("\x1b["), "Output should not contain ANSI escape codes");
    assert!(!output.contains("\u{001b}["), "Output should not contain ANSI escape codes");

    // Verify we got actual file names
    assert!(
        output.contains("Cargo.toml") || output.contains("cargo") || output.len() > 10,
        "Output should contain actual filenames, not just escape codes"
    );
}

#[tokio::test]
async fn test_pty_command_not_found_handling() {
    let (_temp, registry) = temp_registry().await;
    // Keep the command-not-found assertion meaningful while still exercising
    // the shared command allowlist: explicitly allow this inert fixture name.
    let mut commands_config = CommandsConfig::default();
    commands_config
        .allow_list
        .push("this_command_definitely_does_not_exist_12345".to_string());
    registry.apply_commands_config(&commands_config);

    // Run a command that definitely doesn't exist
    let result = registry
        .execute_tool(
            "exec_command",
            json!({
                "mode": "pty",
                "shell": "/bin/bash",
                "command": "this_command_definitely_does_not_exist_12345",
                // Force a deterministic login shell to test robust extraction logic (shell -l -c ...)
                "login": true
            }),
        )
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();

    assert_eq!(response["success"], true);
    // Check for exit_code field (may be "code" or "exit_code" depending on implementation)
    let exit_code = response["exit_code"].as_i64().or_else(|| response["code"].as_i64());
    assert_eq!(exit_code, Some(127));

    // Check that we have error information in message or output
    let message = response["message"].as_str().unwrap_or_default();
    let output = response["output"].as_str().unwrap_or_default();
    let combined = format!("{message} {output}").to_lowercase();

    // Should indicate command not found in some way
    assert!(
        combined.contains("not found")
            || combined.contains("not exist")
            || combined.contains("127")
            || output.contains("this_command_definitely_does_not_exist_12345"),
        "Should indicate command not found. message='{message}', output='{output}'"
    );
}
