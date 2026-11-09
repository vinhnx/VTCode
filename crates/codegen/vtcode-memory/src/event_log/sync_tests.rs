use super::*;
use vtcode_exec_events::{ThreadEvent, ThreadStartedEvent};

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
