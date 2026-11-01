use super::*;
use tempfile::TempDir;

#[tokio::test]
async fn detects_same_mtime_same_size_hash_mismatch() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "before\n")?;

    let first = snapshot_path_async(file.clone()).await?;
    std::fs::write(&file, "after!\n")?;
    let second = snapshot_path_async(file.clone()).await?;

    assert_eq!(first.size_bytes, second.size_bytes);
    assert_ne!(first.sha256, second.sha256);
    Ok(())
}

#[tokio::test]
async fn accepted_snapshot_exposes_exact_baseline_and_detects_later_edit() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "accepted\n")?;
    let monitor = EditedFileMonitor::new();
    let accepted = monitor.accept_disk_version_snapshot(&file).await?;
    std::fs::write(&file, "later change\n")?;
    assert_eq!(accepted.text_content.as_deref(), Some("accepted\n"));
    let conflict = monitor
        .detect_conflict(&file, None, None)
        .await?
        .expect("newer edit must conflict");
    assert_eq!(conflict.read_snapshot.and_then(|snapshot| snapshot.text_content).as_deref(), Some("accepted\n"));
    assert_eq!(conflict.disk_snapshot.and_then(|snapshot| snapshot.text_content).as_deref(), Some("later change\n"));
    Ok(())
}

#[tokio::test]
async fn suppresses_self_write_event() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "before\n")?;
    let monitor = EditedFileMonitor::new();

    monitor.track_read(&file).await?;
    std::fs::write(&file, "after\n")?;
    monitor.record_agent_write_text(&file, "after\n")?;
    monitor.debug_process_path_change(&file).await?;

    assert!(
        monitor
            .detect_conflict(&file, Some("after\n".to_string()), None)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn expected_agent_snapshot_does_not_hide_external_follow_up_change() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "before\n")?;
    let monitor = EditedFileMonitor::new();

    monitor.track_read(&file).await?;
    std::fs::write(&file, "after\n")?;
    monitor.record_agent_write_text(&file, "after\n")?;

    std::fs::write(&file, "after formatted\n")?;
    monitor.debug_process_path_change(&file).await?;

    let conflict = monitor.detect_conflict(&file, Some("agent\n".to_string()), None).await?;
    assert!(conflict.is_some());
    Ok(())
}

#[tokio::test]
async fn later_read_invalidates_old_agent_write_marker() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    let monitor = EditedFileMonitor::new();
    std::fs::write(&file, "agent version\n")?;
    monitor.record_agent_write_text(&file, "agent version\n")?;
    std::fs::write(&file, "read version\n")?;
    monitor.track_read(&file).await?;
    std::fs::write(&file, "agent version\n")?;
    let conflict = monitor
        .detect_conflict(&file, None, None)
        .await?
        .expect("new change must conflict");
    assert_eq!(conflict.read_snapshot.and_then(|s| s.text_content).as_deref(), Some("read version\n"));
    assert_eq!(conflict.disk_snapshot.and_then(|s| s.text_content).as_deref(), Some("agent version\n"));
    Ok(())
}

#[tokio::test]
async fn queues_mutations_in_order() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "hello\n")?;
    let monitor = Arc::new(EditedFileMonitor::new());
    let normalized_file = normalize_event_path(&file);

    let first = monitor.acquire_mutation(&file).await;
    let monitor_clone = Arc::clone(&monitor);
    let file_clone = file.clone();
    let waiter = tokio::spawn(async move {
        let lease = monitor_clone.acquire_mutation(&file_clone).await;
        lease.path().to_path_buf()
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    drop(first);
    let acquired = waiter.await?;
    assert_eq!(acquired, normalized_file);
    Ok(())
}

#[tokio::test]
async fn concurrent_mutation_waiters_all_acquire_without_stall() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "hello\n")?;
    let monitor = Arc::new(EditedFileMonitor::new());
    let normalized_file = normalize_event_path(&file);

    let first = monitor.acquire_mutation(&file).await;
    let mut waiters = Vec::new();
    for _ in 0..8 {
        let monitor_clone = Arc::clone(&monitor);
        let file_clone = file.clone();
        waiters.push(tokio::spawn(async move {
            let lease = monitor_clone.acquire_mutation(&file_clone).await;
            lease.path().to_path_buf()
        }));
    }

    // Let every waiter park inside `acquire_mutation` so the release below
    // must wake a registered waiter. A lost wakeup would stall here.
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(first);

    let acquired = tokio::time::timeout(Duration::from_secs(10), async {
        let mut paths = Vec::new();
        for waiter in waiters {
            paths.push(waiter.await?);
        }
        Ok::<_, anyhow::Error>(paths)
    })
    .await??;
    assert_eq!(acquired.len(), 8);
    for path in &acquired {
        assert_eq!(*path, normalized_file);
    }
    Ok(())
}

#[tokio::test]
async fn detects_external_change_after_read() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "before\n")?;
    let monitor = EditedFileMonitor::new();

    monitor.track_read(&file).await?;
    std::fs::write(&file, "external\n")?;
    monitor.debug_process_path_change(&file).await?;

    let conflict = monitor.detect_conflict(&file, Some("agent\n".to_string()), None).await?;
    assert!(conflict.is_some());
    Ok(())
}

#[tokio::test]
async fn cancelled_waiter_does_not_block_following_mutation() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "hello\n")?;
    let monitor = Arc::new(EditedFileMonitor::new());
    let normalized_file = normalize_event_path(&file);

    let first = monitor.acquire_mutation(&file).await;

    let pending_monitor = Arc::clone(&monitor);
    let pending_file = file.clone();
    let pending = tokio::spawn(async move {
        let _lease = pending_monitor.acquire_mutation(&pending_file).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    pending.abort();

    let next_monitor = Arc::clone(&monitor);
    let next_file = file.clone();
    let next = tokio::spawn(async move {
        let lease = next_monitor.acquire_mutation(&next_file).await;
        lease.path().to_path_buf()
    });

    drop(first);

    let acquired = tokio::time::timeout(Duration::from_secs(1), next).await??;
    assert_eq!(acquired, normalized_file);
    Ok(())
}

#[tokio::test]
async fn clears_conflict_state_when_disk_returns_to_read_snapshot() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "before\n")?;
    let monitor = EditedFileMonitor::new();

    monitor.track_read(&file).await?;
    std::fs::write(&file, "external one\n")?;
    monitor.debug_process_path_change(&file).await?;

    let first_conflict = monitor
        .detect_conflict(&file, Some("agent\n".to_string()), None)
        .await?
        .expect("expected initial conflict");
    assert!(first_conflict.emit_hitl_notification);

    std::fs::write(&file, "before\n")?;
    monitor.debug_process_path_change(&file).await?;
    assert!(
        monitor
            .detect_conflict(&file, Some("agent\n".to_string()), None)
            .await?
            .is_none()
    );

    std::fs::write(&file, "external two\n")?;
    monitor.debug_process_path_change(&file).await?;
    let second_conflict = monitor
        .detect_conflict(&file, Some("agent\n".to_string()), None)
        .await?
        .expect("expected renewed conflict");
    assert!(second_conflict.emit_hitl_notification);
    Ok(())
}

#[tokio::test]
async fn stale_path_queries_expose_external_change_without_clearing_state() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "before\n")?;
    let monitor = EditedFileMonitor::new();

    monitor.track_read(&file).await?;
    let tracked_paths = monitor.tracked_paths();
    assert_eq!(tracked_paths.len(), 1);
    let tracked_path = tracked_paths[0].clone();
    assert!(monitor.stale_tracked_paths().is_empty());

    std::fs::write(&file, "external\n")?;
    monitor.debug_process_path_change(&file).await?;

    let freshness = monitor.tracked_path_freshness();
    assert_eq!(freshness.len(), 1);
    assert_eq!(freshness[0].path, tracked_path);
    assert!(freshness[0].is_stale);
    assert!(freshness[0].fingerprint.is_some());
    assert_eq!(monitor.stale_tracked_paths(), vec![freshness[0].path.clone()]);
    assert_eq!(monitor.tracked_path_fingerprint(&freshness[0].path), freshness[0].fingerprint);
    Ok(())
}

#[tokio::test]
async fn untracked_event_leaves_no_tracked_state() -> Result<()> {
    let temp = TempDir::new()?;
    let sibling = temp.path().join("untracked.log");
    std::fs::write(&sibling, "build output\n")?;
    let monitor = EditedFileMonitor::new();

    // No `track_read` — the path was never read or written by the agent, so
    // the event must be dropped before any file snapshot is taken.
    monitor.debug_process_path_change(&sibling).await?;

    assert!(monitor.tracked_paths().is_empty());
    Ok(())
}

#[tokio::test]
async fn untracked_sibling_event_does_not_block_tracked_processing() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("tracked.txt");
    let sibling = temp.path().join("untracked.log");
    std::fs::write(&file, "before\n")?;
    std::fs::write(&sibling, "build output\n")?;
    let monitor = EditedFileMonitor::new();

    monitor.track_read(&file).await?;
    monitor.debug_process_path_change(&sibling).await?;
    assert!(monitor.tracked_path_freshness().iter().all(|entry| !entry.is_stale));

    std::fs::write(&file, "external\n")?;
    monitor.debug_process_path_change(&file).await?;
    assert_eq!(monitor.stale_tracked_paths().len(), 1);
    Ok(())
}

#[tokio::test]
async fn override_requires_matching_approved_snapshot() -> Result<()> {
    let temp = TempDir::new()?;
    let file = temp.path().join("sample.txt");
    std::fs::write(&file, "before\n")?;
    let monitor = EditedFileMonitor::new();

    monitor.track_read(&file).await?;
    std::fs::write(&file, "external one\n")?;
    let approved_snapshot = snapshot_path_async(file.clone()).await?;

    std::fs::write(&file, "external two\n")?;

    let conflict = monitor
        .detect_conflict(&file, Some("agent\n".to_string()), Some(approved_snapshot))
        .await?;
    assert!(conflict.is_some());
    Ok(())
}

#[test]
fn normalizes_missing_event_paths_via_canonical_parent() -> Result<()> {
    let temp = TempDir::new()?;
    let missing = temp.path().join("missing.txt");
    let canonical_parent = canonicalize(temp.path())?;

    assert_eq!(normalize_event_path(&missing), canonical_parent.join("missing.txt"));
    Ok(())
}
