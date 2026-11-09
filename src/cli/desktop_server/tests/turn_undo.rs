use super::*;
use vtcode_core::exec::events::{
    CommandActivity, CommandExecutionItem, CommandExecutionStatus, FileChangeItem, FileUpdateChange,
    ItemCompletedEvent, ItemContext, PatchApplyStatus, PatchChangeKind, ThreadEvent, ThreadItem, ThreadItemDetails,
    VersionedThreadEvent,
};

const SESSION: &str = "undo-test-session";
const TURN: &str = "turn-undo-target";

fn file_change_event(
    item_id: &str,
    turn_id: Option<&str>,
    changes: Vec<(&str, PatchChangeKind)>,
    terminators: Option<bool>,
    status: PatchApplyStatus,
    diff: Option<&str>,
    incomplete: bool,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: item_id.to_string(),
            context: turn_id.map(|turn_id| {
                Box::new(ItemContext {
                    task_id: "task".to_string(),
                    turn_id: turn_id.to_string(),
                    actor_id: "actor".to_string(),
                    parent_actor_id: None,
                    timestamp: "2026-11-05T00:00:00Z".to_string(),
                    activity: None,
                })
            }),
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                diff_incomplete: incomplete.then_some(true),
                changes: changes
                    .into_iter()
                    .map(|(path, kind)| FileUpdateChange {
                        path: path.to_string(),
                        kind,
                        terminators_normalized: terminators,
                    })
                    .collect(),
                status,
                unified_diff: diff.map(str::to_string),
                additions: None,
                deletions: None,
            })),
        },
    })
}

/// Mirrors `vtcode_memory::snapshot` storage (`SESSIONS_DIR` is private):
/// `<workspace>/.vtcode/sessions/<session>/events.jsonl`.
fn write_events(workspace: &Path, session: &str, events: &[ThreadEvent]) {
    let dir = workspace.join(".vtcode/sessions").join(session);
    std::fs::create_dir_all(&dir).expect("event dir");
    let mut bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut bytes, &VersionedThreadEvent::new(event.clone())).expect("encode");
        bytes.push(b'\n');
    }
    std::fs::write(dir.join("events.jsonl"), bytes).expect("events");
}

/// Canonical workspace root: bound no-follow reads reject symlinked
/// ancestors (macOS tempdirs live under symlinked `/var`).
fn canonical_workspace(directory: &tempfile::TempDir) -> PathBuf {
    vtcode_commons::paths::canonicalize(directory.path()).expect("canonical workspace")
}

async fn initialized_context(workspace: PathBuf) -> ServiceContext {
    initialized_context_for(workspace, SESSION).await
}

async fn initialized_context_for(workspace: PathBuf, session: &str) -> ServiceContext {
    let startup = test_startup_context(&workspace);
    let (context, _) = test_service_context(startup, workspace);
    let mut state = context.state.lock().await;
    state.initialized = true;
    state.session_id = Some(session.to_string());
    drop(state);
    context
}

async fn undo_turn(context: &ServiceContext, session: &str, turn: &str) -> Result<protocol::TurnUndoResult> {
    let (value, _) = handle_request(
        context,
        &Request {
            jsonrpc: "2.0".into(),
            id: RequestId::String("undo".into()),
            method: protocol::method::TURN_UNDO.into(),
            params: serde_json::json!({ "sessionId": session, "turnId": turn }),
        },
    )
    .await?;
    Ok(serde_json::from_value(value)?)
}

#[tokio::test]
async fn undo_update_reverses_exact_bytes() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("notes.md"), "keep\nnew line\ntail\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("notes.md", PatchChangeKind::Update)],
            Some(false),
            PatchApplyStatus::Completed,
            Some("--- a/notes.md\n+++ b/notes.md\n@@ -1,3 +1,3 @@\n keep\n-old line\n+new line\n tail\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(result.undone, "{}", result.reason);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, "notes.md");
    assert_eq!(result.files[0].kind, PatchChangeKind::Update);
    assert_eq!((result.files[0].additions, result.files[0].deletions), (1, 1));
    assert_eq!(std::fs::read_to_string(workspace.join("notes.md")).expect("read"), "keep\nold line\ntail\n");
}

#[tokio::test]
async fn undo_add_removes_file_and_delete_restores_it() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("fresh.rs"), "brand new\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[
            file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![("fresh.rs", PatchChangeKind::Add)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- /dev/null\n+++ b/fresh.rs\n@@ -0,0 +1,1 @@\n+brand new\n"),
                false,
            ),
            file_change_event(
                "call-2:changes",
                Some(TURN),
                vec![("stale.rs", PatchChangeKind::Delete)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/stale.rs\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-gone\n"),
                false,
            ),
        ],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(result.undone, "{}", result.reason);
    assert!(!workspace.join("fresh.rs").exists(), "added file removed");
    assert_eq!(
        std::fs::read_to_string(workspace.join("stale.rs")).expect("read"),
        "gone\n",
        "deleted file restored"
    );
}

#[tokio::test]
async fn undo_repeated_edits_same_file_in_reverse() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("chain.rs"), "v3\n").expect("fixture");
    // Two asymmetric updates in one turn: v1 -> v2, then v2 -> v3.
    write_events(
        &workspace,
        SESSION,
        &[
            file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![("chain.rs", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/chain.rs\n+++ b/chain.rs\n@@ -1,1 +1,1 @@\n-v1\n+v2\n"),
                false,
            ),
            file_change_event(
                "call-2:changes",
                Some(TURN),
                vec![("chain.rs", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/chain.rs\n+++ b/chain.rs\n@@ -1,1 +1,1 @@\n-v2\n+v3\n"),
                false,
            ),
        ],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(result.undone, "{}", result.reason);
    assert_eq!(result.files.len(), 2);
    assert_eq!(std::fs::read_to_string(workspace.join("chain.rs")).expect("read"), "v1\n");
}

#[tokio::test]
async fn undo_headerless_single_file_diff() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("bare.rs"), "now\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("bare.rs", PatchChangeKind::Update)],
            None,
            PatchApplyStatus::Completed,
            Some("@@ -1,1 +1,1 @@\n-was\n+now\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(result.undone, "{}", result.reason);
    assert_eq!(std::fs::read_to_string(workspace.join("bare.rs")).expect("read"), "was\n");
}

#[tokio::test]
async fn undo_refuses_later_edit_and_leaves_file_untouched() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("notes.md"), "keep\nnew line plus more\ntail\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("notes.md", PatchChangeKind::Update)],
            Some(false),
            PatchApplyStatus::Completed,
            Some("--- a/notes.md\n+++ b/notes.md\n@@ -1,3 +1,3 @@\n keep\n-old line\n+new line\n tail\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("notes.md"), "{}", result.reason);
    assert!(result.files.is_empty());
    assert_eq!(
        std::fs::read_to_string(workspace.join("notes.md")).expect("read"),
        "keep\nnew line plus more\ntail\n",
        "refused undo mutates nothing"
    );
}

#[tokio::test]
async fn undo_refuses_when_later_turn_touched_same_file() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("shared.rs"), "turn two\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[
            file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![("shared.rs", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/shared.rs\n+++ b/shared.rs\n@@ -1,1 +1,1 @@\n-original\n+turn one\n"),
                false,
            ),
            file_change_event(
                "call-2:changes",
                Some("turn-later"),
                vec![("shared.rs", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/shared.rs\n+++ b/shared.rs\n@@ -1,1 +1,1 @@\n-turn one\n+turn two\n"),
                false,
            ),
        ],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("shared.rs"), "{}", result.reason);
    assert_eq!(std::fs::read_to_string(workspace.join("shared.rs")).expect("read"), "turn two\n");
}

#[tokio::test]
async fn undo_refuses_failed_incomplete_and_missing_evidence() {
    for (name, status, diff, incomplete, changes) in [
        (
            "failed",
            PatchApplyStatus::Failed,
            Some("@@ -1,1 +1,1 @@\n-a\n+b\n"),
            false,
            vec![("x.rs", PatchChangeKind::Update)],
        ),
        (
            "incomplete",
            PatchApplyStatus::Completed,
            Some("@@ -1,1 +1,1 @@\n-a\n+b\n"),
            true,
            vec![("x.rs", PatchChangeKind::Update)],
        ),
        ("missing-diff", PatchApplyStatus::Completed, None, false, vec![("x.rs", PatchChangeKind::Update)]),
        ("missing-files", PatchApplyStatus::Completed, Some("@@ -1,1 +1,1 @@\n-a\n+b\n"), false, vec![]),
    ] {
        let directory = hermetic_workspace();
        let workspace = canonical_workspace(&directory);
        std::fs::write(workspace.join("x.rs"), "b\n").expect("fixture");
        write_events(
            &workspace,
            SESSION,
            &[file_change_event(
                "call-1:changes",
                Some(TURN),
                changes,
                None,
                status,
                diff,
                incomplete,
            )],
        );
        let context = initialized_context(workspace.clone()).await;
        let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
        assert!(!result.undone, "{name} must refuse");
        assert!(!result.reason.is_empty(), "{name} explains why");
        assert_eq!(std::fs::read_to_string(workspace.join("x.rs")).expect("read"), "b\n", "{name} mutates nothing");
    }
}

#[tokio::test]
async fn undo_refuses_mismatched_file_list() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("real.rs"), "b\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("other.rs", PatchChangeKind::Update)],
            None,
            PatchApplyStatus::Completed,
            Some("--- a/real.rs\n+++ b/real.rs\n@@ -1,1 +1,1 @@\n-a\n+b\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("does not match"), "{}", result.reason);
    assert_eq!(std::fs::read_to_string(workspace.join("real.rs")).expect("read"), "b\n");
}

#[tokio::test]
async fn undo_refuses_unknown_turn_session_and_unattributed_changes() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("x.rs"), "b\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            None,
            vec![("x.rs", PatchChangeKind::Update)],
            None,
            PatchApplyStatus::Completed,
            Some("@@ -1,1 +1,1 @@\n-a\n+b\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let unattributed = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!unattributed.undone);
    assert!(unattributed.reason.contains("attribution"), "{}", unattributed.reason);

    let unknown_turn = undo_turn(&context, SESSION, "turn-missing").await.expect("undo");
    assert!(!unknown_turn.undone);

    let unknown_session = undo_turn(&context, "missing-session", TURN).await.expect_err("unknown session");
    assert!(unknown_session.to_string().contains("selected session"), "{unknown_session}");
    assert_eq!(std::fs::read_to_string(workspace.join("x.rs")).expect("read"), "b\n");
}

#[tokio::test]
async fn undo_refuses_selected_session_without_history() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    let context = initialized_context(workspace).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("no recorded history"), "{}", result.reason);
}

#[tokio::test]
async fn undo_rejects_non_selected_session() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("x.rs"), "b\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("x.rs", PatchChangeKind::Update)],
            None,
            PatchApplyStatus::Completed,
            Some("@@ -1,1 +1,1 @@\n-a\n+b\n"),
            false,
        )],
    );
    let context = initialized_context_for(workspace.clone(), "other-session").await;
    let error = undo_turn(&context, SESSION, TURN)
        .await
        .expect_err("non-selected session must fail");
    assert!(error.to_string().contains("selected session"), "{error}");
    assert_eq!(std::fs::read_to_string(workspace.join("x.rs")).expect("read"), "b\n");
}

#[tokio::test]
async fn undo_rejects_active_turn_with_transport_error() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    let context = initialized_context(workspace).await;
    context.state.lock().await.active_turn_id = Some("turn-live".to_string());
    let error = undo_turn(&context, SESSION, TURN).await.expect_err("active turn must fail");
    assert!(error.to_string().contains("active turn"), "{error}");
}

#[tokio::test]
async fn undo_refuses_path_escape() {
    for path in ["../outside.txt", "/tmp/undo-escape.txt"] {
        let directory = hermetic_workspace();
        let workspace = canonical_workspace(&directory);
        write_events(
            &workspace,
            SESSION,
            &[file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![(path, PatchChangeKind::Update)],
                None,
                PatchApplyStatus::Completed,
                Some("@@ -1,1 +1,1 @@\n-a\n+b\n"),
                false,
            )],
        );
        let context = initialized_context(workspace.clone()).await;
        let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
        assert!(!result.undone, "{path} must refuse");
        assert!(result.reason.contains("escapes the workspace"), "{path}: {}", result.reason);
    }
}

#[tokio::test]
async fn undo_refuses_symlink_file_and_directory() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("target.txt"), "real\n").expect("fixture");
    #[cfg(unix)]
    std::os::unix::fs::symlink(workspace.join("target.txt"), workspace.join("linked.txt")).expect("symlink");
    #[cfg(not(unix))]
    std::fs::write(workspace.join("linked.txt"), "real\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("linked.txt", PatchChangeKind::Update)],
            None,
            PatchApplyStatus::Completed,
            Some("@@ -1,1 +1,1 @@\n-old\n+real\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    #[cfg(unix)]
    {
        assert!(!result.undone);
        assert!(result.reason.contains("symlink"), "{}", result.reason);
        assert_eq!(std::fs::read_to_string(workspace.join("target.txt")).expect("read"), "real\n");
    }
    #[cfg(not(unix))]
    {
        assert!(!result.undone, "content mismatch still refuses: {}", result.reason);
    }
}

#[tokio::test]
async fn undo_refuses_symlinked_ancestor_directory() {
    #[cfg(not(unix))]
    return;
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("inner.txt"), "new\n").expect("fixture");
    std::os::unix::fs::symlink(outside.path(), workspace.join("linked-dir")).expect("symlink");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("linked-dir/inner.txt", PatchChangeKind::Update)],
            None,
            PatchApplyStatus::Completed,
            Some("@@ -1,1 +1,1 @@\n-old\n+new\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("symlink"), "{}", result.reason);
    assert_eq!(std::fs::read_to_string(outside.path().join("inner.txt")).expect("read"), "new\n");
}

#[tokio::test]
async fn undo_partial_flush_reports_applied_files() {
    #[cfg(not(unix))]
    return;
    use std::os::unix::fs::PermissionsExt;
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("first.rs"), "new one\n").expect("fixture");
    std::fs::create_dir(workspace.join("locked")).expect("dir");
    std::fs::write(workspace.join("locked/second.rs"), "new two\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[
            file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![("first.rs", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/first.rs\n+++ b/first.rs\n@@ -1,1 +1,1 @@\n-old one\n+new one\n"),
                false,
            ),
            file_change_event(
                "call-2:changes",
                Some(TURN),
                vec![("locked/second.rs", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/locked/second.rs\n+++ b/locked/second.rs\n@@ -1,1 +1,1 @@\n-old two\n+new two\n"),
                false,
            ),
        ],
    );
    // Read-only directory: planning reads fine, but the temp-file write
    // during flush fails deterministically.
    std::fs::set_permissions(workspace.join("locked"), std::fs::Permissions::from_mode(0o555)).expect("lock");
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    std::fs::set_permissions(workspace.join("locked"), std::fs::Permissions::from_mode(0o755)).expect("unlock");
    assert!(!result.undone);
    assert_eq!(result.files.len(), 1, "applied file disclosed: {}", result.reason);
    assert_eq!(result.files[0].path, "first.rs");
    assert!(result.reason.contains("1 of 2"), "{}", result.reason);
    assert_eq!(std::fs::read_to_string(workspace.join("first.rs")).expect("read"), "old one\n");
    assert_eq!(
        std::fs::read_to_string(workspace.join("locked/second.rs")).expect("read"),
        "new two\n",
        "failed file keeps its content"
    );
}

fn command_event(item_id: &str, turn_id: &str, activity: CommandActivity) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: item_id.to_string(),
            context: Some(Box::new(ItemContext {
                task_id: "task".to_string(),
                turn_id: turn_id.to_string(),
                actor_id: "actor".to_string(),
                parent_actor_id: None,
                timestamp: "2026-11-05T00:00:00Z".to_string(),
                activity: Some(activity),
            })),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: "sed".to_string(),
                arguments: None,
                aggregated_output: String::new(),
                exit_code: Some(0),
                status: CommandExecutionStatus::Completed,
            })),
        },
    })
}

#[tokio::test]
async fn undo_refuses_turn_with_mutating_shell_activity() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("notes.md"), "keep\nnew line\ntail\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[
            file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![("notes.md", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/notes.md\n+++ b/notes.md\n@@ -1,3 +1,3 @@\n keep\n-old line\n+new line\n tail\n"),
                false,
            ),
            command_event("exec-1", TURN, CommandActivity::Mutation),
        ],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("outside the file pipeline"), "{}", result.reason);
    assert_eq!(std::fs::read_to_string(workspace.join("notes.md")).expect("read"), "keep\nnew line\ntail\n");
}

#[tokio::test]
async fn undo_allows_turn_with_inspection_shell_activity() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("notes.md"), "keep\nnew line\ntail\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[
            file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![("notes.md", PatchChangeKind::Update)],
                Some(false),
                PatchApplyStatus::Completed,
                Some("--- a/notes.md\n+++ b/notes.md\n@@ -1,3 +1,3 @@\n keep\n-old line\n+new line\n tail\n"),
                false,
            ),
            command_event("exec-1", TURN, CommandActivity::Inspection),
        ],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(result.undone, "{}", result.reason);
    assert_eq!(std::fs::read_to_string(workspace.join("notes.md")).expect("read"), "keep\nold line\ntail\n");
}

#[tokio::test]
async fn undo_refuses_normalized_and_unverifiable_terminators() {
    // Update with normalized terminators: the recorded bytes cannot
    // reproduce the original, so even a content match is untrustworthy.
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("crlf.md"), "keep\nnew line\n").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("crlf.md", PatchChangeKind::Update)],
            Some(true),
            PatchApplyStatus::Completed,
            Some("--- a/crlf.md\n+++ b/crlf.md\n@@ -1,2 +1,2 @@\n keep\n-old line\n+new line\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("normalized line endings"), "{}", result.reason);

    // Delete without attestation (legacy): the absent file leaves nothing
    // to verify against, so unknown recordings refuse.
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("legacy.rs", PatchChangeKind::Delete)],
            None,
            PatchApplyStatus::Completed,
            Some("--- a/legacy.rs\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-gone\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("cannot be verified"), "{}", result.reason);
    assert!(!workspace.join("legacy.rs").exists());
}

#[tokio::test]
async fn undo_empty_file_add_and_delete() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    std::fs::write(workspace.join("empty-new.rs"), "").expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[
            file_change_event(
                "call-1:changes",
                Some(TURN),
                vec![("empty-new.rs", PatchChangeKind::Add)],
                Some(false),
                PatchApplyStatus::Completed,
                Some(""),
                false,
            ),
            file_change_event(
                "call-2:changes",
                Some(TURN),
                vec![("empty-gone.rs", PatchChangeKind::Delete)],
                Some(false),
                PatchApplyStatus::Completed,
                None,
                false,
            ),
        ],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(result.undone, "{}", result.reason);
    assert!(!workspace.join("empty-new.rs").exists(), "empty added file removed");
    assert_eq!(
        std::fs::read_to_string(workspace.join("empty-gone.rs")).expect("read"),
        "",
        "empty deleted file recreated"
    );
}

#[tokio::test]
async fn undo_refuses_oversized_file_before_reading() {
    let directory = hermetic_workspace();
    let workspace = canonical_workspace(&directory);
    let big = "x\n".repeat(vtcode_diff::MAX_REVERSE_FILE_BYTES);
    std::fs::write(workspace.join("big.log"), &big).expect("fixture");
    write_events(
        &workspace,
        SESSION,
        &[file_change_event(
            "call-1:changes",
            Some(TURN),
            vec![("big.log", PatchChangeKind::Update)],
            Some(false),
            PatchApplyStatus::Completed,
            Some("@@ -1,1 +1,1 @@\n-a\n+x\n"),
            false,
        )],
    );
    let context = initialized_context(workspace.clone()).await;
    let result = undo_turn(&context, SESSION, TURN).await.expect("undo");
    assert!(!result.undone);
    assert!(result.reason.contains("too large"), "{}", result.reason);
}
