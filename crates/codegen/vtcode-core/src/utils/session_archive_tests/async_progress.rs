use super::*;

#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn throttled_async_progress_does_not_claim_durable_checkpoint() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new("ThrottleWS", "/tmp/throttle", "model", "provider", "dark", "medium");
    let archive = SessionArchive::new(metadata, None).await?;
    let initial_messages = vec![SessionMessage::new(MessageRole::User, "initial")];
    let first_status = archive
        .persist_progress_async_with_status(SessionProgressArgs {
            total_messages: initial_messages.len(),
            distinct_tools: Vec::new(),
            messages: initial_messages.clone(),
            recent_messages: initial_messages.clone(),
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: None,
            turn_diagnostics: None,
        })
        .await?;
    assert!(first_status.is_persisted());
    let stored_before = fs::read_to_string(archive.path()).context("read first progress checkpoint")?;

    let updated_messages = vec![
        SessionMessage::new(MessageRole::User, "initial"),
        SessionMessage::new(MessageRole::User, "new steering intent"),
    ];
    let second_status = archive
        .persist_progress_async_with_status(SessionProgressArgs {
            total_messages: updated_messages.len(),
            distinct_tools: Vec::new(),
            messages: updated_messages.clone(),
            recent_messages: updated_messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: None,
            turn_diagnostics: None,
        })
        .await?;
    assert!(matches!(second_status, SessionProgressPersistenceStatus::Throttled(_)));

    let stored_after = fs::read_to_string(archive.path()).context("read throttled progress checkpoint")?;
    assert_eq!(stored_after, stored_before, "throttling must not acknowledge an unwritten message");
    let snapshot: SessionSnapshot = serde_json::from_str(&stored_after).context("parse progress checkpoint")?;
    assert_eq!(snapshot.messages, initial_messages);
    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn forced_async_progress_persists_latest_message_after_throttle() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new("ForcedWS", "/tmp/forced", "model", "provider", "dark", "medium");
    let archive = SessionArchive::new(metadata, None).await?;
    let initial = vec![SessionMessage::new(MessageRole::User, "initial")];
    archive
        .persist_progress_async_with_status(SessionProgressArgs {
            total_messages: 1,
            distinct_tools: Vec::new(),
            messages: initial.clone(),
            recent_messages: initial.clone(),
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: None,
            turn_diagnostics: None,
        })
        .await?;

    let latest = vec![
        SessionMessage::new(MessageRole::User, "initial"),
        SessionMessage::new(MessageRole::Assistant, "blocked latest response"),
    ];
    let status = archive
        .persist_progress_async_with_status_forced(SessionProgressArgs {
            total_messages: latest.len(),
            distinct_tools: Vec::new(),
            messages: latest.clone(),
            recent_messages: latest,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: None,
            turn_diagnostics: None,
        })
        .await?;

    assert!(status.is_persisted());
    let snapshot: SessionSnapshot = serde_json::from_str(&fs::read_to_string(archive.path())?)?;
    assert_eq!(snapshot.messages[1].content.as_text_borrowed(), Some("blocked latest response"));

    let identifier = archive
        .path()
        .file_stem()
        .and_then(|stem| stem.to_str())
        .context("session archive has no identifier")?;
    let verified_identifier = archive
        .verify_persisted_resume_identifier(archive.path())
        .await?
        .context("forced checkpoint should produce a verified resume identifier")?;
    assert_eq!(verified_identifier.as_str(), identifier);

    let listing = find_session_by_identifier(identifier)
        .await?
        .context("forced checkpoint should be resolvable by its archive identifier")?;
    assert_eq!(listing.path, archive.path());
    assert_eq!(listing.snapshot.messages[1].content.as_text_borrowed(), Some("blocked latest response"));
    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_archive_skips_writes_when_history_persistence_is_disabled() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::None, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("NoHistory", "/tmp/no-history", "model-x", "provider-y", "dark", "medium");
    let archive = SessionArchive::new(metadata, None).await?;
    let path = archive.finalize(
        vec!["line one".to_owned()],
        1,
        Vec::new(),
        vec![SessionMessage::new(MessageRole::User, "hello")],
    )?;

    assert_eq!(path, archive.path());
    assert!(!path.exists(), "history disabled should not write archive files");

    let status = archive
        .persist_progress_async_with_status_forced(SessionProgressArgs {
            total_messages: 1,
            distinct_tools: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "not persisted")],
            recent_messages: vec![SessionMessage::new(MessageRole::User, "not persisted")],
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: None,
            turn_diagnostics: None,
        })
        .await?;
    assert!(matches!(status, SessionProgressPersistenceStatus::Disabled(_)));
    assert!(!archive.path().exists());
    Ok(())
}
#[test]
fn snapshot_compaction_shrinks_large_single_message_payloads() -> Result<()> {
    let snapshot = SessionSnapshot {
        metadata: SessionArchiveMetadata::new("Workspace", "/tmp/workspace", "model", "provider", "dark", "medium"),
        started_at: Utc::now(),
        ended_at: Utc::now(),
        total_messages: 1,
        distinct_tools: vec!["very-large-tool-name".repeat(20)],
        transcript: vec!["transcript ".repeat(400)],
        messages: vec![SessionMessage {
            role: MessageRole::Assistant,
            content: MessageContent::Parts(vec![
                ContentPart::text("text ".repeat(800)),
                ContentPart::image("a".repeat(4000), "image/png".to_string()),
            ]),
            reasoning: Some(Box::new("reasoning ".repeat(300))),
            reasoning_details: None,
            tool_calls: Some(Box::new(vec![ToolCall::function(
                "call_1".to_string(),
                "command_session".to_string(),
                "{\"cmd\":\"echo giant payload\"}".repeat(100),
            )])),
            tool_call_id: None,
            phase: None,
            origin_tool: Some(Box::new("command_session".repeat(50))),
            metadata: None,
            clear_at: None,
        }],
        progress: Some(Box::new(SessionProgress {
            turn_number: 1,
            recent_messages: vec![SessionMessage::new(MessageRole::Assistant, "recent ".repeat(500))],
            tool_summaries: vec!["summary ".repeat(100)],
            token_usage: Some("token ".repeat(200)),
            max_context_tokens: Some(128_000),
            loaded_skills: vec!["skill ".repeat(50)],
            turn_diagnostics: None,
        })),
        error_logs: vec![ErrorLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level: "ERROR".to_string(),
            target: "vtcode_test".to_string(),
            message: "error ".repeat(400),
        }],
    };

    let compacted = compact_snapshot_to_max_bytes(snapshot, 2_048)?;

    assert!(serde_json::to_vec(&compacted)?.len() <= 2_048);
    assert_eq!(compacted.messages.len(), 1);
    Ok(())
}
