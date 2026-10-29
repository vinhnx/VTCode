use super::*;

#[tokio::test]
#[cfg(unix)]
async fn pipe_session_drain_clears_so_old_output_does_not_reappear() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "drain-clear".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf hello".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("drain-clear")
        .await?
        .expect("pipe sessions should expose activity receiver");

    timeout(Duration::from_secs(2), activity_rx.changed()).await??;
    let drained = manager
        .read_session_output("drain-clear", true)
        .await?
        .expect("should drain hello");
    assert!(drained.contains("hello"));

    let stale = manager.read_session_output("drain-clear", true).await?;
    assert!(stale.is_none(), "drained output must not reappear: {stale:?}");

    manager.close_session("drain-clear").await?;
    Ok(())
}
#[tokio::test]
#[cfg(unix)]
async fn pipe_session_returns_new_output_after_drain() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "drain-resume".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "printf one; sleep 1; printf two".to_string(),
            ],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let mut activity_rx = manager
        .activity_receiver("drain-resume")
        .await?
        .expect("pipe sessions should expose activity receiver");

    timeout(Duration::from_secs(2), activity_rx.changed()).await??;
    let _first = manager.read_session_output("drain-resume", true).await?;

    timeout(Duration::from_secs(3), activity_rx.changed()).await??;
    let second = manager
        .read_session_output("drain-resume", true)
        .await?
        .expect("should drain post-drain output");
    assert!(second.contains("two"), "output produced after a drain must still be returned: {second:?}");

    manager.close_session("drain-resume").await?;
    Ok(())
}
#[tokio::test]
async fn pipe_output_buffer_peek_is_idempotent_and_non_consuming() {
    let buffer = super::super::PipeOutputBuffer::default();
    buffer.append("hello", 5).await;

    let first = buffer.peek_pending().await;
    let second = buffer.peek_pending().await;
    assert_eq!(first, second);
    assert_eq!(first, Some("hello".to_string()));
}
#[tokio::test]
async fn pipe_output_buffer_drain_returns_exactly_once() {
    let buffer = super::super::PipeOutputBuffer::default();
    buffer.append("hello", 5).await;

    let first = buffer.drain_pending().await;
    let second = buffer.drain_pending().await;
    assert_eq!(first, Some("hello".to_string()));
    assert_eq!(second, None);
}
#[tokio::test]
async fn pipe_output_buffer_drain_clears_internal_pending_length() {
    let buffer = super::super::PipeOutputBuffer::default();
    buffer.append("hello", 5).await;

    buffer.drain_pending().await;
    let peek: Option<String> = buffer.peek_pending().await;
    assert!(peek.is_none(), "buffer must be empty after drain: {peek:?}");
}
#[tokio::test]
async fn pipe_output_buffer_append_after_drain_returns_only_fresh_output() {
    let buffer = super::super::PipeOutputBuffer::default();
    buffer.append("first", 5).await;
    buffer.drain_pending().await;

    buffer.append("second", 6).await;
    let output = buffer.peek_pending().await;
    assert_eq!(output, Some("second".to_string()));
}
#[tokio::test]
async fn pipe_output_buffer_bounds_preview_and_tracks_total_bytes() {
    let buffer = super::super::PipeOutputBuffer::default();
    let chunk = "x".repeat(super::super::PIPE_OUTPUT_HEAD_BYTES * 4);
    buffer.append(&chunk, chunk.len()).await;

    let preview = buffer.peek_pending().await.expect("bounded preview");
    assert!(preview.len() <= super::super::PIPE_OUTPUT_HEAD_BYTES * 3);
    let (total_bytes, truncated) = buffer.stats().await;
    assert_eq!(total_bytes, chunk.len() as u64);
    assert!(truncated);
}
/// Close must not deadlock or panic when a reader still holds
/// `output_read_lock` past `EXEC_SESSION_OUTPUT_READ_LOCK_TIMEOUT`.
/// Post-close reads must fail with a normal error, not a panic.
#[tokio::test]
async fn close_proceeds_cleanly_when_output_read_lock_is_held() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "lock-race".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "printf hello".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let record = manager.session_record("lock-race").await?;
    let _held = record.output_read_lock.lock().await;

    // Close waits at most EXEC_SESSION_OUTPUT_READ_LOCK_TIMEOUT (1s) for
    // the lock, then proceeds; bound the whole call so a hang fails fast.
    let closed = timeout(Duration::from_secs(5), manager.close_session("lock-race")).await;
    let session = closed
        .expect("close_session must finish while the read lock is held")
        .expect("close_session must succeed after the lock acquire timeout");
    assert_eq!(session.id.as_str(), "lock-race");

    drop(_held);

    let after = manager.read_session_output("lock-race", true).await;
    assert!(after.is_err(), "post-close read must be a clean error, got {after:?}");
    Ok(())
}
