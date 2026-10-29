use super::*;

#[cfg(unix)]
#[tokio::test]
async fn public_session_controls_inspect_terminate_and_close() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    // This regression isolates the legacy transport/diff contract.
    registry.set_inspection_mode(vtcode_config::core::agent::InspectionMode::Off);
    registry.allow_all_tools().await?;
    let active = registry
        .execute_tool(
            tools::EXEC_COMMAND,
            json!({
                "cmd": "printf 'retained-evidence\\n'; sleep 60", "shell": "/bin/sh",
                "background": true, "yield_time_ms": 250,
            }),
        )
        .await?;
    let session_id = active["session_id"].as_str().unwrap();
    let inspection = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "inspect",
            }),
        )
        .await?;
    assert_eq!(inspection["content_type"], "exec_inspect");
    let terminated = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "terminate", "yield_time_ms": 1000,
            }),
        )
        .await?;
    assert!(terminated.get("exit_code").is_some(), "{terminated}");
    let closed = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "close",
            }),
        )
        .await?;
    assert_eq!(closed["success"], true);
    assert!(registry.exec_sessions.snapshot_session(session_id).await.is_err());
    let missing = registry
        .execute_tool(
            tools::WRITE_STDIN,
            json!({
                "session_id": session_id, "action": "close",
            }),
        )
        .await?;
    assert!(missing["error"]["message"].as_str().unwrap().contains("not found"));
    Ok(())
}
