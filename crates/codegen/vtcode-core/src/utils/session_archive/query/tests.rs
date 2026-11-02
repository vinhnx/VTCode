use super::{
    delete_session_archive, find_session_by_id_or_name, list_recent_session_summaries_in_workspace,
    list_recent_sessions, list_recent_sessions_in_workspace, resolve_sessions_dir_for_archive_writes,
};
use crate::llm::provider::MessageRole;
use crate::utils::session_archive::{
    SessionArchiveMetadata, SessionListing, SessionMessage, SessionSnapshot, clear_sessions_dir_override_for_tests,
    override_sessions_dir_for_tests,
};
use chrono::Utc;
use tempfile::TempDir;

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn archive_directory_resolution_does_not_prune_session_history() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    for index in 0..101 {
        std::fs::write(sessions_dir.path().join(format!("session-{index:03}.json")), b"{}")
            .expect("seed session archive");
    }

    let resolved = resolve_sessions_dir_for_archive_writes()
        .await
        .expect("resolve archive directory");
    let archive_count = std::fs::read_dir(&resolved)
        .expect("read archive directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|extension| extension.to_str()) == Some("json"))
        .count();

    clear_sessions_dir_override_for_tests();
    assert_eq!(archive_count, 101, "directory setup should not run retention scans");
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn workspace_session_limit_ignores_newer_archives_from_other_workspaces() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());

    let workspace = sessions_dir.path().join("workspace");
    let other_workspace = sessions_dir.path().join("other-workspace");
    let now = Utc::now();
    for index in 0..12 {
        let session_workspace = if index < 9 { &other_workspace } else { &workspace };
        let ended_at = now - chrono::Duration::seconds(index);
        let snapshot = SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "workspace",
                session_workspace.display().to_string(),
                "model",
                "provider",
                "theme",
                "medium",
            ),
            started_at: ended_at,
            ended_at,
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, format!("message {index}"))],
            progress: None,
            error_logs: Vec::new(),
        };
        let path = sessions_dir.path().join(format!("session-{index:02}.json"));
        std::fs::write(path, serde_json::to_vec(&snapshot).expect("serialize session")).expect("write session archive");
    }

    let sessions = list_recent_sessions_in_workspace(2, &workspace)
        .await
        .expect("list workspace sessions");
    assert_eq!(
        sessions
            .iter()
            .map(|session| session.path.file_stem().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        ["session-09", "session-10"]
    );

    let summaries = list_recent_session_summaries_in_workspace(2, &workspace)
        .await
        .expect("list workspace session summaries");
    assert_eq!(
        summaries.iter().map(|session| session.identifier.as_str()).collect::<Vec<_>>(),
        ["session-09", "session-10"]
    );
    assert_eq!(summaries[0].title.as_deref(), Some("message 9"));
    assert_eq!(summaries[0].workspace_path, workspace.display().to_string());
    assert!(summaries[0].completed);
    assert!(summaries[0].resumable);
    clear_sessions_dir_override_for_tests();
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn session_summaries_carry_size_and_recency() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    let workspace = sessions_dir.path().join("workspace");
    let now = Utc::now();
    for (index, age_seconds, padding) in [(0, 90, "x"), (1, 10, "xxxxxxxxxx")] {
        let ended_at = now - chrono::Duration::seconds(age_seconds);
        let snapshot = SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "workspace",
                workspace.display().to_string(),
                "model",
                "provider",
                "theme",
                "medium",
            ),
            started_at: ended_at,
            ended_at,
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: vec![padding.to_string()],
            messages: vec![SessionMessage::new(MessageRole::User, format!("message {index}"))],
            progress: None,
            error_logs: Vec::new(),
        };
        let path = sessions_dir.path().join(format!("session-{index:02}.json"));
        std::fs::write(&path, serde_json::to_vec(&snapshot).expect("serialize session"))
            .expect("write session archive");
    }

    let summaries = list_recent_session_summaries_in_workspace(10, &workspace)
        .await
        .expect("list workspace session summaries");
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].identifier, "session-01");
    assert_eq!(summaries[1].identifier, "session-00");
    for summary in &summaries {
        let expected = std::fs::metadata(sessions_dir.path().join(format!("{}.json", summary.identifier)))
            .expect("stat seeded archive")
            .len();
        assert_eq!(summary.size_bytes, expected, "{}", summary.identifier);
    }
    assert!(summaries[0].size_bytes > summaries[1].size_bytes);
    assert_eq!(summaries[0].updated_at, u64::try_from((now - chrono::Duration::seconds(10)).timestamp()).unwrap());
    assert_eq!(summaries[1].updated_at, u64::try_from((now - chrono::Duration::seconds(90)).timestamp()).unwrap());
    assert!(summaries[0].updated_at > summaries[1].updated_at);
    clear_sessions_dir_override_for_tests();
}

fn seed_snapshot(workspace: &std::path::Path, ended_at: chrono::DateTime<Utc>, message: &str) -> SessionSnapshot {
    SessionSnapshot {
        metadata: SessionArchiveMetadata::new(
            "workspace",
            workspace.display().to_string(),
            "model",
            "provider",
            "theme",
            "medium",
        ),
        started_at: ended_at,
        ended_at,
        total_messages: 1,
        distinct_tools: Vec::new(),
        transcript: Vec::new(),
        messages: vec![SessionMessage::new(MessageRole::User, message)],
        progress: None,
        error_logs: Vec::new(),
    }
}

fn write_snapshot(sessions_dir: &TempDir, file_name: &str, snapshot: &SessionSnapshot) {
    std::fs::write(sessions_dir.path().join(file_name), serde_json::to_vec(snapshot).expect("serialize session"))
        .expect("write session archive");
}

fn listing_stems(sessions: &[SessionListing]) -> Vec<String> {
    sessions
        .iter()
        .map(|session| session.path.file_stem().unwrap().to_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn workspace_listing_backfills_limit_past_unparseable_archives() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    let workspace = sessions_dir.path().join("workspace");
    let now = Utc::now();

    // Newest file: a valid header with a mistyped body the full parse rejects.
    let mut divergent = serde_json::to_value(seed_snapshot(&workspace, now, "divergent")).expect("serialize divergent");
    divergent["transcript"] = serde_json::json!("not-an-array");
    std::fs::write(
        sessions_dir.path().join("session-divergent.json"),
        serde_json::to_vec(&divergent).expect("serialize divergent"),
    )
    .expect("write divergent archive");
    // Corrupt file: not JSON at all, so even the header parse fails.
    std::fs::write(sessions_dir.path().join("session-corrupt.json"), b"{not json").expect("write corrupt archive");

    for (index, age_seconds) in [(0, 10), (1, 20)] {
        let ended_at = now - chrono::Duration::seconds(age_seconds);
        write_snapshot(&sessions_dir, &format!("session-{index:02}.json"), &seed_snapshot(&workspace, ended_at, "ok"));
    }

    // A naive truncate-then-parse would return one entry here; backfilling
    // from the next-ranked headers must still fill the limit.
    let sessions = list_recent_sessions_in_workspace(2, &workspace)
        .await
        .expect("list workspace sessions");
    assert_eq!(listing_stems(&sessions), ["session-00", "session-01"]);

    let unfiltered = list_recent_sessions(10).await.expect("list all sessions");
    assert_eq!(listing_stems(&unfiltered), ["session-00", "session-01"]);

    clear_sessions_dir_override_for_tests();
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn workspace_listing_matches_non_normalized_query_paths() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    let workspace = sessions_dir.path().join("workspace");
    let other_workspace = sessions_dir.path().join("other-workspace");
    let now = Utc::now();
    write_snapshot(&sessions_dir, "session-00.json", &seed_snapshot(&workspace, now, "hello"));

    let canonical = list_recent_sessions_in_workspace(10, &workspace)
        .await
        .expect("list canonical workspace");
    assert_eq!(listing_stems(&canonical), ["session-00"]);

    // `.`/`..` segments and trailing separators must not change the match set.
    for query in [
        workspace.join("sub").join(".."),
        workspace.join("."),
        std::path::PathBuf::from(format!("{}/", workspace.display())),
    ] {
        let sessions = list_recent_sessions_in_workspace(10, &query)
            .await
            .expect("list non-normalized workspace");
        assert_eq!(listing_stems(&sessions), ["session-00"], "query: {}", query.display());
    }

    let foreign = list_recent_sessions_in_workspace(10, &other_workspace)
        .await
        .expect("list other workspace");
    assert!(foreign.is_empty());

    clear_sessions_dir_override_for_tests();
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn find_session_by_id_or_name_exact_substring_and_missing() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    let workspace = sessions_dir.path().join("workspace");
    let now = Utc::now();
    write_snapshot(
        &sessions_dir,
        "session-alpha-old.json",
        &seed_snapshot(&workspace, now - chrono::Duration::seconds(30), "old alpha"),
    );
    write_snapshot(
        &sessions_dir,
        "session-alpha-new.json",
        &seed_snapshot(&workspace, now - chrono::Duration::seconds(5), "new alpha"),
    );
    write_snapshot(
        &sessions_dir,
        "session-beta.json",
        &seed_snapshot(&workspace, now - chrono::Duration::seconds(10), "beta"),
    );

    let exact = find_session_by_id_or_name("session-beta").await.expect("exact match");
    assert_eq!(exact.path.file_stem().unwrap().to_str().unwrap(), "session-beta");

    // Substring search is case-insensitive and prefers the newest match.
    let substring = find_session_by_id_or_name("ALPHA").await.expect("substring match");
    assert_eq!(substring.path.file_stem().unwrap().to_str().unwrap(), "session-alpha-new");
    assert!(!substring.snapshot.messages.is_empty(), "winner must be fully parsed");

    assert!(find_session_by_id_or_name("no-such-session-xyz").await.is_err());

    clear_sessions_dir_override_for_tests();
}

#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn delete_session_archive_removes_only_the_named_file() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    for index in [0, 1] {
        std::fs::write(sessions_dir.path().join(format!("session-{index:02}.json")), b"{}").expect("seed archive");
    }

    assert!(delete_session_archive("session-00").await.expect("delete archive"));
    assert!(!sessions_dir.path().join("session-00.json").exists());
    assert!(sessions_dir.path().join("session-01.json").exists());
    assert!(!delete_session_archive("session-00").await.expect("delete missing archive"));
    assert!(delete_session_archive("../session-01").await.is_err());
    assert!(delete_session_archive("session 01").await.is_err());
    assert!(sessions_dir.path().join("session-01.json").exists());
    clear_sessions_dir_override_for_tests();
}

#[cfg(unix)]
#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn delete_session_archive_refuses_symlinks() {
    let sessions_dir = TempDir::new().expect("sessions directory");
    override_sessions_dir_for_tests(sessions_dir.path());
    let target = sessions_dir.path().join("target.json");
    std::fs::write(&target, b"{}").expect("seed target");
    std::os::unix::fs::symlink(&target, sessions_dir.path().join("linked.json")).expect("seed symlink");

    assert!(!delete_session_archive("linked").await.expect("delete symlink"));
    assert!(target.exists());
    clear_sessions_dir_override_for_tests();
}
