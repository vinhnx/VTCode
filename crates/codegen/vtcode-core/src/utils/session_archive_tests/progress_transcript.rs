use super::*;

#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_progress_persists_budget_and_recent_messages() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium");
    let archive = SessionArchive::new(metadata, None).await?;
    let full = vec![
        SessionMessage::new(MessageRole::User, "first"),
        SessionMessage::new(MessageRole::Assistant, "middle"),
        SessionMessage::new(MessageRole::User, "latest"),
    ];
    let recent = vec![SessionMessage::new(MessageRole::Assistant, "recent")];
    let diagnostics = SnapshotTurnDiagnostics {
        requested_tool_calls: 3,
        denied_tool_calls: 1,
        suppressed_tool_previews: 2,
        model_visible_tool_preview_budget_exhausted: true,
        ..Default::default()
    };

    let path = archive.persist_progress(SessionProgressArgs {
        total_messages: full.len(),
        distinct_tools: vec!["tool_a".to_owned()],
        messages: full.clone(),
        recent_messages: recent.clone(),
        turn_number: 2,
        token_usage: Some("10 tokens".to_string()),
        max_context_tokens: Some(128),
        loaded_skills: None,
        turn_diagnostics: Some(diagnostics.clone()),
    })?;

    let stored =
        fs::read_to_string(&path).with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(snapshot.messages, full);
    let progress = snapshot.progress.expect("progress should exist");
    assert_eq!(progress.turn_number, 2);
    assert_eq!(progress.recent_messages, recent);
    assert_eq!(progress.token_usage, Some("10 tokens".to_string()));
    assert_eq!(progress.tool_summaries, vec!["tool_a".to_string()]);
    assert_eq!(progress.max_context_tokens, Some(128));
    assert_eq!(progress.turn_diagnostics, Some(diagnostics));
    assert_eq!(snapshot.transcript, vec!["recent".to_string()]);
    Ok(())
}
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_progress_transcript_skips_tool_noise_and_duplicates() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium");
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
        distinct_tools: vec!["command_session".to_owned()],
        messages: recent.clone(),
        recent_messages: recent,
        turn_number: 2,
        token_usage: Some("10 tokens".to_string()),
        max_context_tokens: Some(128),
        loaded_skills: None,
        turn_diagnostics: None,
    })?;

    let stored =
        fs::read_to_string(&path).with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(snapshot.transcript, vec!["run cargo check".to_string(), "done".to_string()]);
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
        "[!] Turn balancer: repeated low-signal calls detected; scheduling a final recovery pass.".to_string(),
        "  I couldn't produce a final synthesis because the model returned no answer on the recovery pass.".to_string(),
        "  Latest tool output: {\"output\":\"...\"}".to_string(),
        "  Reuse the latest tool outputs already collected in this turn before retrying.".to_string(),
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
#[serial_test::serial(session_dir_override)]
#[tokio::test]
async fn session_progress_normalizes_exec_tool_aliases_in_summaries() -> Result<()> {
    let _settings_lock = lock_history_test_guard().await;
    let _history_guard = HistorySettingsGuard::set(HistoryPersistence::File, None);
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let _guard = EnvGuard::set(SESSION_DIR_ENV, temp_dir.path());

    let metadata =
        SessionArchiveMetadata::new("ExampleWorkspace", "/tmp/example", "model-x", "provider-y", "dark", "medium");
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
        messages: recent.clone(),
        recent_messages: recent,
        turn_number: 2,
        token_usage: Some("10 tokens".to_string()),
        max_context_tokens: Some(128),
        loaded_skills: None,
        turn_diagnostics: None,
    })?;

    let stored =
        fs::read_to_string(&path).with_context(|| format!("failed to read stored session: {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&stored).context("failed to deserialize stored snapshot")?;

    assert_eq!(snapshot.distinct_tools, vec![tool_names::UNIFIED_EXEC.to_string()]);
    let progress = snapshot.progress.expect("progress should exist");
    assert_eq!(progress.tool_summaries, vec![tool_names::UNIFIED_EXEC.to_string()]);
    Ok(())
}
