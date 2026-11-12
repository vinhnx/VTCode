use std::fs;
use std::path::Path;

use super::{SessionMemoryEnvelope, write_memory_envelope_to_path, write_memory_envelope_to_path_async};

fn envelope(summary: &str) -> SessionMemoryEnvelope {
    SessionMemoryEnvelope {
        session_id: "session-write".to_string(),
        schema_version: Some(3),
        summary: summary.to_string(),
        objective: Some("Preserve committed state".to_string()),
        touched_files: vec!["src/日本.rs".to_string()],
        generated_at: "2026-10-13T00:00:00Z".to_string(),
        ..Default::default()
    }
}

#[test]
fn full_follow_up_queue_survives_envelope_write_and_restore() {
    use crate::core::agent::runtime::AgentRuntime;
    use crate::core::agent::session::AgentSessionState;
    use crate::core::agent::steering::{MAX_OVERFLOW_FOLLOW_UP_INTENTS, MAX_QUEUED_FOLLOW_UP_INTENTS};

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("session.memory.json");
    let state = AgentSessionState::new("session".into(), 8, 8, 16_000);
    let mut original = AgentRuntime::new(state, None, None);
    let capacity = MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS;
    for index in 0..capacity {
        original.try_queue_follow_up_input(format!("input-{index}")).unwrap();
    }
    original
        .split_mut()
        .1
        .try_queue_recovered_follow_up(crate::core::agent::steering::QueuedFollowUpIntent::from_parts(
            "recovered-id",
            "interrupted",
        ))
        .unwrap();
    let update = super::SessionMemoryEnvelopeUpdate {
        pending_intents: Some(original.split_mut().1.pending_follow_up_intents_snapshot()),
        ..Default::default()
    };
    let snapshot = super::build_session_memory_envelope(
        "session",
        temp.path(),
        &[],
        &[],
        String::new(),
        None,
        None,
        &super::TaskTrackerSnapshot::default(),
        Some(&update),
    );
    write_memory_envelope_to_path(&path, &snapshot).unwrap();
    let saved: SessionMemoryEnvelope = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let state = AgentSessionState::new("restored".into(), 8, 8, 16_000);
    let mut restored = AgentRuntime::new(state, None, None);
    restored
        .restore_follow_up_state(saved.pending_intents, saved.applied_intent_ids)
        .unwrap();
    for index in 0..capacity {
        assert_eq!(restored.split_mut().1.pop_follow_up_input().as_deref(), Some(format!("input-{index}").as_str()));
    }
    assert_eq!(restored.split_mut().1.pop_follow_up_input().as_deref(), Some("interrupted"));
    assert!(!restored.has_pending_follow_up_inputs());
}

fn assert_stored_envelope(path: &Path, expected: &SessionMemoryEnvelope) {
    let bytes = fs::read(path).expect("read envelope");
    let stored: SessionMemoryEnvelope = serde_json::from_slice(&bytes).expect("parse complete envelope");
    assert_eq!(&stored, expected);
    // Keep the existing pretty-printed JSON representation.
    assert_eq!(bytes, serde_json::to_string_pretty(expected).expect("legacy serialization").as_bytes());
    assert_eq!(fs::read_dir(path.parent().expect("parent")).expect("directory").count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(path).expect("metadata").permissions().mode() & 0o777, 0o600);
    }
}

#[test]
fn sync_envelope_write_creates_parent_and_replaces_existing_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("history/session.memory.json");
    let first = envelope("original short summary");
    let second = envelope("replacement summary with different content and length: 日本");

    write_memory_envelope_to_path(&path, &first).expect("first write");
    assert_stored_envelope(&path, &first);
    write_memory_envelope_to_path(&path, &second).expect("replacement");
    assert_stored_envelope(&path, &second);
}

#[tokio::test]
async fn async_envelope_write_creates_parent_and_replaces_existing_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("history/session.memory.json");
    let first = envelope("original async summary");
    let second = envelope("replacement async summary: é");

    write_memory_envelope_to_path_async(&path, &first).await.expect("first write");
    assert_stored_envelope(&path, &first);
    write_memory_envelope_to_path_async(&path, &second).await.expect("replacement");
    assert_stored_envelope(&path, &second);
}

#[tokio::test]
async fn envelope_writers_accept_bare_relative_file_names() {
    // Keep the current directory stable; the RAII path owns cleanup even when
    // an atomic replacement changes the file under this unique name.
    let temporary = tempfile::NamedTempFile::new_in(".")
        .expect("unique current-directory file")
        .into_temp_path();
    let relative = Path::new(temporary.file_name().expect("file name"));
    assert!(relative.parent().expect("relative parent").as_os_str().is_empty());
    let first = envelope("sync relative file");
    let second = envelope("async relative replacement");

    write_memory_envelope_to_path(relative, &first).expect("sync relative write");
    let stored: SessionMemoryEnvelope =
        serde_json::from_slice(&fs::read(relative).expect("read sync file")).expect("parse sync envelope");
    assert_eq!(stored, first);
    write_memory_envelope_to_path_async(relative, &second)
        .await
        .expect("async relative write");
    let stored: SessionMemoryEnvelope =
        serde_json::from_slice(&fs::read(relative).expect("read async file")).expect("parse async envelope");
    assert_eq!(stored, second);
}

#[tokio::test]
async fn envelope_writers_reject_directory_without_leaving_temporary_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("session.memory.json");
    fs::create_dir(&path).expect("destination directory");
    fs::write(path.join("retained"), b"keep directory contents").expect("retained file");
    let envelope = envelope("must not publish");

    for result in [
        write_memory_envelope_to_path(&path, &envelope),
        write_memory_envelope_to_path_async(&path, &envelope).await,
    ] {
        let error = result.expect_err("reject non-file destination");
        assert!(format!("{error:#}").contains("not a regular file"));
        assert!(format!("{error:#}").contains("write memory envelope"));
        assert_eq!(fs::read(path.join("retained")).expect("retained file"), b"keep directory contents");
        assert_eq!(fs::read_dir(temp.path()).expect("directory").count(), 1);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn envelope_writers_reject_symlink_without_changing_committed_file() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let committed = temp.path().join("committed.json");
    let path = temp.path().join("session.memory.json");
    let original = envelope("committed envelope");
    write_memory_envelope_to_path(&committed, &original).expect("committed write");
    let original_bytes = fs::read(&committed).expect("committed bytes");
    symlink(&committed, &path).expect("symlink");
    let replacement = envelope("rejected replacement");

    for result in [
        write_memory_envelope_to_path(&path, &replacement),
        write_memory_envelope_to_path_async(&path, &replacement).await,
    ] {
        assert!(format!("{:#}", result.expect_err("reject symlink")).contains("symlink"));
        assert_eq!(fs::read(&committed).expect("committed bytes"), original_bytes);
        assert!(fs::symlink_metadata(&path).expect("symlink metadata").file_type().is_symlink());
        assert_eq!(fs::read_dir(temp.path()).expect("directory").count(), 2);
    }
}

#[tokio::test]
async fn concurrent_envelope_writers_publish_one_complete_envelope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("session.memory.json");
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(4));
    let candidates: Vec<_> = (0..4)
        .map(|index| envelope(&format!("writer {index}: {}", "é".repeat(101 + index))))
        .collect();
    let mut writers = Vec::new();
    for candidate in &candidates {
        let candidate = candidate.clone();
        let path = path.clone();
        let barrier = barrier.clone();
        writers.push(tokio::spawn(async move {
            barrier.wait().await;
            write_memory_envelope_to_path_async(&path, &candidate).await
        }));
    }
    for writer in writers {
        writer.await.expect("join writer").expect("successful write");
    }
    let stored: SessionMemoryEnvelope =
        serde_json::from_slice(&fs::read(&path).expect("read envelope")).expect("complete envelope JSON");
    assert!(candidates.contains(&stored), "one complete writer result must survive");
    assert_eq!(fs::read_dir(temp.path()).expect("directory").count(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn envelope_writers_reject_symlinked_parent_without_writing_outside() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside directory");
    let parent = temp.path().join("history");
    symlink(outside.path(), &parent).expect("symlinked parent");
    let path = parent.join("session.memory.json");
    let candidate = envelope("must not escape");

    for result in [
        write_memory_envelope_to_path(&path, &candidate),
        write_memory_envelope_to_path_async(&path, &candidate).await,
    ] {
        assert!(format!("{:#}", result.expect_err("reject escaping parent")).contains("symlink"));
        assert_eq!(fs::read_dir(outside.path()).expect("outside directory").count(), 0);
        assert_eq!(fs::read_dir(temp.path()).expect("fixture directory").count(), 1);
    }
}
