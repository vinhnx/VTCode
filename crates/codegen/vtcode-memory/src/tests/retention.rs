use super::*;

#[test]
fn retention_evicts_zero_turn_completed_stores_immediately() {
    let dir = TempDir::new().expect("tempdir");
    // Empty shell: thread.started + thread.completed, never a turn.
    let empty = open(dir.path(), "empty-shell", DEFAULT_MAX_EVENTS).expect("open empty");
    for e in &sample_zero_turn_completion() {
        empty.append(e).expect("append empty");
    }
    empty.complete().expect("complete empty");
    drop(empty);

    // Real work: has a turn, must survive.
    let worked = open(dir.path(), "worked-session", DEFAULT_MAX_EVENTS).expect("open worked");
    for e in &sample_turn() {
        worked.append(e).expect("append");
    }
    worked.complete().expect("complete worked");
    drop(worked);

    // Generous age/count: only the empty shell is removed.
    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 50, max_age_days: 30 })
        .expect("retain");
    assert_eq!(removed, 1, "exactly the empty shell must be evicted");
    assert!(!sessions_root(dir.path()).join("empty-shell").exists());
    assert!(sessions_root(dir.path()).join("worked-session").exists());
}

#[test]
fn evict_zero_turn_completed_store_is_a_noop_for_real_sessions() {
    let dir = TempDir::new().expect("tempdir");
    let worked = open(dir.path(), "worked", DEFAULT_MAX_EVENTS).expect("open");
    for e in &sample_turn() {
        worked.append(e).expect("append");
    }
    worked.complete().expect("complete");
    drop(worked);

    assert!(!crate::retention::evict_zero_turn_completed_store(dir.path(), "worked").expect("noop"));
    assert!(sessions_root(dir.path()).join("worked").exists());

    let empty = open(dir.path(), "empty", DEFAULT_MAX_EVENTS).expect("open empty");
    for e in &sample_zero_turn_completion() {
        empty.append(e).expect("append empty");
    }
    empty.complete().expect("complete empty");
    drop(empty);
    assert!(crate::retention::evict_zero_turn_completed_store(dir.path(), "empty").expect("evict"));
    assert!(!sessions_root(dir.path()).join("empty").exists());
}

#[test]
fn evict_zero_turn_completed_store_skips_live_and_pinned() {
    let dir = TempDir::new().expect("tempdir");
    // Live: handle still held (liveness flock active).
    let live = open(dir.path(), "live-empty", DEFAULT_MAX_EVENTS).expect("open live");
    for e in &sample_zero_turn_completion() {
        live.append(e).expect("append");
    }
    live.complete().expect("complete live");
    assert!(
        !crate::retention::evict_zero_turn_completed_store(dir.path(), "live-empty").expect("live noop"),
        "a live process's store must not be evicted"
    );
    assert!(sessions_root(dir.path()).join("live-empty").exists());
    drop(live);

    // Pinned: retention pin wins even for a 0-turn completed store.
    let pinned = open(dir.path(), "pinned-empty", DEFAULT_MAX_EVENTS).expect("open pinned");
    for e in &sample_zero_turn_completion() {
        pinned.append(e).expect("append");
    }
    pinned.complete().expect("complete pinned");
    drop(pinned);
    crate::retention::pin_session_retention(&sessions_root(dir.path()).join("pinned-empty"), "test").expect("pin");
    assert!(
        !crate::retention::evict_zero_turn_completed_store(dir.path(), "pinned-empty").expect("pin noop"),
        "a pinned store must not be evicted"
    );
    assert!(sessions_root(dir.path()).join("pinned-empty").exists());
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
fn retention_pinned_session_ids_lists_only_real_pinned_dirs() {
    let dir = TempDir::new().expect("tempdir");
    let root = sessions_root(dir.path());
    fs::create_dir_all(root.join("sess-pinned")).expect("pinned session dir");
    crate::retention::pin_session_retention(&root.join("sess-pinned"), "test").expect("pin");
    fs::create_dir_all(root.join("sess-open")).expect("unpinned session dir");
    // A symlink to a pinned store is not a real session dir: enumeration
    // must not follow it.
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("sess-pinned"), root.join("sess-link")).expect("symlink");

    assert_eq!(crate::retention::retention_pinned_session_ids(dir.path()), vec!["sess-pinned".to_string()],);

    // Missing sessions root yields an empty list, not an error.
    let empty = TempDir::new().expect("tempdir");
    assert!(crate::retention::retention_pinned_session_ids(empty.path()).is_empty());
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
fn retention_preserves_active_sessions() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "active-session", DEFAULT_MAX_EVENTS).expect("open");
    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "active".to_string() }))
        .expect("append thread start");
    log.flush().expect("flush active session");

    let session_dir = sessions_root(dir.path()).join("active-session");
    let manifest_path = session_dir.join("manifest.json");
    let mut manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse manifest");
    manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("serialize manifest"))
        .expect("write manifest");

    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 0, max_age_days: 0 })
        .expect("retain");

    assert_eq!(removed, 0);
    assert!(session_dir.exists(), "active sessions must not be evicted");
}

#[test]
fn retention_preserves_recent_active_sessions_under_age_window() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "live-session", DEFAULT_MAX_EVENTS).expect("open");
    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "live".to_string() }))
        .expect("append thread start");
    log.flush().expect("flush live session");

    let session_dir = sessions_root(dir.path()).join("live-session");
    let manifest_path = session_dir.join("manifest.json");
    let mut manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse manifest");
    // Just updated: still live under a 30-day window.
    manifest.updated_at = chrono::Utc::now().to_rfc3339();
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("serialize manifest"))
        .expect("write manifest");

    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 50, max_age_days: 30 })
        .expect("retain");

    assert_eq!(removed, 0);
    assert!(session_dir.exists(), "recent active sessions must survive the age window");
}

#[test]
fn mark_abandoned_skips_preserved_session() {
    let dir = TempDir::new().expect("tempdir");
    for session_id in ["preserved-session", "other-abandoned"] {
        let log = open(dir.path(), session_id, DEFAULT_MAX_EVENTS).expect("open");
        log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: session_id.to_string() }))
            .expect("append thread start");
        log.flush().expect("flush");
        let manifest_path = sessions_root(dir.path()).join(session_id).join("manifest.json");
        let mut manifest: crate::SessionManifest =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse manifest");
        manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("serialize manifest"))
            .expect("write manifest");
    }

    let marked = crate::retention::mark_abandoned_active_sessions(dir.path(), 30, Some("preserved-session"))
        .expect("mark abandoned");

    assert_eq!(marked, 1, "only the unpreserved abandoned store is marked");
    let preserved: crate::SessionManifest = serde_json::from_str(
        &fs::read_to_string(sessions_root(dir.path()).join("preserved-session").join("manifest.json"))
            .expect("read preserved"),
    )
    .expect("parse preserved");
    assert_eq!(preserved.status, "active", "preserved session keeps active status");
    let other: crate::SessionManifest = serde_json::from_str(
        &fs::read_to_string(sessions_root(dir.path()).join("other-abandoned").join("manifest.json"))
            .expect("read other"),
    )
    .expect("parse other");
    assert_eq!(other.status, "completed");
}

#[cfg(unix)]
#[test]
fn mark_abandoned_skips_symlink_session_entries() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("tempdir");
    let outside = TempDir::new().expect("outside");
    let outside_manifest = outside.path().join("manifest.json");
    fs::write(
        &outside_manifest,
        r#"{"session_id":"evil","status":"active","updated_at":"2020-01-01T00:00:00Z","created_at":"2020-01-01T00:00:00Z","turn_count":1,"event_count":1}"#,
    )
    .expect("write outside manifest");
    let sessions = sessions_root(dir.path());
    fs::create_dir_all(&sessions).expect("sessions root");
    symlink(outside.path(), sessions.join("evil-session")).expect("symlink");

    let marked = crate::retention::mark_abandoned_active_sessions(dir.path(), 30, None).expect("mark abandoned");

    assert_eq!(marked, 0, "symlinked session entries must be skipped");
    assert!(
        fs::read_to_string(&outside_manifest)
            .expect("outside manifest")
            .contains("\"active\""),
        "must not rewrite a manifest through a planted symlink"
    );
}

#[test]
fn retention_evicts_abandoned_active_sessions_past_age_window() {
    let dir = TempDir::new().expect("tempdir");
    let log = open(dir.path(), "abandoned-session", DEFAULT_MAX_EVENTS).expect("open");
    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "abandoned".to_string() }))
        .expect("append thread start");
    log.flush().expect("flush abandoned session");
    // Drop the event-log handle so the session liveness lock is released:
    // retention must skip sessions a live process still holds open.
    drop(log);

    let session_dir = sessions_root(dir.path()).join("abandoned-session");
    let manifest_path = session_dir.join("manifest.json");
    let mut manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse manifest");
    // Crashed thread: never completed, last update far past the age window.
    manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
    fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("serialize manifest"))
        .expect("write manifest");

    let removed = apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 50, max_age_days: 30 })
        .expect("retain");

    assert!(removed >= 1, "abandoned active must become evictable: {removed}");
    assert!(!session_dir.exists(), "abandoned active store should be reclaimed");
}

#[test]
fn retention_preserves_explicit_current_session() {
    let dir = TempDir::new().expect("tempdir");
    for session_id in ["current-session", "old-session"] {
        let log = open(dir.path(), session_id, DEFAULT_MAX_EVENTS).expect("open");
        for event in &sample_turn() {
            log.append(event).expect("append lifecycle");
        }
        log.complete().expect("complete");
        let manifest_path = sessions_root(dir.path()).join(session_id).join("manifest.json");
        let mut manifest: crate::SessionManifest =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse manifest");
        manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).expect("serialize manifest"))
            .expect("write manifest");
    }

    let removed = crate::retention::apply_retention_preserving(
        dir.path(),
        crate::retention::RetentionPolicy { max_sessions: 0, max_age_days: 0 },
        Some("current-session"),
    )
    .expect("retain");

    assert_eq!(removed, 1);
    assert!(sessions_root(dir.path()).join("current-session").exists());
    assert!(!sessions_root(dir.path()).join("old-session").exists());
}

#[test]
fn retention_ignores_manifest_session_id_for_deletion_path() {
    let dir = TempDir::new().expect("tempdir");
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).expect("create outside");
    fs::write(outside.join("keep.txt"), "preserve").expect("write outside file");

    let log = open(dir.path(), "safe-session", DEFAULT_MAX_EVENTS).expect("open");
    for event in &sample_turn() {
        log.append(event).expect("append lifecycle");
    }
    log.complete().expect("complete");
    drop(log);
    let manifest_path = sessions_root(dir.path()).join("safe-session").join("manifest.json");
    let mut manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse manifest");
    manifest.session_id = "../outside".to_string();
    manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
    fs::write(&manifest_path, serde_json::to_string(&manifest).expect("serialize manifest")).expect("write manifest");

    assert_eq!(
        apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 0, max_age_days: 30 })
            .expect("retain"),
        1
    );
    assert!(outside.join("keep.txt").exists());
}

#[cfg(unix)]
#[test]
fn retention_skips_symlink_session_entries() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("tempdir");
    let target = sessions_root(dir.path()).join("target");
    let log = open(dir.path(), "target", DEFAULT_MAX_EVENTS).expect("open");
    for event in &sample_turn() {
        log.append(event).expect("append lifecycle");
    }
    log.complete().expect("complete");
    drop(log);
    let manifest_path = target.join("manifest.json");
    let mut manifest: crate::SessionManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path).expect("read manifest")).expect("parse manifest");
    manifest.updated_at = "2020-01-01T00:00:00Z".to_string();
    fs::write(&manifest_path, serde_json::to_string(&manifest).expect("serialize manifest")).expect("write manifest");
    let link = sessions_root(dir.path()).join("linked");
    symlink(&target, &link).expect("create symlink");

    apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 0, max_age_days: 30 })
        .expect("retain");
    assert!(!target.exists());
    assert!(link.exists() || fs::symlink_metadata(&link).is_ok());
}

#[cfg(unix)]
#[test]
fn retention_skips_symlink_sessions_root() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("tempdir");
    let real_root = dir.path().join("real-sessions");
    let linked_session = real_root.join("linked-session");
    fs::create_dir_all(&linked_session).expect("create linked session");
    fs::write(
        linked_session.join("manifest.json"),
        serde_json::json!({
            "session_id": "linked-session",
            "turn_count": 0,
            "event_count": 0,
            "status": "completed",
            "updated_at": "2020-01-01T00:00:00Z"
        })
        .to_string(),
    )
    .expect("write linked manifest");

    let sessions_parent = dir.path().join(".vtcode");
    fs::create_dir_all(&sessions_parent).expect("create sessions parent");
    symlink(&real_root, sessions_parent.join("sessions")).expect("create sessions root symlink");

    assert_eq!(
        apply_retention(dir.path(), crate::retention::RetentionPolicy { max_sessions: 0, max_age_days: 0 })
            .expect("retain"),
        0
    );
    assert!(linked_session.exists());
}
