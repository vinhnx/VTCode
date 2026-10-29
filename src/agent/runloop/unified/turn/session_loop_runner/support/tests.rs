use super::{ExecutionSummaryStatus, RefusedTurnRollback, checkpoint_unavailable_notice, classify_execution_summary};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use serde_json::json;

#[test]
fn checkpoint_unavailable_notice_names_safety_lock_and_options_for_busy_workspace() {
    let notice = checkpoint_unavailable_notice("Another turn or rewind is using this workspace: operation would block");
    assert!(notice.starts_with("Prompt kept"), "notice must confirm the prompt is kept: {notice}");
    assert!(notice.contains("safety lock"), "notice must name the safety reason: {notice}");
    assert!(notice.contains("wait then retry"), "notice must offer wait+retry: {notice}");
    assert!(notice.contains("separate worktree"), "notice must offer worktree option: {notice}");
}

#[test]
fn checkpoint_unavailable_notice_routes_interrupted_rewind_to_recover() {
    let notice = checkpoint_unavailable_notice("Interrupted rewind; run again before continuing");
    assert_eq!(notice, "Prompt kept — rewind interrupted. Run /rewind again to auto-recover, then retry.");
}

#[test]
fn checkpoint_unavailable_notice_falls_back_to_concise_retry() {
    let notice = checkpoint_unavailable_notice("Checkpoint skipped paths; cannot safely start turn");
    assert_eq!(notice, "Prompt kept — checkpoint unavailable. Wait a moment and retry.");
}

#[tokio::test]
async fn exec_session_resume_note_is_none_without_sessions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

    let note = super::build_exec_session_resume_note(&registry).await;
    assert!(note.is_none(), "no sessions must produce no hint");
}

#[tokio::test]
#[cfg(unix)]
async fn exec_session_resume_note_carries_identity_and_wait_args() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

    // Start a long-running command through the public exec surface so the
    // session lands in the live registry.
    let run = registry
        .execute_public_tool_ref(
            vtcode_core::config::constants::tools::EXEC_COMMAND,
            &json!({"cmd": "sleep 5", "yield_time_ms": 100}),
        )
        .await
        .expect("run should start");
    let session_id = run["session_id"].as_str().expect("session id present").to_string();

    let note = super::build_exec_session_resume_note(&registry).await.expect("hint present");
    assert!(note.contains(&session_id), "hint must carry the session id: {note}");
    assert!(note.contains("sleep 5"), "hint must carry the command: {note}");
    assert!(note.contains("\"action\": \"wait\""), "hint must pre-fill the wait action: {note}");
    assert!(note.contains("wait_timeout_seconds"), "hint must pre-fill the deadline: {note}");
    assert!(note.len() < 1_024, "hint must stay bounded: {} bytes", note.len());

    registry.close_harness_exec_session(&session_id).await.expect("close session");
    let note_after = super::build_exec_session_resume_note(&registry).await;
    assert!(note_after.is_none(), "closed session must clear the hint");
}

#[tokio::test]
#[cfg(unix)]
async fn exec_session_resume_note_bounds_a_long_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

    // A command body large enough to blow any per-line bound if inlined
    // whole; the note must still stay within its few-hundred-byte budget.
    let long_command = format!("sleep 5 # {}", "x".repeat(4_000));
    let run = registry
        .execute_public_tool_ref(
            vtcode_core::config::constants::tools::EXEC_COMMAND,
            &json!({"cmd": long_command, "yield_time_ms": 100}),
        )
        .await
        .expect("run should start");
    let session_id = run["session_id"].as_str().expect("session id present").to_string();

    let note = super::build_exec_session_resume_note(&registry).await.expect("hint present");
    assert!(note.len() < 1_024, "a long command must not inflate the hint: {} bytes", note.len());
    assert!(!note.contains(&"x".repeat(4_000)), "the command body must be truncated");
    assert!(note.contains(&session_id));

    registry.close_harness_exec_session(&session_id).await.expect("close session");
}

#[tokio::test]
#[cfg(unix)]
async fn exec_session_resume_note_shape_is_stable_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;

    let run = registry
        .execute_public_tool_ref(
            vtcode_core::config::constants::tools::EXEC_COMMAND,
            &json!({"cmd": "sleep 5", "yield_time_ms": 100}),
        )
        .await
        .expect("run should start");
    let session_id = run["session_id"].as_str().expect("session id present").to_string();

    let note = super::build_exec_session_resume_note(&registry).await.expect("hint present");
    assert!(note.starts_with("Exec session resume:"), "stable prefix: {note}");
    assert!(note.contains("are still running:"), "stable running header: {note}");
    assert!(
        note.contains(&format!("- {session_id} (`")),
        "stable session line without guessing shell prefix or elapsed secs: {note}"
    );
    assert!(note.contains("sleep 5"), "stable command body: {note}");
    assert!(note.contains("Settle them before starting new work"), "stable settle directive: {note}");
    assert!(
        note.contains(&format!(
            "{{\"session_id\": \"{session_id}\", \"action\": \"wait\", \"wait_timeout_seconds\": 600}}"
        )),
        "stable pre-filled wait shape: {note}"
    );
    assert!(note.contains("exempt from the per-turn"), "stable budget exemption: {note}");
    assert!(note.contains("deadline-expired wait"), "stable deadline language: {note}");
    assert!(note.len() < 1_024, "contract stays bounded: {} bytes", note.len());

    registry.close_harness_exec_session(&session_id).await.expect("close session");
}

#[tokio::test]
async fn cancelled_note_preparation_does_not_publish_transient_history() {
    use super::{Notify, ToolRegistry};
    use crate::agent::runloop::unified::state::CtrlCState;
    use crate::agent::runloop::unified::stop_requests::await_with_stop;
    use vtcode_core::llm::provider::Message;
    let temp = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let state = CtrlCState::new();
    let notify = Notify::new();
    let mut history = vec![Message::user("preserved request".to_owned())];
    let preparation = async {
        let notes = super::prepare_transient_turn_notes(
            temp.path(),
            &registry,
            Some("dirty workspace evidence".to_owned()),
            Some("completed background job".to_owned()),
        )
        .await;
        assert_eq!(notes, ["dirty workspace evidence", "completed background job"]);
        // Exercise the harder boundary: preparation finished, but stop is
        // accepted before its result is admitted to the request history.
        state.request_local_cancel();
        notes
    };
    if let Some(notes) = await_with_stop(&state, &notify, preparation).await {
        history.extend(notes.into_iter().map(Message::system));
    }
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_text(), "preserved request");
}

#[tokio::test]
async fn prepare_transient_turn_notes_omits_resume_hint_without_sessions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
    let transient = super::prepare_transient_turn_notes(temp.path(), &registry, None, None).await;
    assert!(!transient.iter().any(|note| note.starts_with("Exec session resume:")));
    assert!(transient.is_empty(), "no sessions must produce no notes");
}

#[tokio::test]
#[cfg(unix)]
async fn prepare_transient_turn_notes_injects_bounded_resume_hint() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
    let run = registry
        .execute_public_tool_ref(
            vtcode_core::config::constants::tools::EXEC_COMMAND,
            &json!({"cmd": "sleep 5", "yield_time_ms": 100}),
        )
        .await
        .expect("run should start");
    let session_id = run["session_id"].as_str().expect("session id present").to_string();

    let transient = super::prepare_transient_turn_notes(temp.path(), &registry, None, None).await;

    let hint = transient
        .iter()
        .find(|note| note.starts_with("Exec session resume:"))
        .expect("transient list must carry the resume hint");
    assert!(hint.contains(&session_id));
    assert!(hint.len() < 1_024);
    assert_eq!(transient.len(), 1, "exactly one resume note");

    // Hint suggests the wait; it must not settle the session on its own.
    assert!(
        !registry.in_progress_exec_sessions(4).await.is_empty(),
        "injection must not auto-wait the live session"
    );
    registry.close_harness_exec_session(&session_id).await.expect("close session");
}

#[tokio::test]
#[cfg(unix)]
async fn prepare_transient_turn_notes_omits_retained_background_sessions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let registry = vtcode_core::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
    let run = registry
        .execute_public_tool_ref(
            vtcode_core::config::constants::tools::EXEC_COMMAND,
            &json!({"cmd": "sleep 5", "background": true, "yield_time_ms": 100}),
        )
        .await
        .expect("background run should start");
    let session_id = run["session_id"].as_str().expect("session id present").to_string();

    let transient = super::prepare_transient_turn_notes(temp.path(), &registry, None, None).await;

    assert!(!transient.iter().any(|note| note.starts_with("Exec session resume:")));
    assert!(transient.is_empty(), "retained background work must not force a resume wait");
    registry.close_harness_exec_session(&session_id).await.expect("close session");
}

#[test]
fn pending_approved_plan_checklist_cannot_be_completed() {
    let checklist = json!({
        "total": 3,
        "completed": 1,
        "pending": 1,
        "in_progress": 1,
        "blocked": 0
    });
    let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

    assert_eq!(classify_execution_summary(&result, false, Some(&checklist), true), ExecutionSummaryStatus::Blocked);
}

#[test]
fn completed_approved_plan_requires_all_checklist_items() {
    let checklist = json!({
        "total": 2,
        "completed": 2,
        "pending": 0,
        "in_progress": 0,
        "blocked": 0
    });
    let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

    assert_eq!(
        classify_execution_summary(&result, false, Some(&checklist), true),
        ExecutionSummaryStatus::Completed
    );
}

#[test]
fn recovery_fallback_is_blocked_even_with_a_complete_tracker() {
    let checklist = json!({
        "total": 1,
        "completed": 1,
        "pending": 0,
        "in_progress": 0,
        "blocked": 0
    });
    let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

    assert_eq!(classify_execution_summary(&result, true, Some(&checklist), true), ExecutionSummaryStatus::Blocked);
}

#[test]
fn completed_plan_without_file_changes_is_blocked() {
    let checklist = json!({
        "total": 1,
        "completed": 1,
        "pending": 0,
        "in_progress": 0,
        "blocked": 0,
        "items": [{"description": "Implement feature", "status": "completed"}]
    });
    let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

    assert_eq!(classify_execution_summary(&result, false, Some(&checklist), false), ExecutionSummaryStatus::Blocked);
}

#[test]
fn completed_review_only_plan_without_file_changes_is_completed() {
    let checklist = json!({
        "total": 3,
        "completed": 3,
        "pending": 0,
        "in_progress": 0,
        "blocked": 0,
        "items": [
            {"description": "Review background completion monitoring", "status": "completed"},
            {"description": "Review bounded completion-event handling", "status": "completed"},
            {"description": "Inspect additional runtime diffs", "status": "completed"}
        ]
    });
    let result = TurnLoopResult::Completed { plan_approved_execution_pending: false };

    assert_eq!(
        classify_execution_summary(&result, false, Some(&checklist), false),
        ExecutionSummaryStatus::Completed
    );
    assert!(super::checklist_is_review_only(&checklist));
}

#[test]
fn review_only_checklist_rejects_mutation_descriptions() {
    let mixed = json!({
        "items": [
            {"description": "Review background completion", "status": "completed"},
            {"description": "Implement fix", "status": "completed"}
        ]
    });
    assert!(!super::checklist_is_review_only(&mixed));

    let empty_items = json!({"items": []});
    assert!(!super::checklist_is_review_only(&empty_items));

    let missing_items = json!({"total": 1});
    assert!(!super::checklist_is_review_only(&missing_items));

    // Asymmetric boundary: colon/suffix forms still count as mutation,
    // and `prefix` fail-closes toward implementation (safe direction:
    // preserves the file-change requirement rather than waiving it).
    let fix_colon = json!({"items": [{"description": "Review then Fix: race", "status": "completed"}]});
    assert!(!super::checklist_is_review_only(&fix_colon));

    let prefix_mention = json!({"items": [{"description": "Review prefix handling", "status": "completed"}]});
    assert!(!super::checklist_is_review_only(&prefix_mention));
}

fn pre_turn_history() -> Vec<vtcode_core::llm::provider::Message> {
    use vtcode_core::llm::provider::Message;
    vec![
        Message::system("system prompt".to_string()),
        Message::user("first request".to_string()),
        Message::assistant("first answer".to_string()),
    ]
}

fn run_refused_turn(history: &mut Vec<vtcode_core::llm::provider::Message>) {
    use vtcode_core::llm::provider::Message;
    // Transient note, tool round-trip, and a partial assistant response
    // accumulated before the provider refused.
    history.push(Message::system("Freshness note: transient".to_string()));
    history.push(Message::assistant("partial answer".to_string()));
    history.push(Message::system("recovery directive".to_string()));
}

#[test]
fn refused_turn_rollback_restores_history_before_the_turn() {
    use vtcode_core::llm::provider::Message;
    let before = pre_turn_history();
    let mut history = before.clone();
    let prompt_index = history.len();
    history.push(Message::user("refused request".to_string()));

    let rollback = RefusedTurnRollback::capture(&history, Some(prompt_index), "refused request");
    run_refused_turn(&mut history);

    assert!(rollback.apply(&mut history));
    assert_eq!(history, before);
}

#[test]
fn refused_turn_rollback_finds_queued_follow_up_prompt_without_index() {
    use vtcode_core::llm::provider::Message;
    let before = pre_turn_history();
    let mut history = before.clone();
    history.push(Message::user("queued follow-up".to_string()));

    let rollback = RefusedTurnRollback::capture(&history, None, "queued follow-up");
    run_refused_turn(&mut history);

    assert!(rollback.apply(&mut history));
    assert_eq!(history, before);
}

#[test]
fn refused_turn_rollback_relocates_shifted_prompt() {
    use vtcode_core::llm::provider::Message;
    let mut history = pre_turn_history();
    let prompt_index = history.len();
    history.push(Message::user("refused request".to_string()));
    let rollback = RefusedTurnRollback::capture(&history, Some(prompt_index), "refused request");

    // An in-turn rewrite removed an earlier message, shifting the prompt.
    history.remove(0);
    let rewritten_prefix = history[..prompt_index - 1].to_vec();
    run_refused_turn(&mut history);

    assert!(rollback.apply(&mut history));
    assert_eq!(history, rewritten_prefix);
}

#[test]
fn refused_turn_rollback_leaves_history_when_boundary_is_lost() {
    use vtcode_core::llm::provider::Message;
    let mut history = pre_turn_history();
    let prompt_index = history.len();
    history.push(Message::user("refused request".to_string()));
    let rollback = RefusedTurnRollback::capture(&history, Some(prompt_index), "refused request");

    history.pop();
    history.push(Message::user("summarized request".to_string()));
    let unchanged = history.clone();

    assert!(!rollback.apply(&mut history));
    assert_eq!(history, unchanged);
}

#[test]
fn withdrawn_turn_changes_note_lists_bounded_relative_paths() {
    let workspace = std::path::Path::new("/workspace");
    assert!(super::build_withdrawn_turn_changes_note(workspace, &Default::default()).is_none());

    let paths: std::collections::BTreeSet<std::path::PathBuf> = (0..super::MAX_WITHDRAWN_TURN_PATHS + 3)
        .map(|i| workspace.join(format!("src/f{i:02}.rs")))
        .collect();
    let note = super::build_withdrawn_turn_changes_note(workspace, &paths).expect("note");
    assert!(note.contains("- src/f00.rs\n"), "{note}");
    assert!(!note.contains("/workspace/"), "paths must be workspace-relative: {note}");
    assert_eq!(note.matches("\n- src/").count(), super::MAX_WITHDRAWN_TURN_PATHS);
    assert!(note.contains("- and 3 more\n"), "{note}");
    assert!(note.contains("read them again"), "{note}");
}

#[test]
fn refused_turn_rollback_without_prompt_restores_pre_turn_length() {
    let before = pre_turn_history();
    let mut history = before.clone();
    let rollback = RefusedTurnRollback::capture(&history, None, "prompt that was never appended");
    run_refused_turn(&mut history);

    assert!(rollback.apply(&mut history));
    assert_eq!(history, before);
}
