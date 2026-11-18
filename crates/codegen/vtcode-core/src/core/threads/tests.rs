use super::*;
use crate::exec::events::{ThreadEvent, ThreadStartedEvent};
use crate::llm::provider::MessageRole;
use crate::utils::session_archive::{
    SessionArchiveMetadata, SessionMessage, SessionProgress, SessionSnapshot, clear_sessions_dir_override_for_tests,
    override_sessions_dir_for_tests,
};
use chrono::Utc;
use std::sync::{LazyLock, Mutex};
use tempfile::TempDir;

static SESSION_DIR_TEST_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Seed a source archive so the fork has a real listing to copy.
async fn seed_source_listing(sessions_dir: &Path, workspace: &Path) -> SessionListing {
    let metadata = SessionArchiveMetadata::new(
        "workspace",
        workspace.display().to_string(),
        "model",
        "provider",
        "theme",
        "medium",
    );
    let messages = vec![SessionMessage::new(MessageRole::User, "hello")];
    let snapshot = SessionSnapshot {
        metadata,
        started_at: Utc::now(),
        ended_at: Utc::now(),
        total_messages: messages.len(),
        distinct_tools: Vec::new(),
        transcript: Vec::new(),
        messages: messages.clone(),
        progress: Some(Box::new(SessionProgress {
            recent_messages: messages,
            loaded_skills: vec!["repo-skill".to_string()],
            turn_number: 3,
            ..Default::default()
        })),
        error_logs: Vec::new(),
    };
    let path = sessions_dir.join("session-source.json");
    std::fs::write(&path, serde_json::to_vec(&snapshot).expect("serialize snapshot")).expect("write source archive");
    SessionListing { path, snapshot }
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn fork_thread_for_handoff_writes_a_discoverable_copy_with_its_own_id() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    let workspace = sessions_dir.path().join("workspace");
    let listing = seed_source_listing(sessions_dir.path(), &workspace).await;

    let (fork_id, _archive) = fork_thread_for_handoff(&listing, workspace.clone(), listing.snapshot.metadata.clone())
        .await
        .expect("fork thread");

    // The fork must be discoverable by id: the receiver resolves the
    // handoff through `find_session_by_identifier`, not through the
    // in-memory archive handle.
    let found = find_session_by_identifier(&fork_id)
        .await
        .expect("find fork")
        .expect("fork exists");
    assert_eq!(found.identifier(), fork_id);
    assert_ne!(fork_id, listing.identifier(), "the fork owns a new id");
    assert_eq!(found.snapshot.metadata.parent_session_id.as_deref(), Some("session-source"));
    assert_eq!(found.snapshot.metadata.fork_mode, Some(SessionForkMode::FullCopy));
    assert_eq!(found.snapshot.messages.len(), 1, "the fork carries the source transcript");
    // The progress block carries the loaded skills and turn number the
    // receiver restores; the fork must keep it, not just the metadata
    // fallback.
    let progress = found.snapshot.progress.as_ref().expect("the fork keeps the source progress");
    assert_eq!(progress.loaded_skills, vec!["repo-skill".to_string()]);
    assert_eq!(progress.turn_number, 3);

    clear_sessions_dir_override_for_tests();
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn fork_thread_for_handoff_copies_the_source_transcript() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    let workspace = sessions_dir.path().join("workspace");
    let listing = seed_source_listing(sessions_dir.path(), &workspace).await;

    let (fork_id, _archive) = fork_thread_for_handoff(&listing, workspace.clone(), listing.snapshot.metadata.clone())
        .await
        .expect("fork thread");

    let found = find_session_by_identifier(&fork_id)
        .await
        .expect("find fork")
        .expect("fork exists");
    let messages = messages_from_session_listing(&found);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_text(), "hello");

    clear_sessions_dir_override_for_tests();
}

#[test]
fn event_store_evicts_old_records() {
    let manager = ThreadManager::with_event_buffer_capacity(2);
    let handle = manager.start_thread_with_identifier("thread-1", ThreadBootstrap::new(None));

    handle.record_event(
        None,
        None,
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread-1".to_string() }),
    );
    handle.record_event(
        None,
        Some("turn-1".to_string()),
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread-1-turn-1".to_string() }),
    );
    handle.record_event(
        None,
        Some("turn-2".to_string()),
        ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread-1-turn-2".to_string() }),
    );

    let records = handle.replay_recent();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].sequence, 1);
    assert_eq!(records[1].sequence, 2);
}

#[test]
fn start_thread_with_identifier_preserves_message_history() {
    let manager = ThreadManager::new();
    let bootstrap = ThreadBootstrap::new(None)
        .with_messages(vec![Message::user("hello".to_string())])
        .with_loaded_skills(vec!["repo-skill".to_string()]);
    let handle = manager.start_thread_with_identifier("thread-123", bootstrap);

    assert_eq!(handle.thread_id().as_str(), "thread-123");
    let snapshot = handle.snapshot();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.loaded_skills, vec!["repo-skill".to_string()]);
}

#[test]
fn submit_enforces_single_in_flight_turn() {
    let manager = ThreadManager::new();
    let handle = manager.start_thread_with_identifier("thread-123", ThreadBootstrap::new(None));

    let _first = handle.begin_turn().expect("first turn");
    let err = handle.begin_turn().expect_err("second turn should fail");
    assert!(err.to_string().contains("in-flight turn"));
    handle.finish_turn();
    handle.begin_turn().expect("turn after finish");
}

#[test]
#[serial_test::serial(session_dir_override)]
fn list_recent_sessions_in_scope_filters_by_workspace() {
    let _guard = SESSION_DIR_TEST_GUARD.lock().expect("session dir test guard");

    let tmp = TempDir::new().expect("temp dir");
    override_sessions_dir_for_tests(tmp.path());

    let listing = SessionListing {
        path: tmp.path().join("session-alpha.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "model",
                "provider",
                "theme",
                "medium",
            ),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "hello")],
            progress: None,
            error_logs: Vec::new(),
        },
    };
    std::fs::write(&listing.path, serde_json::to_string(&listing.snapshot).expect("serialize snapshot"))
        .expect("write listing");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let filtered = runtime
        .block_on(list_recent_sessions_in_scope(5, &SessionQueryScope::CurrentWorkspace(tmp.path().join("workspace"))))
        .expect("filter by workspace");
    let all = runtime
        .block_on(list_recent_sessions_in_scope(5, &SessionQueryScope::All))
        .expect("list all");

    clear_sessions_dir_override_for_tests();

    assert_eq!(filtered.len(), 1);
    assert_eq!(all.len(), 1);
}

#[test]
#[serial_test::serial(session_dir_override)]
fn prepare_archived_session_resume_reuses_source_identifier_and_archive() {
    let _guard = SESSION_DIR_TEST_GUARD.lock().expect("session dir test guard");

    let tmp = TempDir::new().expect("temp dir");
    override_sessions_dir_for_tests(tmp.path());

    let listing = SessionListing {
        path: tmp.path().join("session-source.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "old-model",
                "old-provider",
                "old-theme",
                "medium",
            )
            .with_primary_agent("plan"),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 2,
            distinct_tools: vec!["tool_a".to_string()],
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "hello")],
            progress: Some(Box::new(SessionProgress {
                turn_number: 1,
                recent_messages: vec![SessionMessage::new(MessageRole::Assistant, "recent")],
                tool_summaries: Vec::new(),
                token_usage: None,
                max_context_tokens: None,
                loaded_skills: vec!["skill_a".to_string()],
                turn_diagnostics: None,
            })),
            error_logs: Vec::new(),
        },
    };

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let prepared = runtime
        .block_on(prepare_archived_session(
            listing.clone(),
            tmp.path().join("workspace"),
            SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "new-model",
                "new-provider",
                "new-theme",
                "high",
            ),
            ArchivedSessionIntent::ResumeInPlace,
            Some("should-not-be-used".to_string()),
        ))
        .expect("prepare resume");

    clear_sessions_dir_override_for_tests();

    assert_eq!(prepared.thread_id, listing.identifier());
    assert_eq!(prepared.archive.path(), listing.path.as_path());
    assert_eq!(prepared.bootstrap.messages[0].content.as_text(), "hello");
    assert_eq!(prepared.bootstrap.loaded_skills, vec!["skill_a".to_string()]);
    assert_eq!(prepared.bootstrap.metadata.as_ref().expect("metadata").model, "new-model");
    // Resume preserves the source session's primary agent ("mode").
    assert_eq!(prepared.bootstrap.metadata.as_ref().expect("metadata").primary_agent.as_deref(), Some("plan"));
}

#[test]
#[serial_test::serial(session_dir_override)]
fn prepare_archived_session_fork_uses_new_identifier_and_preserves_history() {
    let _guard = SESSION_DIR_TEST_GUARD.lock().expect("session dir test guard");

    let tmp = TempDir::new().expect("temp dir");
    override_sessions_dir_for_tests(tmp.path());

    let listing = SessionListing {
        path: tmp.path().join("session-source.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "model",
                "provider",
                "theme",
                "medium",
            ),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "hello")],
            progress: None,
            error_logs: Vec::new(),
        },
    };

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let prepared = runtime
        .block_on(prepare_archived_session(
            listing.clone(),
            tmp.path().join("workspace"),
            SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "model",
                "provider",
                "theme",
                "medium",
            ),
            ArchivedSessionIntent::ForkNewArchive {
                custom_suffix: Some("branch".to_string()),
                summarize: false,
            },
            Some("session-forked".to_string()),
        ))
        .expect("prepare fork");

    clear_sessions_dir_override_for_tests();

    assert_eq!(prepared.thread_id, "session-forked");
    assert_ne!(prepared.archive.path(), listing.path.as_path());
    assert!(prepared.archive.path().ends_with(Path::new("session-forked.json")));
    assert_eq!(prepared.bootstrap.messages[0].content.as_text(), "hello");
    assert_eq!(
        prepared
            .bootstrap
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.parent_session_id.as_deref()),
        Some("session-source")
    );
    assert_eq!(
        prepared.bootstrap.metadata.as_ref().and_then(|metadata| metadata.fork_mode),
        Some(SessionForkMode::FullCopy)
    );
}

#[test]
#[serial_test::serial(session_dir_override)]
fn prepare_archived_session_preserves_prompt_cache_lineage_when_compatible() {
    let _guard = SESSION_DIR_TEST_GUARD.lock().expect("session dir test guard");

    let tmp = TempDir::new().expect("temp dir");
    override_sessions_dir_for_tests(tmp.path());

    let listing = SessionListing {
        path: tmp.path().join("session-source.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "model",
                "provider",
                "theme",
                "medium",
            )
            .with_prompt_cache_lineage_id("lineage-source"),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "hello")],
            progress: None,
            error_logs: Vec::new(),
        },
    };

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let prepared = runtime
        .block_on(prepare_archived_session(
            listing,
            tmp.path().join("workspace"),
            SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "model",
                "provider",
                "theme",
                "medium",
            )
            .with_prompt_cache_lineage_id("lineage-new"),
            ArchivedSessionIntent::ResumeInPlace,
            None,
        ))
        .expect("prepare resume");

    clear_sessions_dir_override_for_tests();

    assert_eq!(
        prepared
            .bootstrap
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.prompt_cache_lineage_id.as_deref()),
        Some("lineage-source")
    );
}

#[test]
#[serial_test::serial(session_dir_override)]
fn prepare_archived_session_resets_prompt_cache_lineage_on_model_change() {
    let _guard = SESSION_DIR_TEST_GUARD.lock().expect("session dir test guard");

    let tmp = TempDir::new().expect("temp dir");
    override_sessions_dir_for_tests(tmp.path());

    let listing = SessionListing {
        path: tmp.path().join("session-source.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "model-a",
                "provider",
                "theme",
                "medium",
            )
            .with_prompt_cache_lineage_id("lineage-source"),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "hello")],
            progress: None,
            error_logs: Vec::new(),
        },
    };

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let prepared = runtime
        .block_on(prepare_archived_session(
            listing,
            tmp.path().join("workspace"),
            SessionArchiveMetadata::new(
                "ws",
                tmp.path().join("workspace").display().to_string(),
                "model-b",
                "provider",
                "theme",
                "medium",
            )
            .with_prompt_cache_lineage_id("lineage-new"),
            ArchivedSessionIntent::ResumeInPlace,
            None,
        ))
        .expect("prepare resume");

    clear_sessions_dir_override_for_tests();

    assert_eq!(
        prepared
            .bootstrap
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.prompt_cache_lineage_id.as_deref()),
        Some("lineage-new")
    );
}

#[test]
fn messages_from_session_listing_preserves_assistant_phases_from_progress() {
    let listing = SessionListing {
        path: PathBuf::from("session.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new("ws", "/tmp/ws", "gpt-5.6-sol", "openai", "theme", "medium"),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 2,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: Vec::new(),
            progress: Some(Box::new(SessionProgress {
                turn_number: 2,
                recent_messages: vec![
                    SessionMessage::from(
                        &Message::assistant("Working".to_string())
                            .with_phase(Some(crate::llm::provider::AssistantPhase::Commentary)),
                    ),
                    SessionMessage::from(
                        &Message::assistant("Done".to_string())
                            .with_phase(Some(crate::llm::provider::AssistantPhase::FinalAnswer)),
                    ),
                ],
                tool_summaries: Vec::new(),
                token_usage: None,
                max_context_tokens: None,
                loaded_skills: Vec::new(),
                turn_diagnostics: None,
            })),
            error_logs: Vec::new(),
        },
    };

    let messages = messages_from_session_listing(&listing);
    assert_eq!(
        messages.iter().map(|message| message.phase).collect::<Vec<_>>(),
        vec![
            Some(crate::llm::provider::AssistantPhase::Commentary),
            Some(crate::llm::provider::AssistantPhase::FinalAnswer),
        ]
    );
}
