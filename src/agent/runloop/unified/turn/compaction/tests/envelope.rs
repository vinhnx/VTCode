use super::*;

#[test]
fn legacy_memory_envelope_deserializes_with_new_fields_defaulted() {
    let envelope: SessionMemoryEnvelope = serde_json::from_value(json!({
        "session_id": "session-alpha",
        "summary": "Persisted summary",
        "task_summary": "Task tracker",
        "spec_summary": null,
        "evaluation_summary": null,
        "grounded_facts": [{
            "fact": "fact",
            "source": "tool:read_file"
        }],
        "touched_files": ["src/lib.rs"],
        "history_artifact_path": ".vtcode/history/session-alpha.jsonl",
        "generated_at": "2026-03-14T00:00:00Z"
    }))
    .expect("legacy envelope should deserialize");

    assert_eq!(envelope.schema_version, None);
    assert_eq!(envelope.objective, None);
    assert_eq!(envelope.verification_summary, None);
    assert!(envelope.constraints.is_empty());
    assert!(envelope.open_questions.is_empty());
    assert!(envelope.verification_todo.is_empty());
    assert!(envelope.delegation_notes.is_empty());
}

#[test]
fn refresh_session_memory_envelope_merges_existing_continuity_fields() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    fs::create_dir_all(temp.path().join(".vtcode").join("tasks")).expect("tasks dir");
    fs::write(
        temp.path().join(".vtcode").join("tasks").join("current_task.md"),
        "# Ship compaction cleanup\n- [ ] Run cargo nextest\n- [x] Wire in config\n",
    )
    .expect("write task");
    fs::write(
        temp.path().join(".vtcode").join("tasks").join("current_spec.md"),
        "# Spec\nKeep local compaction aligned with summarized forks.\n",
    )
    .expect("write spec");
    fs::write(
        temp.path().join(".vtcode").join("tasks").join("current_evaluation.md"),
        "# Eval\nNeed a regression test for repeated reads.\n",
    )
    .expect("write eval");

    let prior_envelope = SessionMemoryEnvelope {
        session_id: "session-alpha".to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Prior summary".to_string(),
        objective: Some("Keep continuity".to_string()),
        task_summary: Some("Older task summary".to_string()),
        spec_summary: None,
        evaluation_summary: None,
        verification_summary: Some("- [x] Prior verification passed".to_string()),
        constraints: vec!["Do not redesign the harness".to_string()],
        grounded_facts: vec![GroundedFactRecord {
            fact: "Existing grounded fact".to_string(),
            source: "tool:read_file".to_string(),
        }],
        touched_files: vec!["src/old.rs".to_string()],
        open_questions: vec!["What should summarized forks retain?".to_string()],
        verification_todo: vec!["Confirm refresh runs at turn boundaries.".to_string()],
        delegation_notes: vec!["explorer: looked at compaction flow".to_string()],
        pending_intents: Vec::new(),
        applied_intent_ids: Vec::new(),
        history_artifact_path: Some(".vtcode/history/session-alpha_0001.jsonl".to_string()),
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(
        history_dir.join("session-alpha.memory.json"),
        serde_json::to_string_pretty(&prior_envelope).expect("serialize envelope"),
    )
    .expect("write envelope");

    let history = vec![
        Message::user("Continue the compaction work.".to_string()),
        Message::assistant("I will update the local compaction path.".to_string()),
    ];
    let original_history = history.clone();
    let mut session_stats = SessionStats::default();
    session_stats.record_touched_files(["src/new.rs".to_string()]);

    let update = SessionMemoryEnvelopeUpdate {
        grounded_facts: vec![GroundedFactRecord {
            fact: "Child agent confirmed the parser contract.".to_string(),
            source: "subagent:reviewer".to_string(),
        }],
        touched_files: vec!["src/child.rs".to_string()],
        open_questions: vec!["Should dedup cover batch reads?".to_string()],
        verification_todo: vec!["Run cargo check".to_string()],
        delegation_notes: vec!["reviewer: parser contract validated".to_string()],
        ..Default::default()
    };

    let envelope = refresh_session_memory_envelope(
        temp.path(),
        "session-alpha",
        Some(&VTCodeConfig::default()),
        &history,
        &session_stats,
        Some(&update),
    )
    .expect("refresh succeeds")
    .expect("envelope should be refreshed");

    assert_eq!(
        envelope.objective.as_deref(),
        Some("Ship compaction cleanup"),
        "live task-tracker objective wins over a stale prior envelope"
    );
    assert!(envelope.constraints.contains(&"Do not redesign the harness".to_string()));
    assert!(
        envelope
            .spec_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Keep local compaction aligned"))
    );
    assert!(
        envelope
            .evaluation_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Need a regression test"))
    );
    assert!(
        envelope
            .verification_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Run cargo nextest"))
    );
    assert!(envelope.open_questions.contains(&"Should dedup cover batch reads?".to_string()));
    assert!(envelope.verification_todo.iter().any(|item| item.contains("Run cargo nextest")));
    assert!(envelope.verification_todo.contains(&"Run cargo check".to_string()));
    assert!(
        envelope
            .delegation_notes
            .contains(&"reviewer: parser contract validated".to_string())
    );
    assert!(envelope.touched_files.contains(&"src/new.rs".to_string()));
    assert!(envelope.touched_files.contains(&"src/child.rs".to_string()));
    assert_eq!(history, original_history, "ordinary refresh should not mutate live history");

    let persisted_path =
        latest_memory_envelope_path_for_session(temp.path(), "session-alpha").expect("persisted envelope path");
    let persisted: SessionMemoryEnvelope =
        serde_json::from_str(&fs::read_to_string(persisted_path).expect("read persisted envelope"))
            .expect("deserialize persisted envelope");
    assert_eq!(persisted, envelope);
}

#[test]
fn refresh_session_memory_envelope_prefers_structured_verify_metadata() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    fs::create_dir_all(temp.path().join(".vtcode").join("tasks")).expect("tasks dir");
    fs::write(
        temp.path()
            .join(".vtcode")
            .join("tasks")
            .join("current_task.md"),
        "# Ship compaction cleanup\n- [x] Analyze current continuity path\n  outcome: Existing envelope flow reviewed.\n- [ ] Update verification preservation\n  verify: cargo check -p vtcode\n- [ ] Run focused regression\n  verify:\n    - cargo test -p vtcode --bin vtcode agent::runloop::unified::turn::compaction::tests::refresh_session_memory_envelope_prefers_structured_verify_metadata -- --exact\n",
    )
    .expect("write task");

    let history = vec![Message::user("Continue the compaction work.".to_string())];
    let original_history = history.clone();
    let session_stats = SessionStats::default();

    let envelope = refresh_session_memory_envelope(
        temp.path(),
        "session-alpha",
        Some(&VTCodeConfig::default()),
        &history,
        &session_stats,
        None,
    )
    .expect("refresh succeeds")
    .expect("envelope should be refreshed");

    assert_eq!(
        envelope.verification_summary.as_deref(),
        Some(
            "- cargo check -p vtcode\n- cargo test -p vtcode --bin vtcode agent::runloop::unified::turn::compaction::tests::refresh_session_memory_envelope_prefers_structured_verify_metadata -- --exact"
        )
    );
    assert_eq!(history, original_history, "ordinary refresh should not mutate live history");
    let persisted_path =
        latest_memory_envelope_path_for_session(temp.path(), "session-alpha").expect("persisted envelope path");
    let persisted: SessionMemoryEnvelope =
        serde_json::from_str(&fs::read_to_string(persisted_path).expect("read persisted envelope"))
            .expect("deserialize persisted envelope");
    assert_eq!(persisted, envelope);
    assert!(
        persisted
            .verification_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("- cargo check -p vtcode"))
    );
    let focused_regression = "- cargo test -p vtcode --bin vtcode agent::runloop::unified::turn::compaction::tests::refresh_session_memory_envelope_prefers_structured_verify_metadata -- --exact";
    assert!(
        persisted
            .verification_summary
            .as_deref()
            .is_some_and(|summary| summary.contains(focused_regression))
    );
}

#[test]
fn refresh_session_memory_envelope_is_throttled_when_nothing_changes() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    fs::create_dir_all(temp.path().join(".vtcode").join("tasks")).expect("tasks dir");
    fs::write(
        temp.path().join(".vtcode").join("tasks").join("current_task.md"),
        "# Ship compaction cleanup\n- [ ] Run cargo nextest\n",
    )
    .expect("write task");

    let prior_envelope = SessionMemoryEnvelope {
        session_id: "session-alpha".to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Prior summary".to_string(),
        objective: Some("Keep continuity".to_string()),
        task_summary: Some("Ship compaction cleanup: - [ ] Run cargo nextest".to_string()),
        spec_summary: None,
        evaluation_summary: None,
        verification_summary: None,
        constraints: vec![],
        grounded_facts: vec![],
        touched_files: vec![],
        open_questions: vec![],
        verification_todo: vec![],
        delegation_notes: vec![],
        pending_intents: Vec::new(),
        applied_intent_ids: Vec::new(),
        history_artifact_path: None,
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(
        history_dir.join("session-alpha.memory.json"),
        serde_json::to_string_pretty(&prior_envelope).expect("serialize envelope"),
    )
    .expect("write envelope");

    let history = vec![Message::user("Continue the compaction work.".to_string())];
    let original_history = history.clone();
    let session_stats = SessionStats::default();

    // First refresh with matching state should still write once because the
    // summary derived from history differs from the prior one. After that, an
    // identical refresh must be throttled.
    let first = refresh_session_memory_envelope(
        temp.path(),
        "session-alpha",
        Some(&VTCodeConfig::default()),
        &history,
        &session_stats,
        None,
    )
    .expect("refresh succeeds");
    assert!(first.is_some(), "first refresh should produce an envelope");
    assert_eq!(history, original_history, "ordinary refresh should not mutate live history");
    let history_len_after_first = history.len();

    let second = refresh_session_memory_envelope(
        temp.path(),
        "session-alpha",
        Some(&VTCodeConfig::default()),
        &history,
        &session_stats,
        None,
    )
    .expect("refresh succeeds");
    assert!(second.is_none(), "second identical refresh should be throttled");
    assert_eq!(history.len(), history_len_after_first, "history should not grow when refresh is throttled");
    assert_eq!(history, original_history, "throttled refresh should leave live history unchanged");
}

#[test]
fn refresh_session_memory_envelope_summary_is_concise() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    fs::create_dir_all(temp.path().join(".vtcode").join("tasks")).expect("tasks dir");
    fs::write(
        temp.path().join(".vtcode").join("tasks").join("current_task.md"),
        "# Audit agent loop\n- [ ] Reduce duplicated work\n",
    )
    .expect("write task");

    let history = vec![
        Message::user("Check the log and reduce repeated duplicated work.".to_string()),
        Message::assistant("I will audit the agent loop.".to_string()),
        Message::tool_response("call_1".to_string(), json!({"error": "some tool error"}).to_string()),
    ];
    let original_history = history.clone();
    let session_stats = SessionStats::default();

    let envelope = refresh_session_memory_envelope(
        temp.path(),
        "session-alpha",
        Some(&VTCodeConfig::default()),
        &history,
        &session_stats,
        None,
    )
    .expect("refresh succeeds")
    .expect("envelope should be refreshed");

    assert!(!envelope.summary.contains("{\"error\""), "summary should not contain raw JSON tool output");
    assert!(envelope.summary.len() < 300, "summary should be concise, got: {}", envelope.summary);
    assert!(envelope.summary.contains("Audit agent loop"), "summary should reference the objective");
    assert_eq!(history, original_history, "ordinary refresh should not mutate live history");
}
