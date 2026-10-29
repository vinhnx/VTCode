use super::*;

#[cfg(unix)]
#[tokio::test]
async fn test_spool_extent_matches_completed_output_for_pipe_and_pty() {
    for tty in [false, true] {
        let (temp, registry) = temp_registry().await;
        let start = registry
            .execute_tool(
                "exec_command",
                json!({
                    "tty": tty,
                    "cmd": "printf '%09000d\\nsecond\\nlast' 0",
                    "yield_time_ms": 0,
                }),
            )
            .await
            .unwrap();
        let completed = if start["is_exited"] == true {
            start
        } else {
            let sid = exec_session_id(&start);
            registry
                .execute_tool(
                    "write_stdin",
                    json!({
                        "action": "wait",
                        "session_id": sid,
                        "wait_timeout_seconds": 10,
                        "max_output_tokens": 16,
                    }),
                )
                .await
                .unwrap()
        };
        assert_eq!(completed["exit_code"], 0, "tty={tty}: {completed}");
        assert_eq!(completed["spool_line_count"], 3, "tty={tty}: {completed}");
        assert!(completed["spool_note"].as_str().unwrap().contains("EOF at line 3"));
        let path = completed["spool_path"].as_str().unwrap();
        let content = fs::read_to_string(temp.path().join(path)).unwrap();
        assert_eq!(content.lines().count(), 3);
        assert!(content.ends_with("last"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_reading_pipe_spool_file_does_not_create_a_second_spool_reference() {
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
    let spool_path = completed["spool_path"].as_str().expect("large output spool path");
    assert!(temp.path().join(spool_path).is_file());

    let read_result = registry
        .execute_tool(
            "exec_command",
            json!({
                "tty": false,
                "cmd": format!("cat {spool_path}"),
                "max_output_tokens": 8,
            }),
        )
        .await
        .expect("read spool file through exec command");

    assert_eq!(read_result["success"], true);
    assert!(read_result["output"].as_str().unwrap_or_default().contains("line-00000"));
    assert!(read_result.get("spool_path").is_none(), "spool read recursively spooled: {read_result:?}");
    assert!(read_result.get("output_spooled").is_none(), "spool read leaked marker: {read_result:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn test_write_stdin_reports_missing_and_closed_session_ids() {
    let (_temp, registry) = temp_registry().await;

    let missing = registry
        .execute_tool(
            "write_stdin",
            json!({
                "chars": "ignored\n",
            }),
        )
        .await;
    assert!(missing.is_err(), "missing session id should fail preflight");

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

    let close = registry
        .execute_tool(
            "close_pty_session",
            json!({
                "session_id": sid.as_str(),
            }),
        )
        .await
        .expect("close session");
    assert_eq!(close["success"], true);

    let closed = registry
        .execute_tool(
            "write_stdin",
            json!({
                "session_id": sid.as_str(),
                "chars": "after-close\n",
            }),
        )
        .await
        .expect("closed session should return structured tool error");
    assert!(closed.get("error").is_some(), "closed session response should be a structured error: {closed:?}");
    assert_eq!(closed["error"]["tool_name"], "write_stdin");
}

#[cfg(unix)]
#[tokio::test]
async fn test_write_stdin_output_cap_forces_spool_path() {
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
                "cmd": "cat",
                "stdin": true,
                "yield_time_ms": 0,
            }),
        )
        .await
        .expect("start public exec command session");
    let sid = exec_session_id(&start);
    let payload = "abcdefghijklmnopqrstuvwxyz\n";

    let write = registry
        .execute_tool(
            "write_stdin",
            json!({
                "session_id": sid.as_str(),
                "chars": payload,
                "yield_time_ms": 250,
                "max_output_tokens": 1,
            }),
        )
        .await
        .expect("continue with capped output");

    assert_eq!(write["success"], true);
    assert_eq!(write["truncated"].as_bool(), Some(true));
    let spool_path = write["spool_path"]
        .as_str()
        .expect("capped write_stdin output should be spooled");
    assert!(spool_path.contains("write_stdin_"), "spool path should use the public tool name: {spool_path}");
    let spooled = fs::read_to_string(temp.path().join(spool_path)).expect("read spool file");
    assert!(
        spooled.contains(payload.trim_end()),
        "spool file should contain full continuation output: {spooled:?}"
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
