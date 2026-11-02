use super::*;

#[cfg(unix)]
#[tokio::test]
async fn test_read_pty_session_includes_command_context_fields() {
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "sleep 1",
                "tty": true,
                "yield_time_ms": 10
            }),
        )
        .await
        .expect("start sleep command");

    let sid = exec_session_id(&start);

    let read = registry
        .execute_tool(
            "write_stdin",
            json!({
                "session_id": sid.as_str(),
                "chars": "",
                "yield_time_ms": 10
            }),
        )
        .await
        .expect("read pty session");

    assert_eq!(read["success"], true);
    assert_eq!(read["session_id"].as_str(), Some(sid.as_str()));
    assert!(read["command"].as_str().unwrap_or_default().contains("sleep 1"));
    assert!(read["working_directory"].is_string() || read.get("working_directory").is_none());
    assert!(read["rows"].is_number());
    assert!(read["cols"].is_number());
    assert!(read["is_exited"].is_boolean());

    let _closed = registry
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
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
            json!({
                "cmd": "bash -lc 'sleep 0.4; printf \"<alpha>\\n\"; sleep 1'",
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start delayed output command");

    let sid = exec_session_id(&start);

    let mut read = json!({});
    for attempt in 0..8 {
        read = registry
            .execute_tool(
                "write_stdin",
                json!({
                    "session_id": sid.as_str(),
                    "chars": "",
                    "yield_time_ms": 500 + (attempt * 250),
                }),
            )
            .await
            .expect("poll session output");
        if read["output"].as_str().unwrap_or_default().contains("<alpha>") {
            break;
        }
    }

    assert_eq!(read["success"], true);
    assert!(read["output"].as_str().unwrap_or_default().contains("<alpha>"));

    let _closed = registry
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
    let (_temp, registry) = temp_registry().await;

    let start = registry
        .execute_tool(
            "exec_command",
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
    assert_eq!(read["exit_code"].as_i64().or_else(|| read["code"].as_i64()), Some(0));
    assert_eq!(read["is_exited"].as_bool(), Some(true));

    let sessions = registry
        .execute_tool("list_pty_sessions", json!({}))
        .await
        .expect("list sessions");

    let active_sessions = sessions["sessions"].as_array().expect("sessions list should be an array");
    assert!(
        !active_sessions
            .iter()
            .any(|session| { session.get("id").and_then(|value| value.as_str()) == Some(sid.as_str()) })
    );

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
