use super::support::ExecutionSummaryStatus;
use super::{
    archive::NextRuntimeArchiveId,
    archive::next_runtime_archive_id_request,
    archive::workspace_archive_label,
    orchestration::resolve_thread_completion_status,
    support::{
        TurnHistoryCheckpoint, build_tracked_file_freshness_note, build_unrelated_dirty_worktree_note,
        checkpoint_session_archive_start, format_workspace_relative_paths, latest_assistant_result_text,
        prepare_resume_bootstrap_without_archive, remove_transient_system_notes, take_pending_resumed_user_prompt,
    },
};
use crate::agent::agents::ResumeSession;
use crate::agent::runloop::git::normalize_workspace_path;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use chrono::Utc;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use vtcode_core::core::threads::ArchivedSessionIntent;
use vtcode_core::exec::events::ThreadCompletionSubtype;
use vtcode_core::hooks::SessionEndReason;
use vtcode_core::llm::provider::{AssistantPhase, MessageRole, ToolCall};
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
        vtcode_core::llm::provider::Message::system("Recovered from interrupted session".to_string()),
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
        vtcode_core::llm::provider::Message::assistant("VT Code is a Rust coding agent".to_string()),
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

    history.push(vtcode_core::llm::provider::Message::assistant("during turn".to_string()));
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

    history.push(vtcode_core::llm::provider::Message::assistant("retryable append".to_string()));

    checkpoint.rollback(&mut history);

    assert_eq!(history, expected_prefix);
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

#[test]
fn changed_file_summary_uses_relative_paths_with_external_fallback() {
    let summary = format_workspace_relative_paths(
        Path::new("/tmp/workspace"),
        &[
            PathBuf::from("/tmp/workspace/src/main.rs"),
            PathBuf::from("/tmp/external/generated.rs"),
        ],
    );

    assert_eq!(summary, "src/main.rs, /tmp/external/generated.rs");
}

#[test]
fn changed_file_summary_reports_empty_input() {
    assert_eq!(
        format_workspace_relative_paths(Path::new("/tmp/workspace"), std::iter::empty::<&PathBuf>()),
        "none recorded"
    );
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

    let note = build_unrelated_dirty_worktree_note(repo.path(), &touched_paths).expect("note build");

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
        SessionArchiveMetadata::new("workspace", "/tmp/workspace", "model", "provider", "theme", "medium"),
        None,
    );

    assert_eq!(thread_id, "session-source");
    assert_eq!(bootstrap.metadata.as_ref().map(|meta| meta.workspace_label.as_str()), Some("workspace"));
}

#[test]
fn resume_bootstrap_without_archive_prefers_reserved_identifier_for_forks() {
    let resume = resume_session(ArchivedSessionIntent::ForkNewArchive {
        custom_suffix: Some("branch".to_string()),
        summarize: false,
    });
    let (_, thread_id) = prepare_resume_bootstrap_without_archive(
        &resume,
        SessionArchiveMetadata::new("workspace", "/tmp/workspace", "model", "provider", "theme", "medium"),
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
        SessionArchiveMetadata::new("workspace", "/tmp/workspace", "model", "provider", "other-theme", "high"),
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
    assert_eq!(SessionEndReason::Exit.thread_completion_status(false), ("exit", ThreadCompletionSubtype::Cancelled));
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
        ("budget_limit_reached", ThreadCompletionSubtype::ErrorMaxBudgetUsd,)
    );
}

#[test]
fn new_session_preserves_last_turn_outcome() {
    struct CompletionCase {
        turn_result: Option<TurnLoopResult>,
        fallback: bool,
        outcome_code: &'static str,
        subtype: ThreadCompletionSubtype,
    }
    for case in [
        CompletionCase {
            turn_result: Some(TurnLoopResult::Blocked { reason: Some("recovery exhausted".into()) }),
            fallback: true,
            outcome_code: "blocked",
            subtype: ThreadCompletionSubtype::ErrorDuringExecution,
        },
        CompletionCase {
            turn_result: Some(TurnLoopResult::Aborted),
            fallback: false,
            outcome_code: "blocked",
            subtype: ThreadCompletionSubtype::ErrorDuringExecution,
        },
        CompletionCase {
            turn_result: Some(TurnLoopResult::Completed { plan_approved_execution_pending: false }),
            fallback: true,
            outcome_code: "failed",
            subtype: ThreadCompletionSubtype::ErrorDuringExecution,
        },
        CompletionCase {
            turn_result: Some(TurnLoopResult::Completed { plan_approved_execution_pending: false }),
            fallback: false,
            outcome_code: "new_session",
            subtype: ThreadCompletionSubtype::Success,
        },
        CompletionCase {
            turn_result: Some(TurnLoopResult::Completed { plan_approved_execution_pending: true }),
            fallback: false,
            outcome_code: "new_session",
            subtype: ThreadCompletionSubtype::Success,
        },
        CompletionCase {
            turn_result: Some(TurnLoopResult::Cancelled),
            fallback: false,
            outcome_code: "cancelled",
            subtype: ThreadCompletionSubtype::Cancelled,
        },
        CompletionCase {
            turn_result: Some(TurnLoopResult::Exit),
            fallback: false,
            outcome_code: "exit",
            subtype: ThreadCompletionSubtype::Cancelled,
        },
        CompletionCase {
            turn_result: None,
            fallback: false,
            outcome_code: "new_session",
            subtype: ThreadCompletionSubtype::Success,
        },
    ] {
        assert_eq!(
            resolve_thread_completion_status(
                &SessionEndReason::NewSession,
                false,
                None,
                case.turn_result.as_ref(),
                case.fallback,
            ),
            (case.outcome_code, case.subtype)
        );
    }
}

#[test]
fn failed_plan_summary_is_terminal_without_fallback_response() {
    for reason in [SessionEndReason::Completed, SessionEndReason::NewSession] {
        assert_eq!(
            resolve_thread_completion_status(
                &reason,
                false,
                Some(ExecutionSummaryStatus::Failed),
                Some(&TurnLoopResult::Completed { plan_approved_execution_pending: false }),
                false,
            ),
            ("failed", ThreadCompletionSubtype::ErrorDuringExecution)
        );
    }
}

#[test]
fn thread_budget_limit_takes_precedence_over_last_turn_and_clean_exit() {
    for reason in [
        SessionEndReason::Completed,
        SessionEndReason::NewSession,
        SessionEndReason::Exit,
    ] {
        for turn_result in [
            TurnLoopResult::Aborted,
            TurnLoopResult::Completed { plan_approved_execution_pending: false },
        ] {
            assert_eq!(
                resolve_thread_completion_status(&reason, true, None, Some(&turn_result), false),
                ("budget_limit_reached", ThreadCompletionSubtype::ErrorMaxBudgetUsd)
            );
        }
    }
}

#[test]
fn exit_after_completed_turn_reports_success_instead_of_cancellation() {
    assert_eq!(
        resolve_thread_completion_status(
            &SessionEndReason::Exit,
            false,
            Some(ExecutionSummaryStatus::Completed),
            Some(&TurnLoopResult::Completed { plan_approved_execution_pending: false }),
            false,
        ),
        ("exit", ThreadCompletionSubtype::Success)
    );

    assert_eq!(
        resolve_thread_completion_status(&SessionEndReason::Exit, false, None, Some(&TurnLoopResult::Exit), false,),
        ("exit", ThreadCompletionSubtype::Cancelled)
    );
}

#[test]
fn terminal_plan_rejection_never_reports_success_on_session_completion() {
    let rejected = TurnLoopResult::Blocked {
        reason: Some("plan artifact rejected; revise the plan before continuing".to_string()),
    };
    for end_reason in [
        SessionEndReason::Completed,
        SessionEndReason::NewSession,
        SessionEndReason::Exit,
    ] {
        let (_, subtype) = resolve_thread_completion_status(&end_reason, false, None, Some(&rejected), false);
        assert_ne!(subtype, ThreadCompletionSubtype::Success);
    }
    assert_eq!(
        resolve_thread_completion_status(&SessionEndReason::Completed, false, None, Some(&rejected), false),
        ("blocked", ThreadCompletionSubtype::ErrorDuringExecution)
    );
}

#[test]
fn latest_assistant_result_text_uses_latest_substantive_final_assistant_message() {
    let tool_call =
        ToolCall::function("call-1".to_string(), "read_file".to_string(), r#"{"path":"README.md"}"#.to_string());
    let messages = vec![
        vtcode_core::llm::provider::Message::user("hello".to_string()),
        vtcode_core::llm::provider::Message::assistant(" older answer ".to_string())
            .with_phase(Some(AssistantPhase::FinalAnswer)),
        vtcode_core::llm::provider::Message::assistant(" commentary only ".to_string())
            .with_phase(Some(AssistantPhase::Commentary)),
        vtcode_core::llm::provider::Message::assistant_with_tools("tool call only".to_string(), vec![tool_call])
            .with_phase(Some(AssistantPhase::FinalAnswer)),
        vtcode_core::llm::provider::Message::assistant(" final answer ".to_string())
            .with_phase(Some(AssistantPhase::FinalAnswer)),
        vtcode_core::llm::provider::Message::assistant("  \n  ".to_string())
            .with_phase(Some(AssistantPhase::FinalAnswer)),
    ];

    assert_eq!(latest_assistant_result_text(&messages), Some("final answer".to_string()));
}

#[test]
fn latest_assistant_result_text_returns_none_without_substantive_final_response() {
    let tool_call =
        ToolCall::function("call-2".to_string(), "read_file".to_string(), r#"{"path":"README.md"}"#.to_string());
    let messages = vec![
        vtcode_core::llm::provider::Message::assistant("  ".to_string()),
        vtcode_core::llm::provider::Message::assistant("commentary".to_string())
            .with_phase(Some(AssistantPhase::Commentary)),
        vtcode_core::llm::provider::Message::assistant_with_tools("tool call".to_string(), vec![tool_call]),
    ];

    assert_eq!(latest_assistant_result_text(&messages), None);
}

#[tokio::test]
async fn checkpoint_session_archive_start_writes_initial_snapshot() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let archive_path = temp_dir.path().join("session-vtcode-test-start.json");
    let metadata = SessionArchiveMetadata::new("workspace", "/tmp/workspace", "model", "provider", "theme", "medium");
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
        vtcode_core::core::threads::ThreadBootstrap::new(Some(metadata))
            .with_messages(vec![vtcode_core::llm::provider::Message::user("hello".to_string())]),
    );

    checkpoint_session_archive_start(&archive, &thread_handle)
        .await
        .expect("startup checkpoint");

    let snapshot: SessionSnapshot =
        serde_json::from_str(&fs::read_to_string(archive_path).expect("read archive")).expect("parse archive");
    assert_eq!(snapshot.total_messages, 1);
    assert_eq!(snapshot.messages.len(), 1);
    assert!(snapshot.progress.is_some());
}
