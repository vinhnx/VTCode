use super::*;

#[test]
fn inject_latest_memory_envelope_rehydrates_resume_history() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");
    let envelope_path = history_dir.join("resume-session_001.memory.json");
    let envelope = SessionMemoryEnvelope {
        session_id: "resume-session".to_string(),
        schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
        summary: "Persisted summary".to_string(),
        objective: None,
        task_summary: Some("Tracker: - [ ] Follow up".to_string()),
        spec_summary: None,
        evaluation_summary: None,
        verification_summary: Some("- [ ] Run cargo nextest".to_string()),
        constraints: Vec::new(),
        grounded_facts: vec![GroundedFactRecord {
            fact: "Cargo.toml declares vtcode-core".to_string(),
            source: "tool:read_file".to_string(),
        }],
        touched_files: vec!["Cargo.toml".to_string()],
        open_questions: Vec::new(),
        verification_todo: Vec::new(),
        delegation_notes: Vec::new(),
        pending_intents: Vec::new(),
        applied_intent_ids: Vec::new(),
        history_artifact_path: Some(".vtcode/history/resume-session_001.jsonl".to_string()),
        generated_at: "2026-03-14T00:00:00Z".to_string(),
    };
    fs::write(&envelope_path, serde_json::to_string_pretty(&envelope).expect("serialize envelope"))
        .expect("write envelope");

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(temp.path(), "resume-session", &mut history));
    assert!(history[0].content.as_text().contains("Persisted summary"));
    assert!(history[0].content.as_text().contains("Cargo.toml"));
    assert!(history[0].content.as_text().contains("Verification Status"));
}

#[test]
fn inject_latest_memory_envelope_is_session_scoped() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");

    for (session_id, summary) in [("session-alpha", "Alpha summary"), ("session-beta", "Beta summary")] {
        let envelope_path = history_dir.join(format!("{session_id}_0001.memory.json"));
        let envelope = SessionMemoryEnvelope {
            session_id: session_id.to_string(),
            schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: summary.to_string(),
            objective: None,
            task_summary: None,
            spec_summary: None,
            evaluation_summary: None,
            verification_summary: None,
            constraints: Vec::new(),
            grounded_facts: Vec::new(),
            touched_files: Vec::new(),
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            pending_intents: Vec::new(),
            applied_intent_ids: Vec::new(),
            history_artifact_path: None,
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        fs::write(envelope_path, serde_json::to_string_pretty(&envelope).expect("serialize envelope"))
            .expect("write envelope");
    }

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(temp.path(), "session-beta", &mut history));
    assert!(history[0].content.as_text().contains("Beta summary"));
    assert!(!history[0].content.as_text().contains("Alpha summary"));
}

#[test]
fn inject_latest_memory_envelope_requires_exact_session_prefix_match() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");

    for (file_name, summary) in [
        ("session-a_0001.memory.json", "Exact summary"),
        ("session-alpha_0002.memory.json", "Wrong summary"),
    ] {
        let envelope = SessionMemoryEnvelope {
            session_id: "session-a".to_string(),
            schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: summary.to_string(),
            objective: None,
            task_summary: None,
            spec_summary: None,
            evaluation_summary: None,
            verification_summary: None,
            constraints: Vec::new(),
            grounded_facts: Vec::new(),
            touched_files: Vec::new(),
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            pending_intents: Vec::new(),
            applied_intent_ids: Vec::new(),
            history_artifact_path: None,
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        fs::write(history_dir.join(file_name), serde_json::to_string_pretty(&envelope).expect("serialize envelope"))
            .expect("write envelope");
    }

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(temp.path(), "session-a", &mut history));
    assert!(history[0].content.as_text().contains("Exact summary"));
    assert!(!history[0].content.as_text().contains("Wrong summary"));
}

#[tokio::test]
async fn no_envelope_written_when_dynamic_history_is_disabled() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.context.dynamic.enabled = false;

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
    .expect("compaction succeeds");

    assert!(latest_memory_envelope_path_for_session(temp.path(), "session-alpha").is_none());
    assert!(history[0].content.as_text().contains("Previous conversation summary"));
}

#[tokio::test]
async fn persisted_envelope_uses_recorded_touched_files_only() {
    let temp = tempdir().expect("tempdir");
    let provider = RoomyLocalCompactionProvider;
    let mut history = tool_heavy_history(ROOMY_HISTORY_CYCLES);
    history.push(Message::user("Mentioning docs/example.md in prose should not populate touched files.".to_string()));
    let mut session_stats = SessionStats::default();
    session_stats.record_touched_files(["src/main.rs".to_string(), "Cargo.toml".to_string()]);
    let mut context_manager = test_context_manager();

    compact_history_in_place(
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
    .expect("compaction succeeds");

    let envelope_path = latest_memory_envelope_path_for_session(temp.path(), "session-alpha").expect("envelope path");
    let envelope: SessionMemoryEnvelope =
        serde_json::from_str(&fs::read_to_string(envelope_path).expect("read envelope")).expect("parse envelope");

    assert_eq!(envelope.touched_files, vec!["src/main.rs".to_string(), "Cargo.toml".to_string()]);
    assert_eq!(envelope.session_id, "session-alpha");
}

#[test]
fn inject_latest_memory_envelope_uses_exact_session_id_when_prefixes_collide() {
    let temp = tempdir().expect("tempdir");
    let history_dir = temp.path().join(".vtcode").join("history");
    fs::create_dir_all(&history_dir).expect("history dir");

    let session_alpha = "01234567890123456789012345678901-alpha";
    let session_beta = "01234567890123456789012345678901-beta";

    for (session_id, summary, suffix) in [
        (session_alpha, "Alpha summary", "0001"),
        (session_beta, "Beta summary", "0002"),
    ] {
        let envelope = SessionMemoryEnvelope {
            session_id: session_id.to_string(),
            schema_version: Some(SESSION_MEMORY_ENVELOPE_SCHEMA_VERSION),
            summary: summary.to_string(),
            objective: None,
            task_summary: None,
            spec_summary: None,
            evaluation_summary: None,
            verification_summary: None,
            constraints: Vec::new(),
            grounded_facts: Vec::new(),
            touched_files: Vec::new(),
            open_questions: Vec::new(),
            verification_todo: Vec::new(),
            delegation_notes: Vec::new(),
            pending_intents: Vec::new(),
            applied_intent_ids: Vec::new(),
            history_artifact_path: None,
            generated_at: "2026-03-14T00:00:00Z".to_string(),
        };
        let file_name = format!("{}_{suffix}.memory.json", &session_id[..32]);
        fs::write(history_dir.join(file_name), serde_json::to_string_pretty(&envelope).expect("serialize envelope"))
            .expect("write envelope");
    }

    let mut history = vec![Message::user("resume".to_string())];
    assert!(inject_latest_memory_envelope(temp.path(), session_alpha, &mut history));
    assert!(history[0].content.as_text().contains("Alpha summary"));
    assert!(!history[0].content.as_text().contains("Beta summary"));
}
