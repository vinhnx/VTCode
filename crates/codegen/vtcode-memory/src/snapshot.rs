//! Bounded read-only projections of canonical history for external clients.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use vtcode_exec_events::VersionedThreadEvent;

use crate::SessionStoreError;

type Result<T> = std::result::Result<T, SessionStoreError>;

/// Complete records from the end of a canonical event log, in persisted order.
#[derive(Debug)]
pub struct EventTail {
    /// Schema-versioned canonical records; never synthesized lifecycle events.
    pub events: Vec<VersionedThreadEvent>,
    /// The byte limit or an incomplete final record excluded history.
    pub truncated: bool,
}

/// Read at most `max_bytes` from a session without creating files, taking a
/// writer handle, changing metadata, or loading history into agent context.
/// Missing logs return `None` for compatibility with legacy archives.
/// The workspace must be an absolute, resolved root; symlink paths are rejected.
/// Platforms without handle-bound reads return an `Unsupported` I/O error.
pub fn read_event_tail(workspace: &Path, session_id: &str, max_bytes: usize) -> Result<Option<EventTail>> {
    if max_bytes == 0
        || max_bytes > 16 * 1024 * 1024
        || session_id.is_empty()
        || session_id != crate::sanitize_id(session_id)
    {
        return Err(SessionStoreError::io(
            workspace,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid session identifier or event tail limit"),
        ));
    }
    let relative = Path::new(crate::SESSIONS_DIR).join(session_id).join("events.jsonl");
    let mut file = match vtcode_commons::fs::bound_file::open_file_beneath(workspace, &relative) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SessionStoreError::io(workspace.join(&relative), error)),
    };
    let length = file
        .metadata()
        .map_err(|error| SessionStoreError::io(workspace.join(&relative), error))?
        .len();
    let start = length.saturating_sub(max_bytes as u64);
    let partial_prefix = if start > 0 {
        file.seek(SeekFrom::Start(start - 1))
            .map_err(|error| SessionStoreError::io(workspace.join(&relative), error))?;
        let mut preceding = [0];
        file.read_exact(&mut preceding)
            .map_err(|error| SessionStoreError::io(workspace.join(&relative), error))?;
        preceding[0] != b'\n'
    } else {
        false
    };
    file.seek(SeekFrom::Start(start))
        .map_err(|error| SessionStoreError::io(workspace.join(&relative), error))?;
    let mut bytes = Vec::new();
    file.take(length - start)
        .read_to_end(&mut bytes)
        .map_err(|error| SessionStoreError::io(workspace.join(&relative), error))?;
    decode_tail(&bytes, start > 0, partial_prefix).map(Some)
}

fn decode_tail(bytes: &[u8], omitted_prefix: bool, partial_prefix: bool) -> Result<EventTail> {
    let mut records = bytes.split_inclusive(|byte| *byte == b'\n');
    // A bounded tail can begin inside a JSON record. Discard that entire record
    // rather than treating the suffix as authoritative evidence.
    if partial_prefix {
        records.next();
    }
    let mut tail = EventTail { events: Vec::new(), truncated: omitted_prefix };
    for record in records {
        if !record.ends_with(b"\n") {
            tail.truncated = true;
            break;
        }
        if record.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        tail.events.push(serde_json::from_slice(record)?);
    }
    Ok(tail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_exec_events::{ThreadEvent, ThreadStartedEvent};

    fn record(id: &str) -> Vec<u8> {
        let mut bytes =
            serde_json::to_vec(&VersionedThreadEvent::new(ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: id.into(),
            })))
            .unwrap();
        bytes.push(b'\n');
        bytes
    }

    #[test]
    fn tail_preserves_order_and_excludes_partial_records_at_both_edges() {
        let mut bytes = record("first");
        bytes.extend(record("second"));
        bytes.extend(b"{unfinished");
        let full = decode_tail(&bytes, false, false).unwrap();
        assert_eq!(full.events.len(), 2);
        assert!(full.truncated);
        let bounded = decode_tail(&bytes[5..], true, true).unwrap();
        assert_eq!(bounded.events, vec![serde_json::from_slice::<VersionedThreadEvent>(&record("second")).unwrap()]);
        assert!(bounded.truncated);
        assert!(decode_tail(b"bad\n", false, false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn read_is_bounded_and_missing_history_does_not_create_a_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = vtcode_commons::canonicalize(dir.path()).unwrap();
        assert!(read_event_tail(&root, "missing", 1024).unwrap().is_none());
        assert!(!root.join(".vtcode").exists());
        let log = crate::open(&root, "saved", crate::DEFAULT_MAX_EVENTS).unwrap();
        log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "first".into() }))
            .unwrap();
        log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "second".into() }))
            .unwrap();
        log.flush().unwrap();
        let path = crate::session_directory(&root, "saved").join("events.jsonl");
        let before = std::fs::read(&path).unwrap();
        let tail = read_event_tail(&root, "saved", record("second").len()).unwrap().unwrap();
        assert_eq!(tail.events.len(), 1);
        assert!(tail.truncated);
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert!(read_event_tail(&root, "../escape", 1024).is_err());
        assert!(read_event_tail(&root, "saved", 0).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn history_rejects_symlinked_session_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = vtcode_commons::canonicalize(dir.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.join(".vtcode/sessions")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join(".vtcode/sessions/linked")).unwrap();
        assert!(read_event_tail(&root, "linked", 1024).is_err());
    }
}
