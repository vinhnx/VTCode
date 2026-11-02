use super::*;

#[test]
fn internal_session_context_keeps_ordinary_messages_visible() {
    use crate::utils::session_archive::{COLLAPSED_TOOL_OUTPUT_NOTICE, is_internal_session_context};
    assert!(is_internal_session_context(MessageRole::System, COLLAPSED_TOOL_OUTPUT_NOTICE, None));
    assert!(is_internal_session_context(
        MessageRole::System,
        "private guidance",
        Some(MessageClearAt::NextUserMessage)
    ));
    assert!(!is_internal_session_context(MessageRole::User, COLLAPSED_TOOL_OUTPUT_NOTICE, None));
    assert!(!is_internal_session_context(MessageRole::System, "User-visible system notice", None));
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_archive_persists_snapshot() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium")
            .with_external_thread_id("thread-123");
    let archive = SessionArchive::new(metadata.clone(), None).await?;
    let transcript = vec!["line one".to_owned(), "line two".to_owned()];
    let messages = vec![
        SessionMessage::new(MessageRole::User, "Hello world"),
        SessionMessage::new(MessageRole::Assistant, "Hi there"),
    ];
    let path = archive.finalize(transcript.clone(), 4, vec!["tool_a".to_owned()], messages.clone())?;

    let stored =
        fs::read_to_string(&path).with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(snapshot.metadata, metadata);
    assert_eq!(snapshot.transcript, transcript);
    assert_eq!(snapshot.total_messages, 4);
    assert_eq!(snapshot.distinct_tools, vec!["tool_a".to_owned()]);
    assert_eq!(snapshot.messages, messages);
    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_archive_finalization_preserves_last_turn_diagnostics() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium");
    let archive = SessionArchive::new(metadata, None).await?;
    let diagnostics = SnapshotTurnDiagnostics {
        elapsed_ms: 321,
        requested_tool_calls: 4,
        admitted_tool_calls: 2,
        unadmitted_tool_calls: 2,
        ..Default::default()
    };

    let path = archive.finalize_with_diagnostics(Vec::new(), 0, Vec::new(), Vec::new(), Some(diagnostics.clone()))?;
    let stored = fs::read_to_string(&path).context("failed to read stored session")?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(
        snapshot
            .progress
            .as_deref()
            .and_then(|progress| progress.turn_diagnostics.as_ref()),
        Some(&diagnostics)
    );
    assert!(
        snapshot
            .progress
            .as_deref()
            .is_some_and(|progress| progress.recent_messages.is_empty())
    );
    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_archive_persists_budget_limit_continuation_metadata() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium")
            .with_continuation_metadata(Some(SessionContinuationMetadata::budget_limit(2.5, 2.7, true)));
    let archive = SessionArchive::new(metadata.clone(), None).await?;

    let path = archive.finalize(Vec::new(), 0, Vec::new(), Vec::new())?;
    let stored =
        fs::read_to_string(&path).with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    let continuation = snapshot
        .metadata
        .budget_limit_continuation()
        .expect("budget limit continuation metadata should exist");
    assert_eq!(continuation.max_budget_usd(), Some(2.5));
    assert_eq!(continuation.actual_cost_usd(), Some(2.7));
    assert!(continuation.summary_available);
    assert_eq!(snapshot.metadata, metadata);
    Ok(())
}
