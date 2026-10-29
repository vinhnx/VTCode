use crate::utils::session_archive::VerifiedSessionArchiveIdentifier;

use super::*;

#[test]
fn writes_current_and_archived_blocked_handoffs() {
    let temp = tempfile::TempDir::new().expect("temp");
    // Provide a session store so forensics/pin paths resolve.
    let session_dir = vtcode_memory::session_directory(temp.path(), "session-a");
    fs::create_dir_all(session_dir.join("derived")).expect("session dir");
    fs::write(session_dir.join("events.jsonl"), "{\"type\":\"thread.started\"}\n").expect("events");
    fs::write(session_dir.join("derived/atif-trajectory.json"), "{\"schema_version\":\"ATIF-v1.4\",\"steps\":[]}")
        .expect("atif");

    let artifacts =
        write_blocked_handoff(temp.path(), "session-a", "blocked", "first", &[]).expect("write first handoff");

    assert!(artifacts.archive_path.is_file());
    let stem = artifacts.archive_path.file_stem().unwrap().to_string_lossy().to_string();
    let forensics = artifacts
        .archive_path
        .parent()
        .expect("parent")
        .join(format!("{stem}-forensics"));
    assert!(forensics.join("events.jsonl").is_file(), "forensics events copy missing");
    assert!(forensics.join("derived__atif-trajectory.json").is_file(), "forensics ATIF copy missing");
    assert!(
        vtcode_memory::session_retention_pinned(&session_dir),
        "unresolved blocker must pin session retention"
    );

    let artifacts2 =
        write_blocked_handoff(temp.path(), "session-a", "blocked", "second", &[]).expect("write second handoff");
    assert_ne!(artifacts.archive_path, artifacts2.archive_path);
}

#[test]
fn resolution_clears_retention_pin_when_no_other_open_blocker() {
    let temp = tempfile::TempDir::new().expect("temp");
    let session_dir = vtcode_memory::session_directory(temp.path(), "session-pin");
    fs::create_dir_all(&session_dir).expect("session dir");

    let artifacts = write_blocked_handoff(temp.path(), "session-pin", "blocked", "stall", &[]).expect("write handoff");
    assert!(vtcode_memory::session_retention_pinned(&session_dir));

    clear_current_blocked_handoff_for_session(temp.path(), "session-pin").expect("resolve");
    assert!(
        !vtcode_memory::session_retention_pinned(&session_dir),
        "resolution must clear retention pin when no other unresolved archive remains"
    );
    assert!(artifacts.archive_path.is_file());
}

#[test]
fn blocked_handoff_archives_have_unique_paths() {
    let temp = tempfile::tempdir().expect("temp dir");

    let first = write_blocked_handoff(temp.path(), "session-a", "blocked", "first", &[]).expect("write first handoff");
    let second =
        write_blocked_handoff(temp.path(), "session-a", "blocked", "second", &[]).expect("write second handoff");

    assert_ne!(first.archive_path, second.archive_path);
    assert!(first.archive_path.exists());
    assert!(second.archive_path.exists());
    assert!(
        fs::read_to_string(first.archive_path)
            .expect("read first archive")
            .contains("first")
    );
}

#[test]
fn replacing_current_handoff_does_not_leave_temporary_files() {
    let temp = tempfile::tempdir().expect("temp dir");
    let tasks_dir = temp.path().join(TASKS_DIR);
    fs::create_dir_all(&tasks_dir).expect("tasks dir");
    let current = tasks_dir.join(CURRENT_BLOCKED_FILE);
    fs::write(&current, "old handoff").expect("write old handoff");

    write_handoff_file(&current, "new handoff", true).expect("replace handoff");

    assert_eq!(fs::read_to_string(&current).expect("read replacement"), "new handoff");
    assert_eq!(
        fs::read_dir(tasks_dir)
            .expect("read tasks dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".current_blocked.md."))
            .count(),
        0
    );
}

#[test]
fn async_approval_blockers_have_unique_paths() {
    let temp = tempfile::tempdir().expect("temp dir");
    let first = write_async_approval_blocker(
        temp.path(),
        "session-a",
        "approve the first request",
        "exec_command",
        &serde_json::json!({"command": "true"}),
        None,
        None,
    )
    .expect("write first approval blocker");
    let second = write_async_approval_blocker(
        temp.path(),
        "session-a",
        "approve the second request",
        "exec_command",
        &serde_json::json!({"command": "false"}),
        None,
        None,
    )
    .expect("write second approval blocker");

    assert_ne!(first.current_path, second.current_path);
    assert!(first.current_path.exists());
    assert!(second.current_path.exists());
}

#[test]
fn writes_blocked_handoff_without_resume_when_archive_is_unavailable() {
    let temp = tempfile::tempdir().expect("temp dir");

    let artifacts = write_blocked_handoff_with_resume(
        temp.path(),
        "runtime-session",
        "blocked",
        "History persistence is disabled.",
        &[temp.path().join("src/lib.rs")],
        BlockedHandoffResume::Unavailable("Resume is unavailable because the session archive was not persisted."),
        false,
    )
    .expect("write handoff");

    let current = fs::read_to_string(&artifacts.current_path).expect("current handoff");
    assert!(!current.contains("resume_command:"));
    assert!(!current.contains("vtcode --resume"));
    assert!(current.contains("Resume is unavailable because the session archive was not persisted."));
}

#[test]
fn uses_verified_archive_identifier_for_resume_command() {
    let temp = tempfile::tempdir().expect("temp dir");

    let verified_identifier = VerifiedSessionArchiveIdentifier("session-archive-id".to_owned());
    let artifacts = write_blocked_handoff_with_resume(
        temp.path(),
        "runtime-session",
        "blocked",
        "Execution stalled on a loop.",
        &[],
        BlockedHandoffResume::Available(&verified_identifier),
        false,
    )
    .expect("write handoff");

    let current = fs::read_to_string(&artifacts.current_path).expect("current handoff");
    assert!(current.contains("vtcode --resume session-archive-id"));
    assert!(!current.contains("vtcode --resume runtime-session"));
}

#[test]
fn planning_handoff_carries_mode_guidance_in_actionable_steps() {
    let temp = tempfile::tempdir().expect("temp dir");

    let planning = write_blocked_handoff_with_resume(
        temp.path(),
        "plan-session",
        "blocked",
        "Mutation blocked until verification: 1 mutating command(s) await a verifier.",
        &[],
        BlockedHandoffResume::Unavailable("Resume unavailable in this test."),
        true,
    )
    .expect("write planning handoff");
    let planning_content = fs::read_to_string(&planning.current_path).expect("planning handoff");
    assert!(planning_content.contains("Plan mode was active (read-only)"));
    assert!(planning_content.contains("/mode build"));
    // Both the live pointer and the archive carry the same markdown.
    let planning_archive = fs::read_to_string(&planning.archive_path).expect("planning archive");
    assert!(planning_archive.contains("Plan mode was active (read-only)"));

    let non_planning = write_blocked_handoff_with_resume(
        temp.path(),
        "build-session",
        "blocked",
        "Mutation blocked until verification: 1 mutating command(s) await a verifier.",
        &[],
        BlockedHandoffResume::Unavailable("Resume unavailable in this test."),
        false,
    )
    .expect("write non-planning handoff");
    let non_planning_content = fs::read_to_string(&non_planning.current_path).expect("non-planning handoff");
    assert!(!non_planning_content.contains("Plan mode was active (read-only)"));
}

#[test]
fn write_async_approval_blocker_creates_file_with_token() {
    let temp = tempfile::tempdir().expect("temp dir");
    let tasks_dir = temp.path().join(".vtcode/tasks");
    fs::create_dir_all(&tasks_dir).expect("tasks dir");

    let artifacts = write_async_approval_blocker(
        temp.path(),
        "session-456",
        "Push 50 commits to main?",
        "git_push",
        &serde_json::json!({"force": true, "branch": "main"}),
        Some(0.50),
        Some("/usr/local/bin/notify"),
    )
    .expect("write async blocker");

    assert!(!artifacts.approval_token.is_empty());
    assert!(artifacts.current_path.exists());

    let content = fs::read_to_string(&artifacts.current_path).expect("read blocker");
    assert!(content.contains("Push 50 commits to main?"));
    assert!(content.contains("git_push"));
    assert!(content.contains("Estimated cost: $0.50"));
    assert!(content.contains("vtcode approve"));
    assert!(content.contains(&artifacts.approval_token));
}

#[test]
fn write_async_approval_blocker_handles_minimal_input() {
    let temp = tempfile::tempdir().expect("temp dir");
    let tasks_dir = temp.path().join(".vtcode/tasks");
    fs::create_dir_all(&tasks_dir).expect("tasks dir");

    let artifacts = write_async_approval_blocker(
        temp.path(),
        "session-789",
        "Delete the file?",
        "delete_file",
        &serde_json::json!({"path": "/tmp/x"}),
        None,
        None,
    )
    .expect("write async blocker");

    assert!(!artifacts.approval_token.is_empty());
    assert!(artifacts.current_path.exists());

    let content = fs::read_to_string(&artifacts.current_path).expect("read blocker");
    assert!(content.contains("Delete the file?"));
    assert!(content.contains("delete_file"));
    // No cost or notify section
    assert!(!content.contains("Estimated cost:"));
    assert!(!content.contains("Notify command:"));
}

#[test]
fn test_read_and_clear_current_blocked_handoff() {
    let temp = tempfile::tempdir().expect("temp dir");

    // When file does not exist
    assert_eq!(read_current_blocked_handoff(temp.path()), None);
    assert!(!clear_current_blocked_handoff(temp.path()).unwrap());

    // Write a blocked handoff
    let verified_identifier = VerifiedSessionArchiveIdentifier("session-archive-id".to_owned());
    let _artifacts = write_blocked_handoff_with_resume(
        temp.path(),
        "test-session-123",
        "blocked",
        "Tool call failed repeatedly with permission errors.",
        &[],
        BlockedHandoffResume::Available(&verified_identifier),
        false,
    )
    .expect("write handoff");

    // Read it back
    let info = read_current_blocked_handoff(temp.path()).expect("read info");
    assert_eq!(info.session_id, "test-session-123");
    assert_eq!(info.outcome_code, "blocked");
    assert_eq!(info.blocker_summary, "Tool call failed repeatedly with permission errors.");
    assert!(info.created_at.is_some());
    assert_eq!(info.resume_command.as_deref(), Some("vtcode --resume session-archive-id"));

    // Clear it
    assert!(clear_current_blocked_handoff(temp.path()).unwrap());
    // Should no longer exist
    assert_eq!(read_current_blocked_handoff(temp.path()), None);
    assert!(!clear_current_blocked_handoff(temp.path()).unwrap());
}

#[test]
fn session_scoped_clear_preserves_another_sessions_handoff() {
    let temp = tempfile::tempdir().expect("temp dir");
    let artifacts = write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).expect("write handoff");

    assert!(!clear_current_blocked_handoff_for_session(temp.path(), "session-b").expect("scoped clear"));
    assert_eq!(read_current_blocked_handoff(temp.path()).map(|info| info.session_id), Some("session-a".to_string()));
    assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").expect("scoped clear"));
    assert_eq!(read_current_blocked_handoff(temp.path()), None);
    let archive = fs::read_to_string(artifacts.archive_path).expect("read resolved archive");
    assert!(archive.contains("# Resolution"));
    assert!(archive.contains("resolved_by_session: session-a"));
    assert!(
        archive.contains("resolution_scope: blocked turn recovered; task may remain incomplete"),
        "resolution states turn scope, not task completion"
    );
}

#[test]
fn session_scoped_clear_preserves_malformed_handoff() {
    let temp = tempfile::tempdir().expect("temp dir");
    let tasks_dir = temp.path().join(TASKS_DIR);
    fs::create_dir_all(&tasks_dir).expect("tasks dir");
    let current = tasks_dir.join(CURRENT_BLOCKED_FILE);
    fs::write(&current, "not a handoff").expect("write malformed handoff");

    assert!(!clear_current_blocked_handoff_for_session(temp.path(), "session-a").expect("scoped clear"));
    assert!(current.exists());
}

#[test]
fn session_scoped_clear_rejects_archive_path_traversal() {
    let temp = tempfile::tempdir().expect("temp dir");
    let tasks_dir = temp.path().join(TASKS_DIR);
    fs::create_dir_all(&tasks_dir).expect("tasks dir");
    let current = tasks_dir.join(CURRENT_BLOCKED_FILE);
    fs::write(
        &current,
        "---\nsession_id: session-a\noutcome: blocked\narchive_file: ../../outside.md\n---\n\n# Blocker Summary\n\nstalled\n",
    )
    .expect("write traversal handoff");

    assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").is_err());
    assert!(current.exists());
    assert!(!temp.path().join("outside.md").exists());
}

#[cfg(any(unix, windows))]
#[test]
fn session_scoped_clear_rejects_hardlinked_archive() {
    let temp = tempfile::tempdir().expect("temp dir");
    let artifacts = write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).expect("write handoff");
    let external = temp.path().join("outside.md");
    fs::copy(&artifacts.archive_path, &external).expect("copy archive");
    fs::remove_file(&artifacts.archive_path).expect("remove original archive");
    fs::hard_link(&external, &artifacts.archive_path).expect("create hard link");

    assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").is_err());
    assert!(read_current_blocked_handoff(temp.path()).is_some());
    assert!(
        !fs::read_to_string(external)
            .expect("read external file")
            .contains("# Resolution")
    );
}

#[cfg(any(unix, windows))]
#[test]
fn write_blocked_handoff_rejects_hardlinked_current_pointer() {
    let temp = tempfile::tempdir().expect("workspace temp dir");
    let tasks_dir = temp.path().join(TASKS_DIR);
    fs::create_dir_all(&tasks_dir).expect("tasks dir");
    let external = temp.path().join("external.md");
    fs::write(&external, "must remain unchanged").expect("external file");
    fs::hard_link(&external, tasks_dir.join(CURRENT_BLOCKED_FILE)).expect("hard link current pointer");

    assert!(write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).is_err());
    assert_eq!(fs::read_to_string(external).expect("read external file"), "must remain unchanged");
}

#[cfg(unix)]
#[test]
fn session_scoped_clear_rejects_blockers_directory_outside_workspace() {
    let temp = tempfile::tempdir().expect("temp dir");
    let artifacts = write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).expect("write handoff");
    let blockers_dir = temp.path().join(TASKS_DIR).join(BLOCKERS_DIR);
    let outside_temp = tempfile::tempdir().expect("outside temp dir");
    let outside_dir = outside_temp.path().join("outside-blockers");
    fs::create_dir(&outside_dir).expect("outside directory");
    let outside_archive = outside_dir.join(artifacts.archive_path.file_name().expect("archive file name"));
    fs::rename(&artifacts.archive_path, &outside_archive).expect("move archive outside");
    fs::remove_dir(&blockers_dir).expect("remove blockers directory");
    std::os::unix::fs::symlink(&outside_dir, &blockers_dir).expect("link blockers directory");

    assert!(clear_current_blocked_handoff_for_session(temp.path(), "session-a").is_err());
    assert!(read_current_blocked_handoff(temp.path()).is_some());
    assert!(
        !fs::read_to_string(outside_archive)
            .expect("read outside archive")
            .contains("# Resolution")
    );
}

#[cfg(unix)]
#[test]
fn handoff_paths_reject_symlinked_vtcode_parent() {
    let temp = tempfile::tempdir().expect("workspace temp dir");
    let outside = tempfile::tempdir().expect("outside temp dir");
    let outside_tasks = outside.path().join(TASKS_DIR);
    fs::create_dir_all(&outside_tasks).expect("outside tasks dir");
    std::os::unix::fs::symlink(outside.path().join(".vtcode"), temp.path().join(".vtcode"))
        .expect("symlink vtcode parent");

    assert!(write_blocked_handoff(temp.path(), "session-a", "blocked", "stalled", &[]).is_err());
    assert!(
        write_async_approval_blocker(
            temp.path(),
            "session-a",
            "approve this",
            "exec_command",
            &serde_json::json!({}),
            None,
            None,
        )
        .is_err()
    );
    assert!(clear_current_blocked_handoff(temp.path()).is_err());
    assert!(!outside_tasks.join(CURRENT_BLOCKED_FILE).exists());
}

#[test]
fn restoring_claim_does_not_overwrite_concurrent_handoff() {
    let temp = tempfile::tempdir().expect("temp dir");
    let current = temp.path().join(CURRENT_BLOCKED_FILE);
    let claim = temp.path().join(".current_blocked.md.claim");
    fs::write(&claim, "session-a").expect("write claim");
    fs::write(&current, "session-b").expect("write replacement");

    restore_claim_without_overwrite(&claim, &current).expect("restore without overwrite");

    assert_eq!(fs::read_to_string(&current).expect("read current"), "session-b");
    assert!(!claim.exists());
}
