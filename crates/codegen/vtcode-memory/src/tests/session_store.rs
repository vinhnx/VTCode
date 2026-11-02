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

/// Hold a session's lock file on a foreign fd, simulating another process.
/// Same-process flock conflicts, so this deterministically reproduces
/// cross-process contention without spawning subprocesses.
fn hold_session_lock_externally(workspace: &std::path::Path, session_id: &str) -> fs::File {
    let lock_path = sessions_root(workspace).join(session_id).join("session.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("lock file must exist");
    file.try_lock().expect("test must establish the foreign hold");
    file
}

fn lock_is_held_externally(workspace: &std::path::Path, session_id: &str) -> bool {
    let lock_path = sessions_root(workspace).join(session_id).join("session.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("lock file must exist");
    file.try_lock()
        .map_err(std::io::Error::from)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
}

#[test]
fn exclusive_open_fails_when_held_elsewhere_and_recovers_on_release() {
    let dir = TempDir::new().expect("tempdir");
    let manifest_path = sessions_root(dir.path()).join("sess-busy").join("manifest.json");
    {
        let log = open_exclusive(dir.path(), "sess-busy", DEFAULT_MAX_EVENTS).expect("first open");
        for e in &sample_turn() {
            log.append(e).expect("append");
        }
    }
    let manifest_before = fs::read(&manifest_path).expect("manifest persisted");
    let foreign = hold_session_lock_externally(dir.path(), "sess-busy");
    let err = open_exclusive(dir.path(), "sess-busy", DEFAULT_MAX_EVENTS)
        .map(|_| ())
        .expect_err("held session must fail");
    assert!(
        matches!(err, SessionStoreError::SessionBusy { .. }),
        "must fail closed with SessionBusy, got: {err}"
    );
    assert!(err.to_string().contains("sess-busy"), "must name the session: {err}");
    assert!(err.to_string().contains("--session-id"), "must name the remedy: {err}");
    assert_eq!(
        fs::read(&manifest_path).expect("manifest"),
        manifest_before,
        "a refused open must not mutate store state"
    );
    drop(foreign);
    let log = open_exclusive(dir.path(), "sess-busy", DEFAULT_MAX_EVENTS).expect("open after release");
    assert_eq!(log.turn_count(), 1, "history survives the refused open");
}

#[test]
fn shared_open_degrades_when_held_elsewhere() {
    let dir = TempDir::new().expect("tempdir");
    {
        let log = open_exclusive(dir.path(), "sess-read", DEFAULT_MAX_EVENTS).expect("first open");
        for e in &sample_turn() {
            log.append(e).expect("append");
        }
    }
    let _foreign = hold_session_lock_externally(dir.path(), "sess-read");
    // Readers tolerate a live writer: the open succeeds degraded.
    let log = open(dir.path(), "sess-read", DEFAULT_MAX_EVENTS).expect("degraded shared open");
    assert_eq!(log.turn_count(), 1);
}

#[test]
fn exclusive_reopen_shares_state_in_process() {
    let dir = TempDir::new().expect("tempdir");
    let first = open_exclusive(dir.path(), "sess-shared", DEFAULT_MAX_EVENTS).expect("first open");
    let second = open_exclusive(dir.path(), "sess-shared", DEFAULT_MAX_EVENTS).expect("in-process reopen");
    for e in &sample_turn() {
        first.append(e).expect("append");
    }
    assert_eq!(second.turn_count(), 1, "reopened handles share one turn index");
}

#[test]
fn exclusive_open_upgrades_after_degraded_shared_reader() {
    let dir = TempDir::new().expect("tempdir");
    // Establish the session, release it, then hold it externally so the
    // shared open below degrades (no fd) instead of claiming the lock.
    drop(open_exclusive(dir.path(), "sess-upgrade", DEFAULT_MAX_EVENTS).expect("first open"));
    let foreign = hold_session_lock_externally(dir.path(), "sess-upgrade");
    let _reader = open(dir.path(), "sess-upgrade", DEFAULT_MAX_EVENTS).expect("degraded shared open");
    drop(foreign);
    let _writer = open_exclusive(dir.path(), "sess-upgrade", DEFAULT_MAX_EVENTS).expect("upgrade");
    assert!(
        lock_is_held_externally(dir.path(), "sess-upgrade"),
        "the upgraded writer must hold the liveness lock"
    );
}
