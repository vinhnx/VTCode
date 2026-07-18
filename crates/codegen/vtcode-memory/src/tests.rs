//! Tests for the unified session store.

use std::fs;

use tempfile::TempDir;
use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, TurnStartedEvent, Usage, VersionedThreadEvent};

use crate::event_log::{DEFAULT_MAX_EVENTS, SessionEventLog};
use crate::migration::migrate_legacy;
use crate::query::{query_facts, recent_sessions};
use crate::{open, retention::apply_retention, sessions_root};

fn sample_turn() -> Vec<ThreadEvent> {
    vec![
        ThreadEvent::TurnStarted(TurnStartedEvent::default()),
        ThreadEvent::TurnCompleted(TurnCompletedEvent { usage: Usage::default() }),
    ]
}

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

#[test]
fn retention_removes_oldest_sessions() {
    let dir = TempDir::new().expect("tempdir");
    // Create 3 old sessions (2020) and 2 recent sessions (today).
    for i in 0..5u64 {
        let log = open(dir.path(), &format!("sess-{i}"), DEFAULT_MAX_EVENTS).expect("open");
        for e in &sample_turn() {
            log.append(e).expect("append");
        }
        log.complete().expect("complete");
        let mpath = sessions_root(dir.path()).join(format!("sess-{i}")).join("manifest.json");
        let mut m: crate::SessionManifest =
            serde_json::from_str(&fs::read_to_string(&mpath).expect("read manifest")).expect("parse");
        // First 3 are old (2020), last 2 keep today's timestamp.
        if i < 3 {
            m.updated_at = format!("2020-01-{:02}T00:00:00Z", i + 1);
            fs::write(&mpath, serde_json::to_string_pretty(&m).expect("ser")).expect("write manifest");
        }
    }

    // max_sessions=4: count-based eviction removes 1 oldest (sess-0).
    // max_age_days=30: age-based eviction removes 2 more old sessions (sess-1, sess-2).
    // Total: 3 removed, 2 recent remain.
    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 4, max_age_days: 30 })
        .expect("retain");
    assert_eq!(removed, 3);
    let remaining = recent_sessions(dir.path(), 100);
    assert_eq!(remaining.len(), 2);
}

#[test]
fn retention_evicts_old_sessions_even_when_under_count_cap() {
    let dir = TempDir::new().expect("tempdir");
    // Create 3 sessions: 1 old (2020) and 2 recent (today).
    for i in 0..3u64 {
        let log = open(dir.path(), &format!("sess-{i}"), DEFAULT_MAX_EVENTS).expect("open");
        for e in &sample_turn() {
            log.append(e).expect("append");
        }
        log.complete().expect("complete");
        if i == 0 {
            let mpath = sessions_root(dir.path()).join("sess-0").join("manifest.json");
            let mut m: crate::SessionManifest =
                serde_json::from_str(&fs::read_to_string(&mpath).expect("read manifest")).expect("parse");
            m.updated_at = "2020-01-01T00:00:00Z".to_string();
            fs::write(&mpath, serde_json::to_string_pretty(&m).expect("ser")).expect("write manifest");
        }
    }

    // max_sessions=10: count cap is not exceeded (3 < 10).
    // max_age_days=30: age-based eviction should still remove sess-0.
    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 10, max_age_days: 30 })
        .expect("retain");
    assert_eq!(removed, 1);
    let remaining = recent_sessions(dir.path(), 100);
    assert_eq!(remaining.len(), 2);
}

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
fn scan_fallback_when_manifest_missing() {
    let dir = TempDir::new().expect("tempdir");
    // Write events directly to events.jsonl without manifest/index.
    let session_dir = dir.path().join(".vtcode/sessions/sess-raw");
    let events_path = session_dir.join("events.jsonl");
    fs::create_dir_all(&session_dir).expect("mkdir");
    let events = vec![
        VersionedThreadEvent::new(ThreadEvent::ThreadStarted(vtcode_exec_events::ThreadStartedEvent {
            thread_id: "t-1".to_string(),
        })),
        VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
        VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent { usage: Usage::default() })),
    ];
    let lines: Vec<String> = events.iter().map(|v| serde_json::to_string(v).expect("ser")).collect();
    fs::write(&events_path, lines.join("\n") + "\n").expect("write raw events");

    let log = SessionEventLog::open(dir.path(), "sess-raw", DEFAULT_MAX_EVENTS).expect("open");
    assert_eq!(log.turn_count(), 1);
    let rebuilt = log.reconstruct_turn(1).expect("reconstruct");
    assert_eq!(rebuilt.len(), 2);
}
