use super::*;
use crate::config::constants::tools as tool_names;
use crate::llm::provider::{ContentPart, ToolCall};
use anyhow::anyhow;
use chrono::{TimeZone, Timelike};
use std::mem::size_of;
use std::sync::LazyLock;
use std::time::Duration;

static SESSION_HISTORY_TEST_LOCK: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

struct EnvGuard {
    key: &'static str,
}

impl EnvGuard {
    fn set(key: &'static str, value: &Path) -> Self {
        set_test_env_override_path(key, value);
        Self { key }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        clear_test_env_override(self.key);
    }
}

struct HistorySettingsGuard {
    previous: SessionHistorySettings,
}

impl HistorySettingsGuard {
    fn set(persistence: HistoryPersistence, max_bytes: Option<usize>) -> Self {
        let previous = session_history_settings();
        let mut config = VTCodeConfig::default();
        config.history.persistence = persistence;
        config.history.max_bytes = max_bytes;
        apply_session_history_config_from_vtcode(&config);
        Self { previous }
    }
}

impl Drop for HistorySettingsGuard {
    fn drop(&mut self) {
        let mut config = VTCodeConfig::default();
        config.history.persistence = self.previous.persistence;
        config.history.max_bytes = self.previous.max_bytes;
        apply_session_history_config_from_vtcode(&config);
    }
}

async fn lock_history_test_guard() -> tokio::sync::MutexGuard<'static, ()> {
    SESSION_HISTORY_TEST_LOCK.lock().await
}

#[tokio::test]
async fn session_archive_persists_snapshot() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    )
    .with_external_thread_id("thread-123");
    let archive = SessionArchive::new(metadata.clone(), None).await?;
    let transcript = vec!["line one".to_owned(), "line two".to_owned()];
    let messages = vec![
        SessionMessage::new(MessageRole::User, "Hello world"),
        SessionMessage::new(MessageRole::Assistant, "Hi there"),
    ];
    let path = archive.finalize(
        transcript.clone(),
        4,
        vec!["tool_a".to_owned()],
        messages.clone(),
    )?;

    let stored = fs::read_to_string(&path)
        .with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot =
        serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(snapshot.metadata, metadata);
    assert_eq!(snapshot.transcript, transcript);
    assert_eq!(snapshot.total_messages, 4);
    assert_eq!(snapshot.distinct_tools, vec!["tool_a".to_owned()]);
    assert_eq!(snapshot.messages, messages);
    Ok(())
}

#[tokio::test]
async fn session_archive_persists_budget_limit_continuation_metadata() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    )
    .with_continuation_metadata(Some(SessionContinuationMetadata::budget_limit(
        2.5, 2.7, true,
    )));
    let archive = SessionArchive::new(metadata.clone(), None).await?;

    let path = archive.finalize(Vec::new(), 0, Vec::new(), Vec::new())?;
    let stored = fs::read_to_string(&path)
        .with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot =
        serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

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

#[test]
fn session_message_converts_back_and_forth() {
    let mut original = Message::assistant("Test response".to_owned());
    original.reasoning = Some("Model thoughts".to_owned());
    original.phase = Some(AssistantPhase::FinalAnswer);
    let stored = SessionMessage::from(&original);
    let restored = Message::from(&stored);

    assert_eq!(original.role, restored.role);
    assert_eq!(original.content, restored.content);
    assert_eq!(
        original.reasoning.as_deref(),
        stored.reasoning.as_deref().map(|v| v.as_str())
    );
    assert_eq!(original.reasoning, restored.reasoning);
    assert_eq!(original.tool_call_id, restored.tool_call_id);
    assert_eq!(original.phase, stored.phase);
    assert_eq!(original.phase, restored.phase);
}

#[test]
fn session_message_roundtrip_preserves_commentary_phase() {
    let original =
        Message::assistant("Working".to_owned()).with_phase(Some(AssistantPhase::Commentary));
    let stored = SessionMessage::from(&original);
    let restored = Message::from(&stored);

    assert_eq!(stored.phase, Some(AssistantPhase::Commentary));
    assert_eq!(restored.phase, Some(AssistantPhase::Commentary));
}

#[test]
fn session_message_preserves_tool_calls_reasoning_details_and_origin_tool() {
    let mut original = Message::assistant("Calling a tool".to_owned());
    original.reasoning_details = Some(vec![serde_json::json!({
        "summary": "tool call planning"
    })]);
    original.tool_calls = Some(vec![ToolCall::function(
        "call_1".to_string(),
        "unified_exec".to_string(),
        "{\"cmd\":\"cargo fmt\"}".to_string(),
    )]);
    original.origin_tool = Some("unified_exec".to_string());

    let stored = SessionMessage::from(&original);
    let restored = Message::from(&stored);

    assert_eq!(
        stored.reasoning_details.as_deref(),
        original.reasoning_details.as_ref()
    );
    assert_eq!(stored.tool_calls.as_deref(), original.tool_calls.as_ref());
    assert_eq!(
        stored.origin_tool.as_deref().map(|v| v.as_str()),
        original.origin_tool.as_deref()
    );
    assert_eq!(restored.reasoning_details, original.reasoning_details);
    assert_eq!(restored.tool_calls, original.tool_calls);
    assert_eq!(restored.origin_tool, original.origin_tool);
}

#[test]
fn session_message_discards_empty_sparse_metadata() {
    let mut original = Message::assistant("Calling a tool".to_owned());
    original.reasoning = Some(String::new());
    original.reasoning_details = Some(Vec::new());
    original.tool_calls = Some(Vec::new());
    original.tool_call_id = Some(String::new());
    original.origin_tool = Some(String::new());

    let stored = SessionMessage::from(&original);
    assert!(stored.reasoning.is_none());
    assert!(stored.reasoning_details.is_none());
    assert!(stored.tool_calls.is_none());
    assert!(stored.tool_call_id.is_none());
    assert!(stored.origin_tool.is_none());

    let restored = Message::from(&stored);
    assert!(restored.reasoning.is_none());
    assert!(restored.reasoning_details.is_none());
    assert!(restored.tool_calls.is_none());
    assert!(restored.tool_call_id.is_none());
    assert!(restored.origin_tool.is_none());
}

#[test]
fn session_message_deserialize_discards_empty_sparse_metadata() {
    let payload = serde_json::json!({
        "role": "Assistant",
        "content": "done",
        "reasoning": "",
        "reasoning_details": [],
        "tool_calls": [],
        "tool_call_id": "",
        "origin_tool": "",
    });

    let message: SessionMessage = serde_json::from_value(payload).expect("session message");
    assert!(message.reasoning.is_none());
    assert!(message.reasoning_details.is_none());
    assert!(message.tool_calls.is_none());
    assert!(message.tool_call_id.is_none());
    assert!(message.origin_tool.is_none());
}

#[test]
fn session_message_preserves_parts() {
    let original = Message::assistant_with_parts(vec![
        ContentPart::text("See attached image".to_owned()),
        ContentPart::text("See attached image".to_owned()),
        ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
        ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
        ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
    ]);
    let stored = SessionMessage::from(&original);

    assert_eq!(stored.content, original.content);

    let restored = Message::from(&stored);
    assert_eq!(restored.content, original.content);
}

#[test]
fn session_message_layout_is_smaller_than_unboxed_equivalent() {
    #[derive(Debug)]
    #[allow(dead_code)]
    struct UnboxedSessionMessageLike(
        MessageRole,
        MessageContent,
        Option<String>,
        Option<Vec<serde_json::Value>>,
        Option<Vec<ToolCall>>,
        Option<String>,
        Option<AssistantPhase>,
        Option<String>,
    );

    assert!(size_of::<SessionMessage>() < size_of::<UnboxedSessionMessageLike>());
}

#[test]
fn boxed_session_progress_option_is_pointer_sized() {
    assert_eq!(
        size_of::<Option<Box<SessionProgress>>>(),
        size_of::<usize>()
    );
    assert!(size_of::<Option<SessionProgress>>() > size_of::<Option<Box<SessionProgress>>>());
}

#[tokio::test]
async fn session_progress_persists_budget_and_recent_messages() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    );
    let archive = SessionArchive::new(metadata, None).await?;
    let recent = vec![SessionMessage::new(MessageRole::Assistant, "recent")];

    let path = archive.persist_progress(SessionProgressArgs {
        total_messages: 1,
        distinct_tools: vec!["tool_a".to_owned()],
        recent_messages: recent.clone(),
        turn_number: 2,
        token_usage: Some("10 tokens".to_string()),
        max_context_tokens: Some(128),
        loaded_skills: None,
    })?;

    let stored = fs::read_to_string(&path)
        .with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot =
        serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    let progress = snapshot.progress.expect("progress should exist");
    assert_eq!(progress.turn_number, 2);
    assert_eq!(progress.recent_messages, recent);
    assert_eq!(progress.token_usage, Some("10 tokens".to_string()));
    assert_eq!(progress.tool_summaries, vec!["tool_a".to_string()]);
    assert_eq!(progress.max_context_tokens, Some(128));
    assert_eq!(snapshot.transcript, vec!["recent".to_string()]);
    Ok(())
}

#[tokio::test]
async fn session_progress_transcript_skips_tool_noise_and_duplicates() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    );
    let archive = SessionArchive::new(metadata, None).await?;
    let mut assistant = SessionMessage::new(MessageRole::Assistant, "done");
    assistant.reasoning = Some(Box::new("reasoned".to_string()));
    let recent = vec![
        SessionMessage::new(MessageRole::User, "run cargo check"),
        SessionMessage::new(MessageRole::Tool, "{\"output\":\"...\"}"),
        SessionMessage::new(MessageRole::Assistant, "done"),
        assistant,
    ];

    let path = archive.persist_progress(SessionProgressArgs {
        total_messages: recent.len(),
        distinct_tools: vec!["unified_exec".to_owned()],
        recent_messages: recent,
        turn_number: 2,
        token_usage: Some("10 tokens".to_string()),
        max_context_tokens: Some(128),
        loaded_skills: None,
    })?;

    let stored = fs::read_to_string(&path)
        .with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot =
        serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(
        snapshot.transcript,
        vec!["run cargo check".to_string(), "done".to_string()]
    );
    Ok(())
}

#[test]
fn archive_transcript_cleaner_filters_recovery_noise_and_duplicate_tool_blocks() {
    let lines = vec![
        "hello".to_string(),
        "  Hello, Vinh.".to_string(),
        "tell me more".to_string(),
        "  Let me dig deeper into the project structure.".to_string(),
        "• Ran cd /tmp/project &&".to_string(),
        "  │ ls -1 src/".to_string(),
        "    ✓ exit 0".to_string(),
        "• Ran cd /tmp/project &&".to_string(),
        "  │ ls -1 src/".to_string(),
        "    ✓ exit 0".to_string(),
        "• List files Use Unified search".to_string(),
        "  └ Action: list".to_string(),
        "  └ Path: /tmp/project/src".to_string(),
        "  └ Filter: files".to_string(),
        "[!] Turn balancer: repeated low-signal calls detected; scheduling a final recovery pass."
            .to_string(),
        "  I couldn't produce a final synthesis because the model returned no answer on the recovery pass."
            .to_string(),
        "  Latest tool output: {\"output\":\"...\"}".to_string(),
        "  Reuse the latest tool outputs already collected in this turn before retrying."
            .to_string(),
        "run cargo fmt and report me".to_string(),
        "• Ran cd /tmp/project &&".to_string(),
        "  │ cargo fmt 2>&1".to_string(),
        "    (no output)".to_string(),
        "  cargo fmt ran successfully with no output.".to_string(),
    ];

    let cleaned = clean_transcript_lines(&lines);

    assert_eq!(
        cleaned,
        vec![
            "hello".to_string(),
            "  Hello, Vinh.".to_string(),
            "tell me more".to_string(),
            "  Let me dig deeper into the project structure.".to_string(),
            "• Ran cd /tmp/project && ls -1 src/ (repeated x2)".to_string(),
            "• List files Use Unified search [path /tmp/project/src, filter files]".to_string(),
            "Repeated low-signal tool churn triggered recovery.".to_string(),
            "Recovery pass failed to produce a final synthesis.".to_string(),
            "run cargo fmt and report me".to_string(),
            "• Ran cd /tmp/project && cargo fmt 2>&1".to_string(),
            "  cargo fmt ran successfully with no output.".to_string(),
        ]
    );
}

#[test]
fn archive_transcript_cleaner_preserves_paragraph_spacing_and_drops_structured_result_noise() {
    let lines = vec![
        "Project summary:".to_string(),
        "".to_string(),
        "  VT Code is a Rust-based coding agent.".to_string(),
        "".to_string(),
        "Structured result with fields: output, exit_code, wall_time, session_id".to_string(),
        "Next step.".to_string(),
    ];

    let cleaned = clean_transcript_lines(&lines);

    assert_eq!(
        cleaned,
        vec![
            "Project summary:".to_string(),
            "".to_string(),
            "  VT Code is a Rust-based coding agent.".to_string(),
            "".to_string(),
            "Next step.".to_string(),
        ]
    );
}

#[tokio::test]
async fn session_progress_normalizes_exec_tool_aliases_in_summaries() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    );
    let archive = SessionArchive::new(metadata, None).await?;
    let recent = vec![SessionMessage::new(MessageRole::Assistant, "done")];

    let path = archive.persist_progress(SessionProgressArgs {
        total_messages: 1,
        distinct_tools: vec![
            tool_names::UNIFIED_EXEC.to_string(),
            tool_names::RUN_PTY_CMD.to_string(),
            tool_names::SEND_PTY_INPUT.to_string(),
            tool_names::READ_PTY_SESSION.to_string(),
            tool_names::LIST_PTY_SESSIONS.to_string(),
            tool_names::CLOSE_PTY_SESSION.to_string(),
            tool_names::EXECUTE_CODE.to_string(),
            tool_names::EXEC_COMMAND.to_string(),
            tool_names::WRITE_STDIN.to_string(),
            "shell".to_string(),
            "exec_pty_cmd".to_string(),
            "exec".to_string(),
            "container.exec".to_string(),
        ],
        recent_messages: recent,
        turn_number: 2,
        token_usage: Some("10 tokens".to_string()),
        max_context_tokens: Some(128),
        loaded_skills: None,
    })?;

    let stored = fs::read_to_string(&path)
        .with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot =
        serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(
        snapshot.distinct_tools,
        vec![tool_names::UNIFIED_EXEC.to_string()]
    );
    let progress = snapshot.progress.expect("progress should exist");
    assert_eq!(
        progress.tool_summaries,
        vec![tool_names::UNIFIED_EXEC.to_string()]
    );
    Ok(())
}

#[tokio::test]
async fn find_session_by_identifier_returns_match() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "Sample",
        "/tmp/sample",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    );
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

#[tokio::test]
async fn session_archive_path_collision_adds_suffix() -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    );

    let started_at = Utc
        .with_ymd_and_hms(2025, 9, 25, 10, 15, 30)
        .unwrap()
        .with_nanosecond(123_456_000)
        .unwrap();

    let first_path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);
    fs::write(&first_path, "{}").context("failed to create sentinel file")?;

    let second_path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);

    assert_ne!(first_path, second_path);
    let second_name = second_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("file name");
    assert!(second_name.contains("-01"));

    Ok(())
}

#[test]
fn session_archive_filename_includes_microseconds_and_pid() -> Result<()> {
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    );

    let started_at = Utc
        .with_ymd_and_hms(2025, 9, 25, 10, 15, 30)
        .unwrap()
        .with_nanosecond(654_321_000)
        .expect("nanosecond set");

    let path = generate_unique_archive_path(temp_dir.path(), &metadata, started_at, None);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .expect("file name string");

    assert!(name.contains("20250925T101530Z_654321"));
    let pid_fragment = format!("{:05}", process::id());
    assert!(name.contains(&pid_fragment));

    Ok(())
}

#[tokio::test]
async fn reserve_session_identifier_can_be_reused_for_archive() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let session_id = reserve_session_archive_identifier("ExampleWorkspace", None).await?;
    assert!(session_id.starts_with("session-exampleworkspace-"));

    let metadata = SessionArchiveMetadata::new(
        "ExampleWorkspace",
        "/tmp/example",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    )
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
    let stored = fs::read_to_string(&path)
        .with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot =
        serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("missing file stem"))?;
    assert_eq!(stem, session_id);
    assert_eq!(
        snapshot.metadata.debug_log_path,
        Some("/tmp/debug-session.log".to_string())
    );

    Ok(())
}

#[test]
fn generated_session_identifier_includes_workspace_and_custom_suffix() {
    let generated =
        generate_session_archive_identifier("Example Workspace", Some("branch".to_string()));

    assert!(generated.starts_with("session-example-workspace-"));
    assert!(generated.ends_with("-branch"));
}

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
    let snapshot: SessionSnapshot =
        serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;
    assert_eq!(snapshot.total_messages, 2);
    assert_eq!(snapshot.messages.len(), 2);

    Ok(())
}

#[tokio::test]
async fn list_recent_sessions_orders_entries() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let first_metadata = SessionArchiveMetadata::new(
        "First",
        "/tmp/first",
        "model-a",
        "provider-a",
        "light",
        "medium",
    );
    let first_archive = SessionArchive::new(first_metadata.clone(), None).await?;
    first_archive.finalize(
        vec!["first".to_owned()],
        1,
        Vec::new(),
        vec![SessionMessage::new(MessageRole::User, "First")],
    )?;

    tokio::time::sleep(Duration::from_millis(10)).await;

    let second_metadata = SessionArchiveMetadata::new(
        "Second",
        "/tmp/second",
        "model-b",
        "provider-b",
        "dark",
        "high",
    );
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
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_string())
        })
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
        fs::write(&path, "x".repeat(800_000))
            .with_context(|| format!("failed to write {}", path.display()))?;
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
    let remaining_name = remaining[0]
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    assert_eq!(remaining_name, "session-1.json");
    Ok(())
}

#[test]
fn listing_previews_return_first_non_empty_lines() {
    let metadata = SessionArchiveMetadata::new(
        "Workspace",
        "/tmp/ws",
        "model",
        "provider",
        "dark",
        "medium",
    );
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

    assert_eq!(
        listing.first_prompt_preview(),
        Some("prompt line".to_owned())
    );
    let expected = truncate_preview(&long_response, 80);
    assert_eq!(listing.first_reply_preview(), Some(expected));
}

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
        SessionMessage::new(
            MessageRole::Assistant,
            "The secret key is defined in .env.local",
        ),
    ];
    archive.finalize(vec![], 2, vec![], messages)?;

    let results = search_sessions("SECRET KEY", 10, 5).await?;
    assert!(!results.is_empty());
    assert!(
        results[0]
            .content_snippet
            .to_lowercase()
            .contains("secret key")
    );
    assert_eq!(results[0].role, MessageRole::Assistant);

    Ok(())
}

#[tokio::test]
async fn session_archive_skips_writes_when_history_persistence_is_disabled() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::None, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata = SessionArchiveMetadata::new(
        "NoHistory",
        "/tmp/no-history",
        "model-x",
        "provider-y",
        "dark",
        "medium",
    );
    let archive = SessionArchive::new(metadata, None).await?;
    let path = archive.finalize(
        vec!["line one".to_owned()],
        1,
        Vec::new(),
        vec![SessionMessage::new(MessageRole::User, "hello")],
    )?;

    assert_eq!(path, archive.path());
    assert!(
        !path.exists(),
        "history disabled should not write archive files"
    );
    Ok(())
}

#[test]
fn snapshot_compaction_shrinks_large_single_message_payloads() -> Result<()> {
    let snapshot = SessionSnapshot {
        metadata: SessionArchiveMetadata::new(
            "Workspace",
            "/tmp/workspace",
            "model",
            "provider",
            "dark",
            "medium",
        ),
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
                "unified_exec".to_string(),
                "{\"cmd\":\"echo giant payload\"}".repeat(100),
            )])),
            tool_call_id: None,
            phase: None,
            origin_tool: Some(Box::new("unified_exec".repeat(50))),
        }],
        progress: Some(Box::new(SessionProgress {
            turn_number: 1,
            recent_messages: vec![SessionMessage::new(
                MessageRole::Assistant,
                "recent ".repeat(500),
            )],
            tool_summaries: vec!["summary ".repeat(100)],
            token_usage: Some("token ".repeat(200)),
            max_context_tokens: Some(128_000),
            loaded_skills: vec!["skill ".repeat(50)],
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
