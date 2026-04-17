use super::{
    TurnHistoryCheckpoint, archive::NextRuntimeArchiveId, archive::next_runtime_archive_id_request,
    archive::workspace_archive_label, build_partial_timeout_messages,
    build_tracked_file_freshness_note, build_unrelated_dirty_worktree_note,
    checkpoint_session_archive_start, effective_max_tool_calls_for_turn,
    latest_assistant_result_text, prepare_resume_bootstrap_without_archive,
    remove_transient_system_notes, resolve_effective_turn_timeout_secs,
    should_attempt_requesting_timeout_recovery, take_pending_resumed_user_prompt,
};
use crate::agent::agents::ResumeSession;
use crate::agent::runloop::git::normalize_workspace_path;
use crate::agent::runloop::unified::run_loop_context::TurnPhase;
use chrono::Utc;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use vtcode_core::core::threads::ArchivedSessionIntent;
use vtcode_core::exec::events::ThreadCompletionSubtype;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider::MessageRole;
use vtcode_core::utils::session_archive::{
    SessionArchive, SessionArchiveMetadata, SessionListing, SessionMessage, SessionSnapshot,
};

fn resume_session(intent: ArchivedSessionIntent) -> ResumeSession {
    let listing = SessionListing {
        path: PathBuf::from("/tmp/session-source.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "workspace",
                "/tmp/workspace",
                "model",
                "provider",
                "theme",
                "medium",
            ),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "hello")],
            progress: None,
            error_logs: Vec::new(),
        },
    };

    ResumeSession::from_listing(&listing, intent)
}

#[test]
fn turn_timeout_respects_tool_wall_clock_budget() {
    assert_eq!(resolve_effective_turn_timeout_secs(300, 600), 660);
}

#[test]
fn turn_timeout_keeps_higher_configured_value() {
    assert_eq!(resolve_effective_turn_timeout_secs(900, 600), 900);
}

#[test]
fn turn_timeout_includes_full_llm_attempt_grace() {
    assert_eq!(resolve_effective_turn_timeout_secs(360, 360), 432);
}

#[test]
fn turn_timeout_expands_buffer_for_large_configs() {
    assert_eq!(resolve_effective_turn_timeout_secs(600, 600), 720);
}

#[test]
fn plan_mode_applies_tool_call_floor() {
    assert_eq!(effective_max_tool_calls_for_turn(32, true), 48);
    assert_eq!(effective_max_tool_calls_for_turn(64, true), 64);
}

#[test]
fn zero_tool_call_limit_stays_unlimited_in_all_modes() {
    assert_eq!(effective_max_tool_calls_for_turn(0, true), 0);
    assert_eq!(effective_max_tool_calls_for_turn(0, false), 0);
}

#[test]
fn edit_mode_keeps_configured_tool_call_limit() {
    assert_eq!(effective_max_tool_calls_for_turn(32, false), 32);
}

#[test]
fn requesting_partial_timeout_recovery_message_mentions_continuation() {
    let (timeout_message, timeout_error_message) =
        build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, true);
    assert!(timeout_message.contains("continuing with a compacted tool-free recovery pass"));
    assert!(timeout_error_message.contains("Continuing with a compacted tool-free recovery pass"));
}

#[test]
fn requesting_partial_timeout_without_recovery_mentions_retry_skip() {
    let (timeout_message, timeout_error_message) =
        build_partial_timeout_messages(660, TurnPhase::Requesting, 25, 0, false);
    assert!(timeout_message.contains("retry is skipped"));
    assert!(!timeout_message.contains("continuing with a compacted tool-free recovery pass"));
    assert!(!timeout_error_message.contains("Continuing with a compacted tool-free recovery pass"));
}

#[test]
fn take_pending_resumed_user_prompt_removes_trailing_user_message() {
    let mut history = vec![
        vtcode_core::llm::provider::Message::system("[Session Memory Envelope]".to_string()),
        vtcode_core::llm::provider::Message::user("what is this project".to_string()),
    ];

    let pending = take_pending_resumed_user_prompt(&mut history);

    assert_eq!(pending.as_deref(), Some("what is this project"));
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].role, MessageRole::System);
}

#[test]
fn take_pending_resumed_user_prompt_handles_trailing_system_notes() {
    let mut history = vec![
        vtcode_core::llm::provider::Message::system("[Session Memory Envelope]".to_string()),
        vtcode_core::llm::provider::Message::user("what is this project".to_string()),
        vtcode_core::llm::provider::Message::system(
            "Recovered from interrupted session".to_string(),
        ),
    ];

    let pending = take_pending_resumed_user_prompt(&mut history);

    assert_eq!(pending.as_deref(), Some("what is this project"));
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].role, MessageRole::System);
    assert_eq!(history[1].role, MessageRole::System);
}

#[test]
fn take_pending_resumed_user_prompt_ignores_completed_turns() {
    let mut history = vec![
        vtcode_core::llm::provider::Message::user("what is this project".to_string()),
        vtcode_core::llm::provider::Message::assistant(
            "VT Code is a Rust coding agent".to_string(),
        ),
        vtcode_core::llm::provider::Message::system("[Session Memory Envelope]".to_string()),
    ];

    let pending = take_pending_resumed_user_prompt(&mut history);

    assert!(pending.is_none());
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role, MessageRole::User);
    assert_eq!(history[1].role, MessageRole::Assistant);
}

#[test]
fn turn_history_checkpoint_truncates_appended_messages() {
    let mut history = vec![
        vtcode_core::llm::provider::Message::user("before".to_string()),
        vtcode_core::llm::provider::Message::assistant("baseline".to_string()),
    ];
    let checkpoint = TurnHistoryCheckpoint::capture(&history);

    history.push(vtcode_core::llm::provider::Message::assistant(
        "during turn".to_string(),
    ));
    history.push(vtcode_core::llm::provider::Message::tool_response(
        "call-1".to_string(),
        "{\"ok\":true}".to_string(),
    ));

    checkpoint.rollback(&mut history);

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content.as_text(), "before");
    assert_eq!(history[1].content.as_text(), "baseline");
}

#[test]
fn turn_history_checkpoint_preserves_preexisting_history_prefix() {
    let mut history = vec![
        vtcode_core::llm::provider::Message::system("system".to_string()),
        vtcode_core::llm::provider::Message::user("request".to_string()),
        vtcode_core::llm::provider::Message::assistant("response".to_string()),
    ];
    let expected_prefix = history.clone();
    let checkpoint = TurnHistoryCheckpoint::capture(&history);

    history.push(vtcode_core::llm::provider::Message::assistant(
        "retryable append".to_string(),
    ));

    checkpoint.rollback(&mut history);

    assert_eq!(history, expected_prefix);
}

#[test]
fn requesting_timeout_without_tool_activity_omits_autonomous_recovery_note() {
    let (timeout_message, timeout_error_message) =
        build_partial_timeout_messages(660, TurnPhase::Requesting, 0, 0, false);
    assert!(!timeout_message.contains("continuing with a compacted tool-free recovery pass"));
    assert!(!timeout_error_message.contains("Continuing with a compacted tool-free recovery pass"));
}

#[test]
fn requesting_timeout_recovery_only_runs_once() {
    assert!(should_attempt_requesting_timeout_recovery(
        TurnPhase::Requesting,
        true,
        false,
    ));
    assert!(!should_attempt_requesting_timeout_recovery(
        TurnPhase::Requesting,
        true,
        true,
    ));
    assert!(!should_attempt_requesting_timeout_recovery(
        TurnPhase::ExecutingTools,
        true,
        false,
    ));
}

#[test]
fn transient_system_note_cleanup_removes_by_content_from_latest_match() {
    let note = "Freshness note: file changed".to_string();
    let older = vtcode_core::llm::provider::Message::system(note.clone());
    let transient = vtcode_core::llm::provider::Message::system(note.clone());
    let mut history = vec![
        older,
        vtcode_core::llm::provider::Message::assistant("summary".to_string()),
        transient,
        vtcode_core::llm::provider::Message::user("preserved".to_string()),
    ];

    remove_transient_system_notes(&mut history, std::slice::from_ref(&note));

    assert_eq!(history.len(), 3);
    assert_eq!(history[0].content.as_text(), note);
    assert_eq!(history[1].content.as_text(), "summary");
    assert_eq!(history[2].content.as_text(), "preserved");
}

#[test]
fn workspace_archive_label_uses_directory_name() {
    assert_eq!(workspace_archive_label(Path::new("/tmp/demo")), "demo");
}

#[test]
fn tracked_file_freshness_note_uses_relative_paths_and_reread_guidance() {
    let note = build_tracked_file_freshness_note(
        Path::new("/tmp/workspace"),
        &[
            PathBuf::from("/tmp/workspace/src/main.rs"),
            PathBuf::from("/tmp/workspace/docs/project/TODO.md"),
        ],
    )
    .expect("freshness note");

    assert!(note.contains("Freshness note"));
    assert!(note.contains("- src/main.rs"));
    assert!(note.contains("- docs/project/TODO.md"));
    assert!(note.contains("Re-read these files before relying on earlier content"));
}

fn init_git_repo() -> TempDir {
    let temp = TempDir::new().expect("temp dir");
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(temp.path())
            .status()
            .expect("git command");
        assert!(status.success(), "git command failed: {args:?}");
    };

    run(&["init"]);
    run(&["config", "user.name", "VT Code"]);
    run(&["config", "user.email", "vtcode@example.com"]);
    temp
}

fn seed_dirty_repo() -> (TempDir, PathBuf) {
    let repo = init_git_repo();
    let path = repo.path().join("docs/project/TODO.md");
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(&path, "before\n").expect("write");

    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .status()
            .expect("git command");
        assert!(status.success(), "git command failed: {args:?}");
    };

    run(&["add", "."]);
    run(&["commit", "-m", "test: seed repo"]);
    fs::write(&path, "after\n").expect("write");
    (repo, path)
}

#[test]
fn unrelated_dirty_worktree_note_uses_relative_paths_and_user_owned_guidance() {
    let (repo, path) = seed_dirty_repo();

    let note = build_unrelated_dirty_worktree_note(repo.path(), &BTreeSet::new())
        .expect("note build")
        .expect("note");

    assert!(note.contains("Workspace note"));
    assert!(note.contains("docs/project/TODO.md"));
    assert!(note.contains("user-owned changes"));
    assert!(!note.contains(&path.display().to_string()));
}

#[test]
fn unrelated_dirty_worktree_note_skips_agent_touched_files() {
    let (repo, path) = seed_dirty_repo();
    let mut touched_paths = BTreeSet::new();
    touched_paths.insert(normalize_workspace_path(repo.path(), &path));

    let note =
        build_unrelated_dirty_worktree_note(repo.path(), &touched_paths).expect("note build");

    assert!(note.is_none());
}

#[test]
fn next_runtime_archive_id_request_reuses_existing_id_for_resume() {
    let resume = resume_session(ArchivedSessionIntent::ResumeInPlace);

    assert_eq!(
        next_runtime_archive_id_request(Path::new("/tmp/workspace"), Some(&resume)),
        NextRuntimeArchiveId::Existing("session-source".to_string())
    );
}

#[test]
fn next_runtime_archive_id_request_reserves_for_fork_and_new_session() {
    let resume = resume_session(ArchivedSessionIntent::ForkNewArchive {
        custom_suffix: Some("branch".to_string()),
        summarize: false,
    });

    assert_eq!(
        next_runtime_archive_id_request(Path::new("/tmp/workspace"), Some(&resume)),
        NextRuntimeArchiveId::Reserve {
            workspace_label: "workspace".to_string(),
            custom_suffix: Some("branch".to_string()),
        }
    );
    assert_eq!(
        next_runtime_archive_id_request(Path::new("/tmp/workspace"), None),
        NextRuntimeArchiveId::Reserve {
            workspace_label: "workspace".to_string(),
            custom_suffix: None,
        }
    );
}

#[test]
fn resume_bootstrap_without_archive_reuses_identifier_for_in_place_resume() {
    let resume = resume_session(ArchivedSessionIntent::ResumeInPlace);
    let (bootstrap, thread_id) = prepare_resume_bootstrap_without_archive(
        &resume,
        SessionArchiveMetadata::new(
            "workspace",
            "/tmp/workspace",
            "model",
            "provider",
            "theme",
            "medium",
        ),
        None,
    );

    assert_eq!(thread_id, "session-source");
    assert_eq!(
        bootstrap
            .metadata
            .as_ref()
            .map(|meta| meta.workspace_label.as_str()),
        Some("workspace")
    );
}

#[test]
fn resume_bootstrap_without_archive_prefers_reserved_identifier_for_forks() {
    let resume = resume_session(ArchivedSessionIntent::ForkNewArchive {
        custom_suffix: Some("branch".to_string()),
        summarize: false,
    });
    let (_, thread_id) = prepare_resume_bootstrap_without_archive(
        &resume,
        SessionArchiveMetadata::new(
            "workspace",
            "/tmp/workspace",
            "model",
            "provider",
            "theme",
            "medium",
        ),
        Some("reserved-session-id".to_string()),
    );

    assert_eq!(thread_id, "reserved-session-id");
}

#[test]
fn resume_bootstrap_without_archive_preserves_compatible_prompt_cache_lineage() {
    let listing = SessionListing {
        path: PathBuf::from("/tmp/session-source.json"),
        snapshot: SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "workspace",
                "/tmp/workspace",
                "model",
                "provider",
                "theme",
                "medium",
            )
            .with_prompt_cache_lineage_id("lineage-123"),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 1,
            distinct_tools: Vec::new(),
            transcript: Vec::new(),
            messages: vec![SessionMessage::new(MessageRole::User, "hello")],
            progress: None,
            error_logs: Vec::new(),
        },
    };
    let resume = ResumeSession::from_listing(&listing, ArchivedSessionIntent::ResumeInPlace);
    let (bootstrap, _) = prepare_resume_bootstrap_without_archive(
        &resume,
        SessionArchiveMetadata::new(
            "workspace",
            "/tmp/workspace",
            "model",
            "provider",
            "other-theme",
            "high",
        ),
        None,
    );

    assert_eq!(
        bootstrap
            .metadata
            .as_ref()
            .and_then(|meta| meta.prompt_cache_lineage_id.as_deref()),
        Some("lineage-123")
    );
}

#[test]
fn thread_completion_status_matches_public_contract() {
    assert_eq!(
        SessionEndReason::Completed.thread_completion_status(false),
        ("completed", ThreadCompletionSubtype::Success)
    );
    assert_eq!(
        SessionEndReason::NewSession.thread_completion_status(false),
        ("new_session", ThreadCompletionSubtype::Success)
    );
    assert_eq!(
        SessionEndReason::Exit.thread_completion_status(false),
        ("exit", ThreadCompletionSubtype::Cancelled)
    );
    assert_eq!(
        SessionEndReason::Cancelled.thread_completion_status(false),
        ("cancelled", ThreadCompletionSubtype::Cancelled)
    );
    assert_eq!(
        SessionEndReason::Error.thread_completion_status(false),
        ("error", ThreadCompletionSubtype::ErrorDuringExecution)
    );
    assert_eq!(
        SessionEndReason::Completed.thread_completion_status(true),
        (
            "budget_limit_reached",
            ThreadCompletionSubtype::ErrorMaxBudgetUsd,
        )
    );
}

#[test]
fn latest_assistant_result_text_uses_latest_nonempty_assistant_message() {
    let messages = vec![
        vtcode_core::llm::provider::Message::user("hello".to_string()),
        vtcode_core::llm::provider::Message::assistant(" first ".to_string()),
        vtcode_core::llm::provider::Message::tool_response(
            "call-1".to_string(),
            "{\"ok\":true}".to_string(),
        ),
        vtcode_core::llm::provider::Message::assistant(" final answer ".to_string()),
    ];

    assert_eq!(
        latest_assistant_result_text(&messages),
        Some("final answer".to_string())
    );
}

#[tokio::test]
async fn checkpoint_session_archive_start_writes_initial_snapshot() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let archive_path = temp_dir.path().join("session-vtcode-test-start.json");
    let metadata = SessionArchiveMetadata::new(
        "workspace",
        "/tmp/workspace",
        "model",
        "provider",
        "theme",
        "medium",
    );
    let archive = SessionArchive::resume_from_listing(
        &SessionListing {
            path: archive_path.clone(),
            snapshot: SessionSnapshot {
                metadata: metadata.clone(),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 0,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: Vec::new(),
                progress: None,
                error_logs: Vec::new(),
            },
        },
        metadata.clone(),
    );
    let thread_manager = vtcode_core::core::threads::ThreadManager::new();
    let thread_handle = thread_manager.start_thread_with_identifier(
        "session-vtcode-test-start",
        vtcode_core::core::threads::ThreadBootstrap::new(Some(metadata)).with_messages(vec![
            vtcode_core::llm::provider::Message::user("hello".to_string()),
        ]),
    );

    checkpoint_session_archive_start(&archive, &thread_handle)
        .await
        .expect("startup checkpoint");

    let snapshot: SessionSnapshot =
        serde_json::from_str(&std::fs::read_to_string(archive_path).expect("read archive"))
            .expect("parse archive");
    assert_eq!(snapshot.total_messages, 1);
    assert_eq!(snapshot.messages.len(), 1);
    assert!(snapshot.progress.is_some());
}
