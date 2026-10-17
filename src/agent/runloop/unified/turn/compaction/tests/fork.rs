use super::*;

#[tokio::test]
async fn compaction_strips_existing_memory_envelope_before_recompacting() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let original_len = history.len();
    history.insert(0, Message::system("[Session Memory Envelope]\nSummary:\nPersisted summary".to_string()));
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    let outcome = compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_eq!(outcome.original_len, original_len);
    assert!(
        outcome.compacted_len < outcome.original_len,
        "expected shrink, got {} -> {}",
        outcome.original_len,
        outcome.compacted_len
    );
    assert_eq!(
        history
            .iter()
            .filter(|message| message.content.as_text().contains("[Session Memory Envelope]"))
            .count(),
        1
    );
}

#[tokio::test]
async fn summarized_fork_history_reuses_compaction_pipeline_and_prior_envelope() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    let source_envelope = SessionMemoryEnvelope {
        session_id: "session-source".to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Prior source summary".to_string(),
        objective: Some("Keep the source session moving".to_string()),
        task_summary: Some("Tracker: keep going".to_string()),
        spec_summary: None,
        evaluation_summary: None,
        verification_summary: None,
        constraints: Vec::new(),
        grounded_facts: vec![GroundedFactRecord {
            fact: "src/lib.rs was updated".to_string(),
            source: "tool:write_file".to_string(),
        }],
        touched_files: vec!["src/lib.rs".to_string()],
        open_questions: Vec::new(),
        verification_todo: Vec::new(),
        delegation_notes: Vec::new(),
        pending_intents: Vec::new(),
        applied_intent_ids: Vec::new(),
        history_artifact_path: Some(".vtcode/history/session-source_0001.jsonl".to_string()),
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(
        history_dir.join("session-source_0001.memory.json"),
        serde_json::to_string_pretty(&source_envelope).expect("serialize envelope"),
    )
    .expect("write envelope");

    let compacted = build_summarized_fork_history(
        &LocalCompactionProvider,
        "stub-model",
        "session-source",
        "session-target",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &test_history(),
        false,
    )
    .await
    .expect("summarized fork history");

    // The fixture fits within the continuity tail, so the fork keeps the
    // complete protocol history in addition to its summary metadata.
    assert!(compacted.len() >= 6);
    assert!(compacted[0].content.as_text().contains("[Session Memory Envelope]"));
    assert!(compacted[0].content.as_text().contains("src/lib.rs"));
    assert!(compacted[1].content.as_text().contains("Previous conversation summary"));
    assert_eq!(compacted.iter().filter(|message| message.role == MessageRole::User).count(), 4);
    assert_history_contains_messages(&compacted, &test_history());
}

#[tokio::test]
async fn budget_resume_summary_reuses_saved_envelope_without_provider_compaction() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("create history dir");

    let source_envelope = SessionMemoryEnvelope {
        session_id: "session-source".to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Budget-limited session summary".to_string(),
        objective: None,
        task_summary: None,
        spec_summary: None,
        evaluation_summary: None,
        verification_summary: None,
        constraints: Vec::new(),
        grounded_facts: Vec::new(),
        touched_files: vec!["src/lib.rs".to_string()],
        open_questions: Vec::new(),
        verification_todo: Vec::new(),
        delegation_notes: Vec::new(),
        pending_intents: Vec::new(),
        applied_intent_ids: Vec::new(),
        history_artifact_path: None,
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(
        history_dir.join("session-source_0001.memory.json"),
        serde_json::to_string_pretty(&source_envelope).expect("serialize envelope"),
    )
    .expect("write envelope");

    let compacted = build_summarized_fork_history(
        &FailingProviderCompactionProvider,
        "stub-model",
        "session-source",
        "session-target",
        temp.path(),
        Some(&VTCodeConfig::default()),
        &test_history(),
        true,
    )
    .await
    .expect("saved summary fork history");

    assert!(compacted[0].content.as_text().contains("[Session Memory Envelope]"));
    assert!(compacted[1].content.as_text().contains("Budget-limited session summary"));
}

#[tokio::test]
async fn local_and_fork_compaction_preserve_continuity_tail() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.context.dynamic.retained_user_messages = 2;

    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    let mut session_stats = SessionStats::default();
    let mut context_manager = test_context_manager();

    compact_history_in_place(
        &provider,
        "stub-model",
        "session-alpha",
        temp.path(),
        Some(&vt_cfg),
        &mut history,
        &mut session_stats,
        &mut context_manager,
    )
    .await
    .expect("compaction succeeds")
    .expect("history should compact");

    assert_local_compaction_history_with_user_count(&history, 2);

    let compacted = build_summarized_fork_history(
        &provider,
        "stub-model",
        "session-alpha",
        "session-beta",
        temp.path(),
        Some(&vt_cfg),
        &test_history(),
        false,
    )
    .await
    .expect("summarized fork history");

    assert_eq!(compacted.iter().filter(|message| message.role == MessageRole::User).count(), 4);
    assert_history_contains_messages(&compacted, &test_history());
}
