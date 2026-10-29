use super::*;

#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn resume_from_listing_reuses_existing_archive_path() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let session_id = reserve_session_archive_identifier("ResumeWorkspace", None).await?;
    let metadata = SessionArchiveMetadata::new(
        "ResumeWorkspace",
        "/tmp/resume-workspace",
        "model-a",
        "provider-a",
        "light",
        "medium",
    );
    let archive = SessionArchive::new_with_identifier(metadata.clone(), session_id.clone())
        .await
        .context("failed to create initial archive")?;
    let original_path = archive.finalize(
        vec!["user: first".to_string()],
        1,
        vec!["read_file".to_string()],
        vec![SessionMessage::new(MessageRole::User, "first")],
    )?;

    let listing = find_session_by_identifier(&session_id)
        .await?
        .context("expected archived session listing")?;
    let resumed = SessionArchive::resume_from_listing(&listing, metadata);
    let resumed_path = resumed.finalize(
        vec!["user: second".to_string()],
        2,
        vec!["read_file".to_string()],
        vec![
            SessionMessage::new(MessageRole::User, "first"),
            SessionMessage::new(MessageRole::Assistant, "second"),
        ],
    )?;

    assert_eq!(resumed_path, original_path);
    let stored = fs::read_to_string(&resumed_path)
        .with_context(|| format!("failed to read stored session: {}", resumed_path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;
    assert_eq!(snapshot.total_messages, 2);
    assert_eq!(snapshot.messages.len(), 2);

    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn list_recent_sessions_orders_entries() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let first_metadata = SessionArchiveMetadata::new("First", "/tmp/first", "model-a", "provider-a", "light", "medium");
    let first_archive = SessionArchive::new(first_metadata.clone(), None).await?;
    first_archive.finalize(
        vec!["first".to_owned()],
        1,
        Vec::new(),
        vec![SessionMessage::new(MessageRole::User, "First")],
    )?;

    tokio::time::sleep(Duration::from_millis(10)).await;

    let second_metadata = SessionArchiveMetadata::new("Second", "/tmp/second", "model-b", "provider-b", "dark", "high");
    let second_archive = SessionArchive::new(second_metadata.clone(), None).await?;
    second_archive.finalize(
        vec!["second".to_owned()],
        2,
        vec!["tool_b".to_owned()],
        vec![SessionMessage::new(MessageRole::User, "Second")],
    )?;

    let listings = list_recent_sessions(10).await?;
    assert_eq!(listings.len(), 2);
    assert_eq!(listings[0].snapshot.metadata, second_metadata);
    assert_eq!(listings[1].snapshot.metadata, first_metadata);
    Ok(())
}
#[test]
fn session_archive_retention_prunes_oldest_by_count() -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    for idx in 0..3 {
        let path = temp_dir.path().join(format!("session-{idx}.json"));
        fs::write(&path, format!("{{\"idx\":{idx}}}"))
            .with_context(|| format!("failed to write {}", path.display()))?;
        std::thread::sleep(Duration::from_millis(5));
    }

    apply_session_retention_with_limits(
        temp_dir.path(),
        SessionRetentionLimits {
            max_files: 2,
            max_age_days: 365,
            max_total_size_bytes: 10 * BYTES_PER_MB,
        },
    )?;

    let mut remaining = fs::read_dir(temp_dir.path())
        .context("failed to list retained session files")?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter_map(|path| path.file_name().and_then(|name| name.to_str()).map(|name| name.to_string()))
        .collect::<Vec<_>>();
    remaining.sort();

    assert_eq!(remaining.len(), 2);
    assert!(!remaining.iter().any(|name| name == "session-0.json"));
    Ok(())
}
#[test]
fn session_archive_retention_prunes_by_total_size() -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    for idx in 0..2 {
        let path = temp_dir.path().join(format!("session-{idx}.json"));
        fs::write(&path, "x".repeat(800_000)).with_context(|| format!("failed to write {}", path.display()))?;
        std::thread::sleep(Duration::from_millis(5));
    }

    apply_session_retention_with_limits(
        temp_dir.path(),
        SessionRetentionLimits {
            max_files: 10,
            max_age_days: 365,
            max_total_size_bytes: BYTES_PER_MB,
        },
    )?;

    let remaining = fs::read_dir(temp_dir.path())
        .context("failed to list retained session files")?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();

    assert_eq!(remaining.len(), 1);
    let remaining_name = remaining[0].file_name().and_then(|name| name.to_str()).unwrap_or_default();
    assert_eq!(remaining_name, "session-1.json");
    Ok(())
}
#[test]
fn listing_previews_return_first_non_empty_lines() {
    let metadata = SessionArchiveMetadata::new("Workspace", "/tmp/ws", "model", "provider", "dark", "medium");
    let long_response = "response snippet ".repeat(6);
    let snapshot = SessionSnapshot {
        metadata,
        started_at: Utc::now(),
        ended_at: Utc::now(),
        total_messages: 2,
        distinct_tools: Vec::new(),
        transcript: Vec::new(),
        messages: vec![
            SessionMessage::new(MessageRole::System, ""),
            SessionMessage::new(MessageRole::User, "  prompt line\nsecond"),
            SessionMessage::new(MessageRole::Assistant, long_response.clone()),
        ],
        progress: None,
        error_logs: Vec::new(),
    };
    let listing = SessionListing {
        path: PathBuf::from("session-workspace.json"),
        snapshot,
    };

    assert_eq!(listing.first_prompt_preview(), Some("prompt line".to_owned()));
    let expected = truncate_preview(&long_response, 80);
    assert_eq!(listing.first_reply_preview(), Some(expected));
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn search_sessions_finds_keyword_case_insensitively() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new("SearchWS", "/tmp/s", "mod", "prov", "d", "m");
    let archive = SessionArchive::new(metadata, None).await?;

    let messages = vec![
        SessionMessage::new(MessageRole::User, "Where is the secret API key?"),
        SessionMessage::new(MessageRole::Assistant, "The secret key is defined in .env.local"),
    ];
    archive.finalize(vec![], 2, vec![], messages)?;

    let results = search_sessions("SECRET KEY", 10, 5).await?;
    assert!(!results.is_empty());
    assert!(results[0].content_snippet.to_lowercase().contains("secret key"));
    assert_eq!(results[0].role, MessageRole::Assistant);

    Ok(())
}
