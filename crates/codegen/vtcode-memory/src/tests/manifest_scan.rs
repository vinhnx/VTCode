use super::*;

#[test]
fn manifest_shortcut_skips_scan_on_reopen() {
    let dir = TempDir::new().expect("tempdir");
    // Write a few turns and complete.
    {
        let log = open(dir.path(), "sess-shortcut", DEFAULT_MAX_EVENTS).expect("open");
        for e in &sample_turn() {
            log.append(e).expect("append");
        }
        log.complete().expect("complete");
    }
    // Reopen: the manifest + index should be loaded without scanning.
    let log = SessionEventLog::open(dir.path(), "sess-shortcut", DEFAULT_MAX_EVENTS).expect("reopen");
    assert_eq!(log.turn_count(), 1);
    assert_eq!(log.manifest().status, "completed");
    let rebuilt = log.reconstruct_turn(1).expect("reconstruct");
    assert_eq!(rebuilt.len(), 2);
}

#[test]
fn corrupt_manifest_falls_back_to_event_log_scan() {
    let dir = TempDir::new().expect("tempdir");
    {
        let log = open(dir.path(), "sess-corrupt-manifest", DEFAULT_MAX_EVENTS).expect("open");
        for event in &sample_turn() {
            log.append(event).expect("append");
        }
        log.complete().expect("complete");
    }

    let session_dir = sessions_root(dir.path()).join("sess-corrupt-manifest");
    fs::write(session_dir.join("manifest.json"), b"{\"broken\"").expect("corrupt manifest");

    let reopened = open(dir.path(), "sess-corrupt-manifest", DEFAULT_MAX_EVENTS).expect("recover from manifest");
    assert_eq!(reopened.turn_count(), 1);
    assert_eq!(reopened.reconstruct_turn(1).expect("reconstruct").len(), 2);
    serde_json::from_str::<crate::SessionManifest>(
        &fs::read_to_string(session_dir.join("manifest.json")).expect("read repaired manifest"),
    )
    .expect("manifest should be repaired");
}

#[test]
fn stale_turn_index_offsets_fall_back_to_event_log_scan() {
    let dir = TempDir::new().expect("tempdir");
    {
        let log = open(dir.path(), "sess-stale-index", DEFAULT_MAX_EVENTS).expect("open");
        for event in &sample_turn() {
            log.append(event).expect("append");
        }
        log.complete().expect("complete");
    }

    let index_path = sessions_root(dir.path()).join("sess-stale-index/index/turns.json");
    let mut index: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&index_path).expect("read index")).expect("parse index");
    index["entries"][0]["end_offset"] = serde_json::json!(u64::MAX);
    fs::write(&index_path, serde_json::to_vec(&index).expect("serialize stale index")).expect("write stale index");

    let reopened = open(dir.path(), "sess-stale-index", DEFAULT_MAX_EVENTS).expect("recover from stale index");
    assert_eq!(reopened.turn_count(), 1);
    assert_eq!(reopened.reconstruct_turn(1).expect("reconstruct").len(), 2);
}

#[test]
fn current_manifest_rejects_a_stale_but_well_formed_turn_index() {
    let dir = TempDir::new().expect("tempdir");
    let session_id = "sess-stale-valid-index";
    let session_dir = sessions_root(dir.path()).join(session_id);
    let index_path = session_dir.join("index/turns.json");
    let log = open(dir.path(), session_id, DEFAULT_MAX_EVENTS).expect("open");
    for event in [
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
    ] {
        log.append(&event).expect("append first turn");
    }
    log.flush().expect("flush first turn");
    let first_turn_index = fs::read(&index_path).expect("read first index");
    for event in [
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
    ] {
        log.append(&event).expect("append second turn");
    }
    log.flush().expect("flush second turn");
    drop(log);

    // A crash after manifest.json was published but before turns.json would
    // leave exactly this pairing: the current file length and manifest with a
    // shorter, still structurally valid index.
    let mut current_index: serde_json::Value =
        serde_json::from_slice(&fs::read(&index_path).expect("read current index")).expect("parse current index");
    let first_index: serde_json::Value = serde_json::from_slice(&first_turn_index).expect("parse first index");
    let second_entry = current_index["entries"][1].clone();
    current_index["entries"] = serde_json::json!([first_index["entries"][0].clone()]);
    fs::write(&index_path, serde_json::to_vec(&current_index).expect("serialize stale index"))
        .expect("write stale index");

    let reopened = open(dir.path(), session_id, DEFAULT_MAX_EVENTS).expect("recover from stale index");
    assert_eq!(reopened.turn_count(), 2);
    assert_eq!(reopened.reconstruct_turn(1).expect("first turn").len(), 2);
    assert_eq!(reopened.reconstruct_turn(2).expect("second turn").len(), 2);

    // An index containing only turn 2 is also structurally valid, but its
    // first ordinal does not match the manifest's retained base. It must be
    // rejected and rebuilt so turn 1 remains addressable.
    drop(reopened);
    current_index["entries"] = serde_json::json!([second_entry]);
    fs::write(&index_path, serde_json::to_vec(&current_index).expect("serialize offset index"))
        .expect("write offset index");

    let reopened = open(dir.path(), session_id, DEFAULT_MAX_EVENTS).expect("recover from offset index");
    assert_eq!(reopened.turn_count(), 2);
    assert_eq!(reopened.reconstruct_turn(1).expect("rebuilt first turn").len(), 2);
    assert_eq!(reopened.reconstruct_turn(2).expect("rebuilt second turn").len(), 2);
}

#[test]
fn legacy_manifest_uses_valid_index_base_when_scanning() {
    let dir = TempDir::new().expect("tempdir");
    let session_id = "sess-legacy-index-base";
    let session_dir = sessions_root(dir.path()).join(session_id);
    let events_path = session_dir.join("events.jsonl");
    let index_path = session_dir.join("index/turns.json");
    {
        let log = open(dir.path(), session_id, 0).expect("open");
        for _ in 0..3 {
            log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
                .expect("append turn start");
            log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }))
            .expect("append turn completion");
        }
        log.flush().expect("flush");
    }

    let bytes = fs::read(&events_path).expect("read event log");
    let mut index: serde_json::Value =
        serde_json::from_slice(&fs::read(&index_path).expect("read turn index")).expect("parse turn index");
    let third_entry = index["entries"][2].clone();
    let third_start = third_entry["start_offset"].as_u64().expect("third turn start");
    let third_end = third_entry["end_offset"].as_u64().expect("third turn end");
    let retained_len = third_end.saturating_sub(third_start);
    fs::write(
        &events_path,
        &bytes[usize::try_from(third_start).expect("offset fits")..usize::try_from(third_end).expect("offset fits")],
    )
    .expect("write retained turn");

    // Legacy manifests did not persist `retained_turn_base` or `in_turn`.
    // Keep a structurally valid, rebased index so the scan must recover the
    // first retained ordinal from that index instead of restarting at turn 1.
    index["entries"] = serde_json::json!([{
        "turn_number": third_entry["turn_number"],
        "start_offset": 0,
        "end_offset": retained_len,
        "event_count": third_entry["event_count"],
        "ts": third_entry["ts"],
    }]);
    fs::write(&index_path, serde_json::to_vec(&index).expect("serialize legacy index")).expect("write legacy index");
    let manifest_path = session_dir.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("read manifest")).expect("parse manifest");
    manifest.as_object_mut().expect("manifest object").remove("retained_turn_base");
    manifest.as_object_mut().expect("manifest object").remove("in_turn");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).expect("serialize legacy manifest"))
        .expect("write legacy manifest");

    let reopened = open(dir.path(), session_id, 0).expect("reopen legacy session");
    assert_eq!(reopened.turn_count(), 3);
    assert!(reopened.reconstruct_turn(1).is_err(), "evicted ordinals must stay absent");
    assert!(reopened.reconstruct_turn(2).is_err(), "evicted ordinals must stay absent");
    assert_eq!(reopened.reconstruct_turn(3).expect("retained turn").len(), 2);
}

#[test]
fn cap_rewrite_keeps_event_log_appendable_and_reopenable() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-cap-rewrite", 2).expect("open");
    let turn = || {
        [
            ThreadEvent::TurnStarted(TurnStartedEvent::default()),
            ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }),
        ]
    };

    for event in turn().into_iter().chain(turn()) {
        log.append(&event).expect("append");
    }

    let events_path = sessions_root(dir.path()).join("sess-cap-rewrite/events.jsonl");
    assert_eq!(fs::read_to_string(&events_path).expect("read compacted log").lines().count(), 2);
    let summaries: Vec<_> = fs::read_dir(sessions_root(dir.path()).join("sess-cap-rewrite/derived"))
        .expect("read summaries")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("eviction-summary-"))
        .collect();
    assert!(!summaries.is_empty(), "evicted history must have a persisted summary");
    assert_eq!(log.reconstruct_turn(2).expect("reconstruct retained turn").len(), 2);

    drop(log);
    let reopened = open(dir.path(), "sess-cap-rewrite", 2).expect("reopen compacted log");
    assert_eq!(reopened.event_count(), 2);
    assert_eq!(reopened.reconstruct_turn(2).expect("reconstruct after reopen").len(), 2);
}

#[test]
fn cap_eviction_summary_contains_bounded_grounded_facts() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-grounded-summary", 3).expect("open");
    log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
        .expect("append turn start");
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: "message-1".into(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: "The canonical event recorded a grounded implementation fact.".into(),
            }),
        },
    }))
    .expect("append message");
    log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    }))
    .expect("complete first turn");
    log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
        .expect("append second turn start");
    log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    }))
    .expect("complete second turn");

    let derived = sessions_root(dir.path()).join("sess-grounded-summary/derived");
    let summary_path = fs::read_dir(&derived)
        .expect("read derived directory")
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("eviction-summary-"))
        .map(|entry| entry.path())
        .expect("summary file");
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(summary_path).expect("read summary")).expect("parse summary");
    assert_eq!(
        summary["grounded_facts"],
        serde_json::json!(["The canonical event recorded a grounded implementation fact."])
    );
}

#[test]
fn cap_eviction_counts_session_records_removed_with_oldest_turn() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-cap-session-record", 2).expect("open");
    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread".to_string() }))
        .expect("append thread start");
    for _ in 0..2 {
        log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
            .expect("append turn start");
        log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }))
        .expect("append turn completion");
    }

    assert_eq!(log.event_count(), 2, "eviction count must include thread.started in the removed prefix");
    assert_eq!(
        fs::read_to_string(sessions_root(dir.path()).join("sess-cap-session-record/events.jsonl"))
            .expect("read compacted event log")
            .lines()
            .count(),
        2
    );
    assert!(log.reconstruct_turn(1).is_err(), "oldest turn must be evicted");
    assert_eq!(log.reconstruct_turn(2).expect("retained turn").len(), 2);
}

#[test]
fn scan_after_cap_rewrite_crash_preserves_retained_turn_ordinals() {
    let dir = TempDir::new().expect("tempdir");
    let events_path = sessions_root(dir.path()).join("sess-cap-crash/events.jsonl");
    {
        let log = open(dir.path(), "sess-cap-crash", 0).expect("open");
        for _ in 0..3 {
            log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
                .expect("append turn start");
            log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }))
            .expect("append turn completion");
        }
        log.flush().expect("flush before simulated rewrite");
    }

    // Simulate a process dying after the atomic event-file replacement but
    // before the shortened manifest and index are published. The stale index
    // still contains the original offsets and ordinals, which the reopen path
    // must use to recover the first retained turn number.
    let index_path = sessions_root(dir.path()).join("sess-cap-crash/index/turns.json");
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(&index_path).expect("read turn index")).expect("parse turn index");
    let truncate_offset = index["entries"][0]["end_offset"].as_u64().expect("first turn end offset");
    let bytes = fs::read(&events_path).expect("read event log");
    fs::write(&events_path, &bytes[usize::try_from(truncate_offset).expect("offset fits")..])
        .expect("simulate shortened event log");

    let reopened = open(dir.path(), "sess-cap-crash", 0).expect("reopen after simulated crash");
    assert_eq!(reopened.turn_count(), 3);
    assert!(reopened.reconstruct_turn(1).is_err(), "evicted ordinal must stay absent");
    assert_eq!(reopened.reconstruct_turn(2).expect("retained turn 2").len(), 2);
    assert_eq!(reopened.reconstruct_turn(3).expect("retained turn 3").len(), 2);
}

#[test]
fn scan_after_full_cap_rewrite_crash_preserves_next_turn_ordinal() {
    let dir = TempDir::new().expect("tempdir");
    let events_path = sessions_root(dir.path()).join("sess-cap-crash-empty/events.jsonl");
    {
        let log = open(dir.path(), "sess-cap-crash-empty", 0).expect("open");
        for event in [
            ThreadEvent::TurnStarted(TurnStartedEvent::default()),
            ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }),
        ] {
            log.append(&event).expect("append");
        }
        log.flush().expect("flush before simulated rewrite");
    }

    // Simulate the atomic replacement after every indexed turn was evicted,
    // before the shortened manifest was published. The stale manifest still
    // carries the last observed turn count and must seed the next ordinal.
    fs::write(&events_path, []).expect("simulate empty rewritten event log");

    let reopened = open(dir.path(), "sess-cap-crash-empty", 0).expect("reopen after simulated crash");
    assert_eq!(reopened.turn_count(), 1);
    reopened
        .append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
        .expect("append next turn start");
    reopened
        .append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }))
        .expect("append next turn completion");
    assert_eq!(reopened.turn_count(), 2);
    assert_eq!(reopened.reconstruct_turn(2).expect("reconstruct next turn").len(), 2);
}

#[test]
fn pending_cap_rewrite_recovers_when_index_is_missing() {
    let dir = TempDir::new().expect("tempdir");
    let session_id = "sess-cap-crash-missing-index";
    let session_dir = sessions_root(dir.path()).join(session_id);
    let events_path = session_dir.join("events.jsonl");
    {
        let log = open(dir.path(), session_id, 0).expect("open");
        for _ in 0..3 {
            log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
                .expect("append turn start");
            log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }))
            .expect("append turn completion");
        }
        log.flush().expect("flush before simulated rewrite");
    }

    let index_path = session_dir.join("index/turns.json");
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(&index_path).expect("read turn index")).expect("parse turn index");
    let truncate_offset = index["entries"][0]["end_offset"].as_u64().expect("first turn end offset");
    let bytes = fs::read(&events_path).expect("read event log");
    let new_file_len = bytes
        .len()
        .saturating_sub(usize::try_from(truncate_offset).expect("offset fits"));
    fs::write(
        session_dir.join("index/pending-cap-rewrite.json"),
        serde_json::to_vec(&serde_json::json!({
            "previous_file_len": bytes.len(),
            "new_file_len": new_file_len,
            "retained_turn_base": 2,
        }))
        .expect("serialize rewrite marker"),
    )
    .expect("write rewrite marker");
    fs::write(&events_path, &bytes[usize::try_from(truncate_offset).expect("offset fits")..])
        .expect("simulate shortened event log");
    fs::remove_file(index_path).expect("remove stale index");

    let reopened = open(dir.path(), session_id, 0).expect("reopen after simulated crash");
    assert_eq!(reopened.turn_count(), 3);
    assert!(reopened.reconstruct_turn(1).is_err(), "evicted ordinal must stay absent");
    assert_eq!(reopened.reconstruct_turn(2).expect("retained turn 2").len(), 2);
    assert_eq!(reopened.reconstruct_turn(3).expect("retained turn 3").len(), 2);
    assert!(!session_dir.join("index/pending-cap-rewrite.json").exists());
}

#[test]
fn pending_cap_rewrite_recovers_when_index_is_invalid() {
    let dir = TempDir::new().expect("tempdir");
    let session_id = "sess-cap-crash-invalid-index";
    let session_dir = sessions_root(dir.path()).join(session_id);
    let events_path = session_dir.join("events.jsonl");
    {
        let log = open(dir.path(), session_id, 0).expect("open");
        for _ in 0..3 {
            log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default()))
                .expect("append turn start");
            log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }))
            .expect("append turn completion");
        }
        log.flush().expect("flush before simulated rewrite");
    }

    let index_path = session_dir.join("index/turns.json");
    let index: serde_json::Value =
        serde_json::from_slice(&fs::read(&index_path).expect("read turn index")).expect("parse turn index");
    let truncate_offset = index["entries"][0]["end_offset"].as_u64().expect("first turn end offset");
    let bytes = fs::read(&events_path).expect("read event log");
    let new_file_len = bytes
        .len()
        .saturating_sub(usize::try_from(truncate_offset).expect("offset fits"));
    fs::write(
        session_dir.join("index/pending-cap-rewrite.json"),
        serde_json::to_vec(&serde_json::json!({
            "previous_file_len": bytes.len(),
            "new_file_len": new_file_len,
            "retained_turn_base": 2,
        }))
        .expect("serialize rewrite marker"),
    )
    .expect("write rewrite marker");
    fs::write(&events_path, &bytes[usize::try_from(truncate_offset).expect("offset fits")..])
        .expect("simulate shortened event log");
    fs::write(index_path, b"{invalid index").expect("corrupt stale index");

    let reopened = open(dir.path(), session_id, 0).expect("reopen after simulated crash");
    assert_eq!(reopened.turn_count(), 3);
    assert!(reopened.reconstruct_turn(1).is_err(), "evicted ordinal must stay absent");
    assert_eq!(reopened.reconstruct_turn(2).expect("retained turn 2").len(), 2);
    assert_eq!(reopened.reconstruct_turn(3).expect("retained turn 3").len(), 2);
    assert!(!session_dir.join("index/pending-cap-rewrite.json").exists());
}

#[test]
fn multiple_open_handles_share_turn_state_and_file_lock() {
    let dir = TempDir::new().expect("tempdir");
    let first = open(dir.path(), "sess-shared-handles", 0).expect("open first handle");
    let second = open(dir.path(), "sess-shared-handles", 0).expect("open second handle");

    for event in [
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
    ] {
        first.append(&event).expect("append first turn");
    }
    for event in [
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
    ] {
        second.append(&event).expect("append second turn");
    }

    assert_eq!(first.turn_count(), 2);
    assert_eq!(second.turn_count(), 2);
    assert_eq!(first.reconstruct_turn(1).expect("first turn").len(), 2);
    assert_eq!(second.reconstruct_turn(2).expect("second turn").len(), 2);
}

#[test]
fn failed_eviction_summary_keeps_canonical_events() {
    let dir = TempDir::new().expect("tempdir");
    let hook: EvictionSummaryHook =
        Arc::new(|_| Err(SessionStoreError::io("summary", std::io::Error::other("summary backend unavailable"))));
    let log = open_with_eviction_summary(dir.path(), "sess-summary-failure", 2, hook).expect("open");
    for event in [
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
    ] {
        let _ = log.append(&event);
    }
    let error = log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
        completed_at: None,
        usage: Usage::default(),
        in_progress_exec_sessions: Vec::new(),
    }));
    assert!(error.is_err());
    let events_path = sessions_root(dir.path()).join("sess-summary-failure/events.jsonl");
    assert_eq!(fs::read_to_string(events_path).expect("read canonical log").lines().count(), 4);
    assert_eq!(log.event_count(), 4);
}

#[cfg(unix)]
#[test]
fn session_artifacts_use_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-private", DEFAULT_MAX_EVENTS).expect("open");
    for event in &sample_turn() {
        log.append(event).expect("append");
    }
    log.flush().expect("flush");

    let session_dir = sessions_root(dir.path()).join("sess-private");
    let mode = |path: &std::path::Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o777;
    assert_eq!(mode(&session_dir), 0o700);
    assert_eq!(mode(&session_dir.join("derived")), 0o700);
    assert_eq!(mode(&session_dir.join("index")), 0o700);
    assert_eq!(mode(&session_dir.join("events.jsonl")), 0o600);
    assert_eq!(mode(&session_dir.join("manifest.json")), 0o600);
    assert_eq!(mode(&session_dir.join("index/turns.json")), 0o600);
}

#[test]
fn scan_fallback_when_manifest_missing() {
    let dir = TempDir::new().expect("tempdir");
    // Write events directly to events.jsonl without manifest/index.
    let session_dir = dir.path().join(".vtcode/sessions/sess-raw");
    let events_path = session_dir.join("events.jsonl");
    fs::create_dir_all(&session_dir).expect("mkdir");
    let events = [
        VersionedThreadEvent::new(ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "t-1".to_string() })),
        VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
        VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        })),
    ];
    let lines: Vec<String> = events.iter().map(|v| serde_json::to_string(v).expect("ser")).collect();
    fs::write(&events_path, lines.join("\n") + "\n").expect("write raw events");

    let log = SessionEventLog::open(dir.path(), "sess-raw", DEFAULT_MAX_EVENTS).expect("open");
    assert_eq!(log.turn_count(), 1);
    let rebuilt = log.reconstruct_turn(1).expect("reconstruct");
    assert_eq!(rebuilt.len(), 2);
}

#[test]
fn scan_skips_malformed_lifecycle_payloads() {
    let dir = TempDir::new().expect("tempdir");
    let session_dir = dir.path().join(".vtcode/sessions/sess-invalid");
    let events_path = session_dir.join("events.jsonl");
    fs::create_dir_all(&session_dir).expect("mkdir");

    let valid_events = [
        VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
        VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        })),
    ];
    let mut lines = vec![
        r#"{"schema_version":"0.11.0","event":{"type":"thread.started","thread_id":123}}"#.to_string(),
        r#"{"schema_version":"0.11.0","event":{"type":"turn.started","token_breakdown":"invalid"}}"#.to_string(),
        r#"{"schema_version":"0.11.0","event":{"type":"turn.completed","usage":"invalid"}}"#.to_string(),
    ];
    lines.extend(
        valid_events
            .iter()
            .map(|event| serde_json::to_string(event).expect("serialize")),
    );
    fs::write(&events_path, lines.join("\n") + "\n").expect("write raw events");

    let log = SessionEventLog::open(dir.path(), "sess-invalid", DEFAULT_MAX_EVENTS).expect("open");
    assert_eq!(log.event_count(), 2);
    assert_eq!(log.turn_count(), 1);
    assert_eq!(log.reconstruct_turn(1).expect("reconstruct").len(), 2);
}

#[test]
fn retention_skips_pinned_sessions_and_evicts_when_unpinned() {
    use crate::retention::{RetentionPolicy, apply_retention, pin_session_retention, unpin_session_retention};
    use std::path::Path;

    fn write_session(root: &Path, id: &str, updated_at: &str, status: &str) {
        let dir = root.join(".vtcode/sessions").join(id);
        fs::create_dir_all(&dir).expect("session dir");
        let manifest = serde_json::json!({
            "session_id": id,
            "schema_version": 1,
            "created_at": updated_at,
            "updated_at": updated_at,
            "turn_count": 1,
            "event_count": 1,
            "status": status,
        });
        fs::write(dir.join("manifest.json"), manifest.to_string()).expect("manifest");
        fs::write(dir.join("events.jsonl"), "{}\n").expect("events");
    }

    let dir = TempDir::new().expect("temp");
    write_session(dir.path(), "sess-old", "2026-01-01T00:00:00Z", "completed");
    write_session(dir.path(), "sess-new", "2026-01-02T00:00:00Z", "completed");
    pin_session_retention(&dir.path().join(".vtcode/sessions/sess-old"), "unresolved-blocker").expect("pin");
    let policy = RetentionPolicy { max_sessions: 1, max_age_days: 365 };
    let removed = apply_retention(dir.path(), policy).expect("retention");
    assert_eq!(removed, 0, "pinned old session must not be evicted: {removed}");
    assert!(dir.path().join(".vtcode/sessions/sess-old").is_dir());
    assert!(dir.path().join(".vtcode/sessions/sess-new").is_dir());

    assert!(unpin_session_retention(&dir.path().join(".vtcode/sessions/sess-old")).expect("unpin"));
    let removed = apply_retention(dir.path(), policy).expect("retention 2");
    assert!(removed >= 1, "unpinned oldest should evict under max_sessions=1");
    assert!(!dir.path().join(".vtcode/sessions/sess-old").exists());
}

#[test]
fn retention_never_marks_a_live_open_session_abandoned() {
    // The open-but-idle eviction gap: a session left open with no completed
    // turn is a retention candidate by manifest age alone. While any
    // event-log handle exists, the liveness lock must keep it active.
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-live", DEFAULT_MAX_EVENTS).expect("open");
    let manifest_path = sessions_root(dir.path()).join("sess-live").join("manifest.json");
    let stale_manifest = |status: &str| {
        let mut manifest: crate::SessionManifest =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse");
        manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
        manifest.status = status.to_string();
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("ser")).expect("write manifest");
    };

    stale_manifest("active");
    let marked = crate::retention::mark_abandoned_active_sessions(dir.path(), 30, None).expect("mark while live");
    assert_eq!(marked, 0, "a live open session must not be marked abandoned");

    // Once every handle is closed the liveness lock releases and the idle
    // session becomes reclaimable again.
    drop(log);
    let marked = crate::retention::mark_abandoned_active_sessions(dir.path(), 30, None).expect("mark after close");
    assert_eq!(marked, 1, "a closed idle session is reclaimable");
    let manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse");
    assert_eq!(manifest.status, "completed");
}

#[test]
fn retention_eviction_skips_live_sessions_even_when_completed() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-live", DEFAULT_MAX_EVENTS).expect("open");
    log.complete().expect("complete");
    let manifest_path = sessions_root(dir.path()).join("sess-live").join("manifest.json");
    let mut manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse");
    manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("ser")).expect("write manifest");

    // A user may keep a finished session open: the liveness lock must keep
    // it out of count/age eviction while it is open.
    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 1, max_age_days: 30 })
        .expect("retain while live");
    assert_eq!(removed, 0, "a live open session must not be evicted");
    assert!(manifest_path.exists());

    drop(log);
    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 1, max_age_days: 30 })
        .expect("retain after close");
    assert_eq!(removed, 1, "a closed aged session is evictable");
    assert!(!manifest_path.exists());
}

#[test]
fn liveness_probe_held_lock_reports_live_consistently() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "sess-probe", DEFAULT_MAX_EVENTS).expect("open");
    let target = sessions_root(dir.path()).join("sess-probe");
    for _ in 0..200 {
        assert!(crate::retention::session_dir_is_live(&target), "held liveness lock must report live");
    }
    log.complete().expect("complete");
    // complete() must not release the liveness lock: the session handle is
    // still open and the user may still be interacting with it.
    for _ in 0..200 {
        assert!(
            crate::retention::session_dir_is_live(&target),
            "liveness must survive completion while the handle is open"
        );
    }
    drop(log);
    assert!(!crate::retention::session_dir_is_live(&target), "released lock must report not live");
}

#[test]
fn planning_checkpoint_survives_event_cap_and_index_rebuild() {
    use vtcode_exec_events::{
        PlanDeltaEvent,
        planning::{PlanningOutcome, PlanningRecoverySnapshot},
    };
    let workspace = TempDir::new().unwrap();
    let log = open(workspace.path(), "planning-cap", 5).unwrap();
    let recovery = PlanningRecoverySnapshot {
        outcome: PlanningOutcome::AwaitingInput,
        research_closed: true,
        requests_used: 3,
        input_prompt_shown: true,
        ..Default::default()
    };
    for turn in 0..8 {
        log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
        if turn == 0 {
            log.append(&ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
                thread_id: "planning-cap".into(),
                turn_id: "t1".into(),
                item_id: "planning-recovery".into(),
                delta: String::new(),
                recovery: Some(Box::new(recovery.clone())),
            })))
            .unwrap();
        }
        log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }))
        .unwrap();
    }
    log.flush().unwrap();
    let mut retained = None;
    log.visit_snapshot(|_, bytes| {
        if let Ok(event) = serde_json::from_slice::<VersionedThreadEvent>(bytes)
            && let ThreadEvent::PlanDelta(delta) = event.into_event()
        {
            retained = delta.recovery.map(|state| *state);
        }
    })
    .unwrap();
    assert_eq!(retained, Some(recovery));
    assert_eq!(log.turn_count(), 8);
    assert!(log.manifest().evicted_turn_count() > 0);
}
