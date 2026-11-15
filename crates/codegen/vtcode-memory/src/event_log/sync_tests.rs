use super::*;
use vtcode_exec_events::{ThreadEvent, ThreadStartedEvent, TurnCompletedEvent, TurnStartedEvent, Usage};

#[test]
fn failed_sync_is_retried_before_metadata_without_reappending() {
    let dir = tempfile::tempdir().unwrap();
    let log = SessionEventLog::open_exclusive(dir.path(), "sync-retry", DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "one".to_string() }))
        .unwrap();
    let bytes_after_write = {
        let mut state = log.shared.state.lock().unwrap();
        let fail_sync = |_: &File| Err(std::io::Error::other("injected sync failure"));
        assert!(log.flush_write_buf_with_sync_locked(&mut state, fail_sync).is_err());
        assert!(state.needs_sync);
        assert!(state.write_buf.is_empty());
        let bytes = std::fs::read(&log.events_path).unwrap();
        assert!(log.flush_write_buf_with_sync_locked(&mut state, fail_sync).is_err());
        assert_eq!(std::fs::read(&log.events_path).unwrap(), bytes);
        bytes
    };
    log.flush().unwrap();
    assert_eq!(std::fs::read(&log.events_path).unwrap(), bytes_after_write);
    assert!(!log.shared.state.lock().unwrap().needs_sync);
    drop(log);
    let reopened = SessionEventLog::open_exclusive(dir.path(), "sync-retry", DEFAULT_MAX_EVENTS).unwrap();
    assert_eq!(reopened.event_count(), 1);
}

#[test]
fn failed_write_and_rollback_stop_the_live_writer_until_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let log = SessionEventLog::open_exclusive(dir.path(), "write-failure", DEFAULT_MAX_EVENTS).unwrap();
    let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "first".to_string() });
    log.append(&event).unwrap();
    {
        let mut state = log.shared.state.lock().unwrap();
        let error = log
            .flush_write_buf_with_ops_locked(
                &mut state,
                |file, bytes| {
                    file.write_all(&bytes[..8])?;
                    Err(std::io::Error::other("injected write failure"))
                },
                |_, _| Err(std::io::Error::other("injected rollback failure")),
                File::sync_data,
            )
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("injected write failure") && message.contains("injected rollback failure"));
        assert!(state.write_failed);
    }
    assert!(log.flush().is_err());
    assert!(log.append(&event).is_err());
    drop(log);

    let reopened = SessionEventLog::open_exclusive(dir.path(), "write-failure", DEFAULT_MAX_EVENTS).unwrap();
    assert_eq!(reopened.event_count(), 0);
    reopened.append(&event).unwrap();
    reopened.flush().unwrap();
    drop(reopened);
    let reopened = SessionEventLog::open_exclusive(dir.path(), "write-failure", DEFAULT_MAX_EVENTS).unwrap();
    assert_eq!(reopened.event_count(), 1);
}

#[test]
fn successful_rollback_keeps_buffer_available_for_retry() {
    let dir = tempfile::tempdir().unwrap();
    let log = SessionEventLog::open_exclusive(dir.path(), "write-retry", DEFAULT_MAX_EVENTS).unwrap();
    log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "first".to_string() }))
        .unwrap();
    {
        let mut state = log.shared.state.lock().unwrap();
        assert!(
            log.flush_write_buf_with_ops_locked(
                &mut state,
                |file, bytes| {
                    file.write_all(&bytes[..8])?;
                    Err(std::io::Error::other("injected write failure"))
                },
                File::set_len,
                File::sync_data,
            )
            .is_err()
        );
        assert!(!state.write_failed);
        assert!(!state.write_buf.is_empty());
    }
    log.flush().unwrap();
    drop(log);
    assert_eq!(
        SessionEventLog::open_exclusive(dir.path(), "write-retry", DEFAULT_MAX_EVENTS)
            .unwrap()
            .event_count(),
        1
    );
}

#[test]
fn unavailable_event_file_rejects_append_and_empty_flush() {
    let dir = tempfile::tempdir().unwrap();
    let mut log = SessionEventLog::open_exclusive(dir.path(), "missing-handle", 0).unwrap();
    let events_path = log.events_path.clone();
    let blocked_path = dir.path().join("not-a-file");
    std::fs::create_dir(&blocked_path).unwrap();
    log.events_path = blocked_path;
    assert!(log.replace_event_file_contents(b"replacement\n").is_err());
    assert!(log.shared.file.lock().unwrap().is_none(), "failed reopen leaves no live handle");
    let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "first".into() });
    assert!(log.append(&event).is_err());
    assert_eq!(log.event_count(), 0, "refused append must not advance the manifest");
    assert!(log.flush().is_err(), "an empty buffer must not hide the missing file");
    log.events_path = events_path;
    *log.shared.file.lock().unwrap() = Some(log.open_event_file().unwrap());
    log.append(&event).unwrap();
    log.flush().unwrap();
    assert_eq!(log.event_count(), 1);
}

#[test]
fn failed_cap_metadata_publication_quarantines_shared_writer_until_reopen() {
    for blocked_relative in ["index/turns.json", "manifest.json"] {
        let dir = tempfile::tempdir().unwrap();
        let session = "cap-metadata-failure";
        let session_path = session_dir(dir.path(), session);
        let index_path = session_path.join(blocked_relative);
        let blocked_index = index_path.clone();
        let hook: EvictionSummaryHook = Arc::new(move |_| {
            std::fs::remove_file(&blocked_index).unwrap();
            std::fs::create_dir(&blocked_index).unwrap();
            Ok(())
        });
        let log = SessionEventLog::open_with_eviction_summary(dir.path(), session, 2, hook).unwrap();
        log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
        log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
            final_response_item_id: None,
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }))
        .unwrap();
        let uncapped = SessionEventLog::open(dir.path(), session, 0).unwrap();

        assert!(log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default())).is_err());
        let marker = session_path.join("index/pending-cap-rewrite.json");
        assert!(marker.exists());
        let rewritten = std::fs::read(session_path.join("events.jsonl")).unwrap();
        let later = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "later".into() });
        assert!(uncapped.append(&later).is_err(), "another handle must not invalidate the marker");
        assert!(uncapped.flush().is_err());
        assert_eq!(std::fs::read(session_path.join("events.jsonl")).unwrap(), rewritten);
        std::fs::remove_dir(&index_path).unwrap();
        drop(uncapped);
        drop(log);

        let recovered = SessionEventLog::open(dir.path(), session, 0).unwrap();
        assert!(!marker.exists());
        assert!(recovered.reconstruct_turn(1).is_err());
        recovered
            .append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
                final_response_item_id: None,
                completed_at: None,
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            }))
            .unwrap();
        assert_eq!(recovered.turn_count(), 2);
        assert_eq!(recovered.reconstruct_turn(2).unwrap().len(), 2);
    }
}
