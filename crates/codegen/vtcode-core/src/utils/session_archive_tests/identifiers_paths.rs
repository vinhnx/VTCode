use super::*;

#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn find_session_by_identifier_returns_match() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new("Sample", "/tmp/sample", "model-x", "provider-y", "dark", "medium");
    let archive = SessionArchive::new(metadata.clone(), None).await?;
    let messages = vec![
        SessionMessage::new(MessageRole::User, "Hi"),
        SessionMessage::new(MessageRole::Assistant, "Hello"),
    ];
    let path = archive.finalize(Vec::new(), messages.len(), Vec::new(), messages)?;
    let identifier = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow!("missing file stem"))?
        .to_string();

    let listing = find_session_by_identifier(&identifier)
        .await?
        .ok_or_else(|| anyhow!("expected session to be found"))?;
    assert_eq!(listing.identifier(), identifier);
    assert_eq!(listing.snapshot.metadata, metadata);

    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_archive_path_collision_adds_suffix() -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium");

    let started_at = Utc
        .with_ymd_and_hms(2025, 9, 25, 10, 15, 30)
        .unwrap()
        .with_nanosecond(123_456_000)
        .unwrap();

    let first_path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);
    fs::write(&first_path, "{}").context("failed to create sentinel file")?;

    let second_path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);

    assert_ne!(first_path, second_path);
    let second_name = second_path.file_name().and_then(|name| name.to_str()).expect("file name");
    assert!(second_name.contains("-01"));

    Ok(())
}
#[test]
fn session_archive_filename_includes_microseconds_and_pid() -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium");

    let started_at = Utc
        .with_ymd_and_hms(2025, 9, 25, 10, 15, 30)
        .unwrap()
        .with_nanosecond(654_321_000)
        .expect("nanosecond set");

    let path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);
    let name = path.file_name().and_then(|value| value.to_str()).expect("file name string");

    assert!(name.contains("20250925T101530Z_654321"));
    let pid_fragment = format!("{:05}", process::id());
    assert!(name.contains(&pid_fragment));

    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn reserve_session_identifier_can_be_reused_for_archive() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let session_id = reserve_session_archive_identifier("ExampleWorkspace", None).await?;
    assert!(session_id.starts_with("session-exampleworkspace-"));

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium")
            .with_debug_log_path(Some("/tmp/debug-session.log".to_string()));
    let archive = SessionArchive::new_with_identifier(metadata.clone(), session_id.clone())
        .await
        .context("failed to create archive with reserved session id")?;
    let path = archive.finalize(
        vec!["line one".to_owned()],
        1,
        vec![],
        vec![SessionMessage::new(MessageRole::User, "hello")],
    )?;
    let stored =
        fs::read_to_string(&path).with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("missing file stem"))?;
    assert_eq!(stem, session_id);
    assert_eq!(snapshot.metadata.debug_log_path, Some("/tmp/debug-session.log".to_string()));

    Ok(())
}
#[test]
fn generated_session_identifier_includes_workspace_and_custom_suffix() {
    let generated = generate_session_archive_identifier("Example Workspace", Some("branch".to_string()));

    assert!(generated.starts_with("session-example-workspace-"));
    assert!(generated.ends_with("-branch"));
}
