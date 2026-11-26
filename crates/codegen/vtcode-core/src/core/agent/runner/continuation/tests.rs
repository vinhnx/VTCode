use super::*;
use tempfile::TempDir;

fn make_controller(
    temp: &TempDir,
    continuation_policy: ContinuationPolicy,
    review_like: bool,
) -> ContinuationController {
    make_controller_with_flags(temp, continuation_policy, true, false, review_like)
}

fn make_controller_with_flags(
    temp: &TempDir,
    continuation_policy: ContinuationPolicy,
    full_auto_enabled: bool,
    planning_active: bool,
    review_like: bool,
) -> ContinuationController {
    ContinuationController::new(
        temp.path().to_path_buf(),
        PlanningWorkflowState::new(temp.path().to_path_buf()),
        continuation_policy,
        full_auto_enabled,
        planning_active,
        review_like,
        ContextResetMode::default(),
        2,
    )
}

fn sample_task() -> Task {
    Task {
        id: "exec-task".to_string(),
        title: "Exec Task".to_string(),
        description: "Implement the change".to_string(),
        instructions: None,
    }
}

#[tokio::test]
async fn tracker_adoption_survives_compaction_before_first_assessment() {
    use crate::llm::provider::{Message, ToolCall};
    use crate::tools::registry::ToolRegistry;

    let temp = TempDir::new().expect("tempdir");
    let mut previous = make_controller(&temp, ContinuationPolicy::All, false);
    previous.prepare(&sample_task()).await.expect("old scaffold");
    let mut state = AgentSessionState::new("fresh-session".to_string(), 5, 10_000);
    state.messages = std::sync::Arc::new(vec![
        Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                "old-adoption".to_string(),
                "task_tracker".to_string(),
                json!({"action":"update", "index":1, "status":"in_progress"}).to_string(),
            )],
        ),
        Message::tool_response("old-adoption".to_string(), json!({"status":"updated"}).to_string()),
    ]);
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.begin_tracker_request(false);
    let mut current = make_controller(&temp, ContinuationPolicy::All, false);
    current.prepare(&sample_task()).await.expect("fresh task");
    assert!(
        current
            .incomplete_tracker_labels(registry.tracker_adopted_for_request())
            .await
            .expect("unowned labels")
            .is_empty()
    );
    assert!(matches!(
        current
            .assess_completion(&sample_task(), &state, registry.tracker_adopted_for_request())
            .await
            .expect("bootstrap assessment"),
        CompletionAssessment::SkipAccept { .. }
    ));

    registry
        .execute_tool("task_tracker", json!({"action":"update", "index":1, "status":"in_progress"}))
        .await
        .expect("current adoption");
    // Compact before the first assessment following the actual mutation.
    state.messages = std::sync::Arc::new(vec![Message::user("Compacted current-task progress".to_string())]);
    assert!(
        !current
            .incomplete_tracker_labels(registry.tracker_adopted_for_request())
            .await
            .expect("retained ownership")
            .is_empty()
    );
    assert!(matches!(
        current
            .assess_completion(&sample_task(), &state, registry.tracker_adopted_for_request())
            .await
            .expect("adopted assessment"),
        CompletionAssessment::Continue { .. }
    ));
    registry.begin_tracker_request(false);
    assert!(
        current
            .incomplete_tracker_labels(registry.tracker_adopted_for_request())
            .await
            .expect("fresh request")
            .is_empty()
    );
}

#[tokio::test]
async fn existing_internal_scaffold_is_not_adopted_by_a_new_task() {
    let temp = TempDir::new().expect("tempdir");
    let mut previous = make_controller(&temp, ContinuationPolicy::All, false);
    previous.prepare(&sample_task()).await.expect("old scaffold");
    let tracker_path = temp.path().join(".vtcode/tasks/current_task.md");
    let before = std::fs::read_to_string(&tracker_path).expect("old tracker");
    let mut current = make_controller(&temp, ContinuationPolicy::All, false);
    current.prepare(&sample_task()).await.expect("fresh task");
    let state = AgentSessionState::new("fresh-session".to_string(), 5, 10_000);
    assert!(current.incomplete_tracker_labels(false).await.expect("labels").is_empty());
    assert!(matches!(
        current
            .assess_completion(&sample_task(), &state, false)
            .await
            .expect("assessment"),
        CompletionAssessment::SkipAccept { .. }
    ));
    assert_eq!(std::fs::read_to_string(&tracker_path).expect("preserved tracker"), before);
}

#[tokio::test]
async fn prepare_creates_internal_scaffold_when_missing() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);

    controller.prepare(&sample_task()).await.expect("prepare");

    let checklist = controller.load_tracker().await.expect("load").expect("checklist");
    assert!(is_internal_scaffold(&checklist));
}

#[tokio::test]
async fn assess_completion_requests_continuation_when_tracker_incomplete() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);
    controller.prepare(&sample_task()).await.expect("prepare");

    let session_state = AgentSessionState::new("session".to_string(), 5, 10_000);
    let assessment = controller
        .assess_completion(&sample_task(), &session_state, false)
        .await
        .expect("assessment");

    assert!(matches!(assessment, CompletionAssessment::Continue { .. }));
}

#[tokio::test]
async fn verification_failure_requests_continuation() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);
    controller.prepare(&sample_task()).await.expect("prepare");

    let assessment = controller
        .after_verification(&[VerificationResult {
            command: "cargo check".to_string(),
            success: false,
            exit_code: Some(101),
            output: "error: failed".to_string(),
        }])
        .await
        .expect("verification");

    assert!(matches!(assessment, CompletionAssessment::Continue { .. }));
}

#[tokio::test]
async fn review_like_task_skips_continuation() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller(&temp, ContinuationPolicy::All, true);
    controller.prepare(&sample_task()).await.expect("prepare");

    let session_state = AgentSessionState::new("session".to_string(), 5, 10_000);
    let assessment = controller
        .assess_completion(&sample_task(), &session_state, false)
        .await
        .expect("assessment");

    assert!(matches!(assessment, CompletionAssessment::SkipAccept { .. }));
}

#[tokio::test]
async fn off_policy_skips_continuation() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller(&temp, ContinuationPolicy::Off, false);
    controller.prepare(&sample_task()).await.expect("prepare");

    let session_state = AgentSessionState::new("session".to_string(), 5, 10_000);
    let assessment = controller
        .assess_completion(&sample_task(), &session_state, false)
        .await
        .expect("assessment");

    assert!(matches!(assessment, CompletionAssessment::SkipAccept { .. }));
}

#[tokio::test]
async fn exec_only_policy_skips_non_full_auto_sessions() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller_with_flags(&temp, ContinuationPolicy::ExecOnly, false, false, false);
    controller.prepare(&sample_task()).await.expect("prepare");

    let session_state = AgentSessionState::new("session".to_string(), 5, 10_000);
    let assessment = controller
        .assess_completion(&sample_task(), &session_state, false)
        .await
        .expect("assessment");

    assert!(matches!(assessment, CompletionAssessment::SkipAccept { .. }));
}

#[test]
fn tracker_incomplete_continue_prompt_names_open_steps_and_blocker_exit() {
    let prompt = tracker_incomplete_continue_prompt("#2 change (pending), #3 verify (pending)");
    assert!(prompt.contains("#2 change (pending), #3 verify (pending)"));
    assert!(prompt.contains("in this run"));
    assert!(prompt.contains("what is blocking it"));
    assert!(!prompt.contains("Do not stop yet"));
}

#[test]
fn tracker_status_force_continue_eligible_gates() {
    use super::tracker_status_force_continue_eligible as eligible;
    assert!(eligible(true, false, false, "Task tracker is incomplete: #2 change (pending)."));
    assert!(eligible(true, false, false, "Task tracker still has incomplete steps: #3 verify (pending)."));
    assert!(!eligible(true, false, false, "Task tracker could not be loaded."));
    assert!(!eligible(false, false, false, "Task tracker is incomplete: #2 change (pending)."));
    assert!(!eligible(true, true, false, "Task tracker is incomplete: #2 change (pending)."));
    assert!(!eligible(true, false, true, "Task tracker is incomplete: #2 change (pending)."));
    assert!(!eligible(true, false, false, "Scaffold created for analysis."));
}

#[tokio::test]
async fn incomplete_tracker_labels_are_read_only_when_absent() {
    let temp = TempDir::new().expect("tempdir");
    let controller = make_controller(&temp, ContinuationPolicy::All, false);
    let labels = controller.incomplete_tracker_labels(false).await.expect("labels");
    assert!(labels.is_empty(), "absent tracker must not invent incomplete work");
}

#[test]
fn tracker_status_text_safety_handoff_shared_vocabulary() {
    use crate::core::agent::completion::tracker_final_text_is_safety_handoff as handoff;
    assert!(handoff("Permission denied for exec_command. Next step: retry after access is granted."));
    assert!(handoff("I hit the tool-call safety fuse mid-verification; policy block."));
    assert!(!handoff("## Status\nBlocked by turn budget. Next step: read design/diff.rs."));
}
#[tokio::test]
async fn empty_or_missing_terminal_verification_results_cannot_be_accepted() {
    let temp = TempDir::new().unwrap();
    let mut controller = make_controller(&temp, ContinuationPolicy::Off, false);
    for results in [
        vec![],
        vec![VerificationResult {
            command: "cargo check".into(),
            success: true,
            exit_code: None,
            output: "still running".into(),
        }],
    ] {
        assert!(matches!(
            controller.after_verification(&results).await.unwrap(),
            CompletionAssessment::Continue { .. }
        ));
    }
}

#[test]
fn verification_failure_signal_localizes_without_copying_output() {
    let failure = VerificationResult {
        command: "cargo nextest run".into(),
        success: false,
        exit_code: Some(101),
        output: "test auth::expired_token ... FAILED\nleftover SECRET_USER_TOKEN_XYZ".into(),
    };

    let payload = build_verification_failure_payload(&failure);
    let prompt = build_verification_failure_prompt(&failure);

    for text in [&payload, &prompt] {
        assert!(text.contains("Verification failed: cargo nextest run (exit code 101)."));
        assert!(text.contains("Failure shape: test at auth::expired_token (fingerprint "), "{text}");
    }
    assert!(payload.contains("leftover SECRET_USER_TOKEN_XYZ"), "payload keeps the bounded raw tail");
    let shape_only = vtcode_commons::verification_signal::extract_verification_failure_shape(&failure.output);
    assert!(!shape_only.describe().contains("SECRET_USER_TOKEN_XYZ"));
}

fn failed_results(output: &str) -> Vec<VerificationResult> {
    vec![VerificationResult {
        command: "cargo nextest run".into(),
        success: false,
        exit_code: Some(101),
        output: output.into(),
    }]
}

fn continue_prompt(assessment: CompletionAssessment) -> String {
    match assessment {
        CompletionAssessment::Continue { prompt, .. } => prompt,
        other => panic!("expected a Continue assessment, got {other:?}"),
    }
}

#[tokio::test]
async fn repeated_failure_shape_adds_anti_thrash_nudge() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);
    controller.prepare(&sample_task()).await.expect("prepare");

    // Same test, different assertion text: one bug, one fingerprint.
    let first = controller
        .after_verification(&failed_results("test auth::expired ... FAILED\nleft differs"))
        .await
        .expect("first failure");
    assert_eq!(controller.fingerprint_streak, 1);
    assert!(!continue_prompt(first).contains("change your approach"));

    let second = controller
        .after_verification(&failed_results("test auth::expired ... FAILED\nright differs"))
        .await
        .expect("second failure");
    assert_eq!(controller.fingerprint_streak, 2);
    let prompt = continue_prompt(second);
    assert!(prompt.contains("2 times consecutively"), "{prompt}");
    assert!(prompt.contains("change your approach"), "{prompt}");
}

#[tokio::test]
async fn new_shape_and_success_reset_the_streak() {
    let temp = TempDir::new().expect("tempdir");
    let mut controller = make_controller(&temp, ContinuationPolicy::ExecOnly, false);
    controller.prepare(&sample_task()).await.expect("prepare");

    controller
        .after_verification(&failed_results("test auth::expired ... FAILED"))
        .await
        .expect("first failure");
    let moved = controller
        .after_verification(&failed_results("test cache::other ... FAILED"))
        .await
        .expect("different shape");
    assert_eq!(controller.fingerprint_streak, 1, "a new shape starts a fresh streak");
    assert!(!continue_prompt(moved).contains("change your approach"));

    let passed = controller
        .after_verification(&[VerificationResult {
            command: "cargo nextest run".into(),
            success: true,
            exit_code: Some(0),
            output: "test result: ok".into(),
        }])
        .await
        .expect("pass");
    assert!(matches!(passed, CompletionAssessment::Accept));
    assert_eq!(controller.fingerprint_streak, 0, "a pass clears the streak");

    let relapsed = controller
        .after_verification(&failed_results("test auth::expired ... FAILED"))
        .await
        .expect("relapse");
    assert_eq!(controller.fingerprint_streak, 1);
    assert!(!continue_prompt(relapsed).contains("change your approach"));
}
