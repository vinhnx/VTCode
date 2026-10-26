use super::list_recent_sessions_in_workspace;
use crate::llm::provider::MessageRole;
use crate::utils::session_archive::{
    SessionArchiveMetadata, SessionMessage, SessionSnapshot, clear_sessions_dir_override_for_tests,
    override_sessions_dir_for_tests,
};
use chrono::Utc;
use tempfile::TempDir;

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
    clear_sessions_dir_override_for_tests();

    assert_eq!(
        sessions
            .iter()
            .map(|session| session.path.file_stem().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        ["session-09", "session-10"]
    );
}
