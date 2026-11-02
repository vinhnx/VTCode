use super::*;

#[cfg(unix)]
#[tokio::test]
async fn test_exec_command_write_preserves_whitespace() {
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "mode": "pty",
                "stdin": true,
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
    let combined_output = format!("{}{}", write["output"].as_str().unwrap_or_default(), tail_output);
    assert!(combined_output.contains("<  keep  >"), "combined output was: {combined_output:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn test_exec_command_write_stdin_continues_session() {
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "cat",
                "stdin": true,
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start public exec command session");
    let sid = exec_session_id(&start);

    let write = registry
        .execute_tool(
            "write_stdin",
            json!({
                "session_id": sid.as_str(),
                "chars": "  keep  \n",
                "yield_time_ms": 250,
            }),
        )
        .await
        .expect("continue public exec command session");

    assert_eq!(write["success"], true);
    assert_eq!(write["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(write["is_exited"].as_bool(), Some(false));
    assert!(
        write["output"].as_str().unwrap_or_default().contains("  keep  "),
        "write_stdin output was: {write:?}"
    );

    let _closed = registry
        .execute_tool(
            "close_pty_session",
            json!({
                "session_id": sid.as_str(),
            }),
        )
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_write_stdin_empty_chars_polls_without_sending_input() {
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "IFS= read -r line; printf '<%s>\\n' \"$line\"",
                "stdin": true,
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start command waiting for stdin");
    let sid = exec_session_id(&start);

    let poll = registry
        .execute_tool(
            "write_stdin",
            json!({
                "session_id": sid.as_str(),
                "chars": "",
                "yield_time_ms": 25,
                "max_output_tokens": 4,
            }),
        )
        .await
        .expect("poll public exec command session");

    assert_eq!(poll["success"], true);
    assert_eq!(poll["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(poll["is_exited"].as_bool(), Some(false));
    assert_eq!(poll["output"].as_str().unwrap_or_default(), "");

    let write = registry
        .execute_tool(
            "write_stdin",
            json!({
                "session_id": sid.as_str(),
                "chars": "  keep  \n",
                "yield_time_ms": 250,
            }),
        )
        .await
        .expect("write exact bytes after public poll");

    assert_eq!(write["success"], true);
    assert_eq!(write["session_id"].as_str(), Some(sid.as_str()));
    assert!(
        write["output"].as_str().unwrap_or_default().contains("<  keep  >"),
        "write_stdin output was: {write:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_write_stdin_empty_poll_honours_output_cap() {
    let (temp, registry) = temp_registry_with_config(Some(
        r#"[context.dynamic]
enabled = true
tool_output_threshold = 64
max_spooled_files = 7
spool_max_age_secs = 12
"#,
    ))
    .await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "sleep 0.5; printf 'abcdefghijklmnopqrstuvwxyz\\n'; sleep 2",
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start delayed output command");
    let sid = exec_session_id(&start);

    let poll = registry
        .execute_tool(
            "write_stdin",
            json!({
                "session_id": sid.as_str(),
                "chars": "",
                "yield_time_ms": 1000,
                "max_output_tokens": 1,
            }),
        )
        .await
        .expect("poll with capped output");

    assert_eq!(poll["success"], true);
    assert_eq!(poll["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(poll["is_exited"].as_bool(), Some(false));
    assert_eq!(poll["truncated"].as_bool(), Some(true));
    let spool_path = poll["spool_path"].as_str().expect("capped poll output should be spooled");
    assert!(spool_path.contains("write_stdin_"), "spool path should use the public tool name: {spool_path}");
    let spooled = fs::read_to_string(temp.path().join(spool_path)).expect("read spool file");
    assert!(
        spooled.contains("abcdefghijklmnopqrstuvwxyz"),
        "spool file should contain full polled output: {spooled:?}"
    );

    let _closed = registry
        .execute_tool(
            "close_pty_session",
            json!({
                "session_id": sid.as_str(),
            }),
        )
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_repeated_write_stdin_empty_polls_observe_fresh_output_and_exit() {
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "sleep 0.5; printf 'delayed-output\\n'; sleep 1.2",
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start delayed command");
    let sid = exec_session_id(&start);
    let poll_args = json!({
        "session_id": sid.as_str(),
        "chars": "",
        "yield_time_ms": 1000,
        "max_output_tokens": 16,
    });

    let first = registry
        .execute_tool("write_stdin", poll_args.clone())
        .await
        .expect("first public poll");

    assert_eq!(first["success"], true);
    assert_eq!(first["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(first["is_exited"].as_bool(), Some(false));
    assert!(
        first["output"].as_str().unwrap_or_default().contains("delayed-output"),
        "first poll output was: {first:?}"
    );
    assert!(first.get("reused_recent_result").is_none());

    let second = registry
        .execute_tool("write_stdin", poll_args)
        .await
        .expect("second identical public poll");

    assert_eq!(second["success"], true);
    assert_eq!(second["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(second["is_exited"].as_bool(), Some(true));
    assert_eq!(second["exit_code"].as_i64(), Some(0));
    assert!(second.get("reused_recent_result").is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn test_write_stdin_wait_deadline_preserves_reusable_session() {
    let (temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "sleep 0.2; printf 'wait-output'; i=0; while [ $i -lt 9000 ]; do printf 'x'; i=$((i+1)); done; printf '\\n'; sleep 2",
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start long-running command");
    let sid = exec_session_id(&start);

    let in_progress = registry
        .execute_tool(
            "write_stdin",
            json!({
                "action": "wait",
                "session_id": sid.as_str(),
                "wait_timeout_seconds": 1,
                "max_output_tokens": 16,
            }),
        )
        .await
        .expect("wait until the explicit deadline");

    assert_eq!(in_progress["success"], true);
    assert_eq!(in_progress["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(in_progress["is_exited"].as_bool(), Some(false));

    let completed = registry
        .execute_tool(
            "write_stdin",
            json!({
                "action": "wait",
                "session_id": sid.as_str(),
                "wait_timeout_seconds": 3,
                "max_output_tokens": 16,
            }),
        )
        .await
        .expect("continue waiting on the reusable session");

    assert_eq!(completed["success"], true);
    assert_eq!(completed["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(completed["is_exited"].as_bool(), Some(true));
    assert_eq!(completed["exit_code"].as_i64(), Some(0));
    let spool_path = completed["spool_path"]
        .as_str()
        .expect("wait should expose the complete output spool");
    let spooled = fs::read_to_string(temp.path().join(spool_path)).expect("read complete waited output");
    assert!(spooled.contains("wait-output"), "spooled wait output was: {spooled:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn test_write_stdin_wait_spools_high_volume_pipe_output() {
    let (temp, registry) = temp_registry().await;
    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "tty": false,
                "cmd": "i=0; while [ $i -lt 20000 ]; do printf 'line-%05d\\n' \"$i\"; i=$((i+1)); done; sleep 1",
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start high-volume pipe command");
    let sid = exec_session_id(&start);

    let completed = registry
        .execute_tool(
            "write_stdin",
            json!({
                "action": "wait",
                "session_id": sid.as_str(),
                "wait_timeout_seconds": 10,
                "max_output_tokens": 8,
            }),
        )
        .await
        .expect("wait for high-volume pipe command");

    assert_eq!(completed["success"], true);
    assert_eq!(completed["is_exited"], true);
    assert_eq!(completed["exit_code"].as_i64(), Some(0));
    assert!(completed["output_truncated"].as_bool().is_some(), "wait should expose truncation state");
    assert!(completed["total_output_bytes"].as_u64().unwrap_or_default() > 200_000);

    let spool_path = completed["spool_path"].as_str().expect("wait should expose pipe spool");
    let spooled = fs::read_to_string(temp.path().join(spool_path)).expect("read high-volume pipe spool");
    assert!(spooled.starts_with("line-00000\n"), "first pipe output chunk was lost");
    assert!(spooled.contains("line-19999\n"), "last pipe output chunk was lost");
    assert_eq!(spooled.lines().count(), 20_000);
    assert_eq!(completed["spool_line_count"], 20_000);
}
