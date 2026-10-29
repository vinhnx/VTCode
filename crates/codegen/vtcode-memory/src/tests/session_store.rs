use super::*;

#[test]
fn append_and_reconstruct_roundtrip() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-1", DEFAULT_MAX_EVENTS).expect("open");
    for _ in 0..3 {
        for e in &sample_turn() {
            log.append(e).expect("append");
        }
    }
    assert_eq!(log.turn_count(), 3);
    let rebuilt = log.reconstruct_turn(2).expect("reconstruct");
    assert_eq!(rebuilt.len(), 2);
    assert!(matches!(rebuilt[0], ThreadEvent::TurnStarted(_)));
    assert!(matches!(rebuilt[1], ThreadEvent::TurnCompleted(_)));
}

#[test]
fn event_log_batches_appends_until_turn_boundary() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-buffered", DEFAULT_MAX_EVENTS).expect("open");
    let events_path = sessions_root(dir.path()).join("sess-buffered").join("events.jsonl");

    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }))
        .expect("append thread event");
    assert_eq!(fs::metadata(&events_path).expect("metadata").len(), 0);

    log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
        .expect("append turn start");
    assert_eq!(fs::metadata(&events_path).expect("metadata").len(), 0);

    log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    }))
    .expect("append turn completion");
    assert!(fs::metadata(&events_path).expect("metadata").len() > 0);
    assert_eq!(log.reconstruct_turn(1).expect("reconstruct").len(), 2);
}

#[test]
fn large_buffer_flush_persists_manifest_progress() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-large-buffer", DEFAULT_MAX_EVENTS).expect("open");

    for index in 0..1_000 {
        log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: format!("thread-{index:04}-buffer-boundary"),
        }))
        .expect("append event");
    }

    let manifest_path = sessions_root(dir.path()).join("sess-large-buffer").join("manifest.json");
    let manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(manifest_path).expect("read manifest")).expect("parse manifest");
    assert!(manifest.event_count > 0);
    assert!(manifest.event_count < 1_000);
}

#[test]
fn flushing_mid_turn_persists_buffered_metadata_for_reopen() {
    let dir = TempDir::new().expect("tempdir");
    {
        let log = open(dir.path(), "sess-mid-turn", DEFAULT_MAX_EVENTS).expect("open");
        log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }))
            .expect("append thread event");
        log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
            .expect("append turn start");
        log.flush().expect("flush mid-turn event log");
    }

    let reopened = open(dir.path(), "sess-mid-turn", DEFAULT_MAX_EVENTS).expect("reopen");
    assert_eq!(reopened.event_count(), 2);
    assert_eq!(reopened.turn_count(), 0);
    assert_eq!(reopened.reconstruct_turn(1).expect("reconstruct open turn").len(), 1);
}

#[test]
fn reopening_after_drop_scans_bytes_flushed_without_metadata() {
    let dir = TempDir::new().expect("tempdir");
    {
        let log = open(dir.path(), "sess-drop-flush", DEFAULT_MAX_EVENTS).expect("open");
        log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }))
            .expect("append thread event");
        // Drop performs a best-effort byte flush but deliberately leaves the
        // manifest untouched, so reopen must detect the stale file length.
    }

    let reopened = open(dir.path(), "sess-drop-flush", DEFAULT_MAX_EVENTS).expect("reopen");
    assert_eq!(reopened.event_count(), 1);
}

#[test]
fn reopening_mid_turn_preserves_state_for_completion() {
    let dir = TempDir::new().expect("tempdir");
    {
        let log = open(dir.path(), "sess-mid-turn-resume", DEFAULT_MAX_EVENTS).expect("open");
        log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
            .expect("append turn start");
        log.flush().expect("flush mid-turn event log");
    }

    let reopened = open(dir.path(), "sess-mid-turn-resume", DEFAULT_MAX_EVENTS).expect("reopen");
    reopened
        .append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }))
        .expect("append turn completion");

    assert_eq!(reopened.turn_count(), 1);
    assert_eq!(reopened.reconstruct_turn(1).expect("reconstruct completed turn").len(), 2);
}

#[test]
fn index_rebuilt_on_reopen() {
    let dir = TempDir::new().expect("tempdir");
    {
        let log = open(dir.path(), "sess-2", DEFAULT_MAX_EVENTS).expect("open");
        for e in &sample_turn() {
            log.append(e).expect("append");
        }
        log.complete().expect("complete");
    }
    // Reopen: scan must rebuild the index from events.jsonl.
    let log = SessionEventLog::open(dir.path(), "sess-2", DEFAULT_MAX_EVENTS).expect("reopen");
    assert_eq!(log.turn_count(), 1);
    let rebuilt = log.reconstruct_turn(1).expect("reconstruct after reopen");
    assert_eq!(rebuilt.len(), 2);
    assert!(log.manifest().status == "completed");
}

#[test]
fn migrate_legacy_imports_history_and_trajectory() {
    let dir = TempDir::new().expect("tempdir");
    let vt = dir.path().join(".vtcode");
    fs::create_dir_all(vt.join("history")).expect("mk history");
    fs::create_dir_all(vt.join("logs")).expect("mk logs");

    let memory = serde_json::json!({
        "session_id": "session-foo",
        "schema_version": 2,
        "summary": "did a thing",
        "grounded_facts": [{"fact": "the widget is blue"}],
    });
    fs::write(
        vt.join("history").join("session-foo.memory.json"),
        serde_json::to_string_pretty(&memory).expect("ser"),
    )
    .expect("write memory");

    fs::write(
        vt.join("logs").join("trajectory-20260101T000000Z.jsonl"),
        "{\"kind\":\"llm_retry_metrics\",\"turn\":1}\n",
    )
    .expect("write traj");

    let report = migrate_legacy(dir.path(), false).expect("migrate");
    assert_eq!(report.sessions_created, 2);
    assert_eq!(report.memory_imported, 1);
    assert_eq!(report.trajectory_imported, 1);

    // Cross-session fact query works off the unified store.
    let facts = query_facts(dir.path(), 10).expect("facts");
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].fact, "the widget is blue");

    // Legacy history + logs still present (remove_legacy=false).
    assert!(vt.join("history").exists());
    assert!(vt.join("logs").exists());

    // recent_sessions lists the migrated sessions.
    let sessions = recent_sessions(dir.path(), 10);
    assert_eq!(sessions.len(), 2);
}
