use super::{
    list_recent_session_summaries_in_workspace, list_recent_sessions_in_workspace,
    resolve_sessions_dir_for_archive_writes,
};
use crate::llm::provider::MessageRole;
use crate::utils::session_archive::{
    SessionArchiveMetadata, SessionMessage, SessionSnapshot, clear_sessions_dir_override_for_tests,
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
