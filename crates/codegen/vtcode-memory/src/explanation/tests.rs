use super::*;
use vtcode_exec_events::*;

fn turn(log: &SessionEventLog, task: &str, goal: &str, origin: InputOrigin) {
    let mut event = TurnStartedEvent::default();
    event.context = Some(Box::new(ExecutionContext {
        task_id: task.into(),
        turn_id: format!("turn-{task}"),
        actor_id: "root".into(),
        parent_actor_id: None,
        origin,
        timestamp: "2026-10-03T00:00:00Z".into(),
        goal: Some(goal.into()),
    }));
    log.append(&ThreadEvent::TurnStarted(event)).unwrap();
}

#[test]
fn cancelled_calls_are_distinct_from_errors_and_do_not_verify() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Inspect results", InputOrigin::User);
    for (id, outcome, status, activity, exit_code) in [
        ("closure", ToolOutcome::Cancelled, ToolCallStatus::Failed, None, None),
        ("error", ToolOutcome::Error, ToolCallStatus::Failed, None, Some(2)),
        (
            "empty-search",
            ToolOutcome::Success,
            ToolCallStatus::Completed,
            Some(CommandActivity::Inspection),
            Some(1),
        ),
        (
            "cancelled-check",
            ToolOutcome::Cancelled,
            ToolCallStatus::Failed,
            Some(CommandActivity::Verification),
            None,
        ),
    ] {
        item(
            &log,
            "a",
            id,
            activity,
            ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "exec_command".into(),
                arguments: None,
                tool_call_id: None,
                status: status.clone(),
                outcome: Some(outcome),
            })),
        );
        item(
            &log,
            "a",
            &format!("{id}:output"),
            activity,
            ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: id.into(),
                tool_call_id: None,
                output: "retained evidence".into(),
                exit_code,
                spool_path: None,
                status,
            })),
        );
    }
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.actions.len(), 4);
    assert_eq!(
        model.actions.iter().map(|action| action.status.as_str()).collect::<Vec<_>>(),
        vec!["cancelled", "failed or denied", "completed", "failed or denied"]
    );
    assert_eq!(model.failures.len(), 2);
    assert_eq!(model.verification.len(), 1);
    assert_eq!(model.verification[0].fact.status, "failed or denied");
    assert!(!model.verification[0].fresh);
}

fn item(log: &SessionEventLog, task: &str, id: &str, activity: Option<CommandActivity>, details: ThreadItemDetails) {
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: id.into(),
            context: Some(Box::new(ItemContext {
                task_id: task.into(),
                turn_id: format!("turn-{task}"),
                actor_id: "root".into(),
                parent_actor_id: None,
                timestamp: "2026-10-03T00:00:01Z".into(),
                activity,
            })),
            details,
        },
    }))
    .unwrap();
}

fn edit(log: &SessionEventLog, task: &str, id: &str, path: &str, status: PatchApplyStatus) {
    item(
        log,
        task,
        id,
        None,
        ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            changes: vec![FileUpdateChange { path: path.into(), kind: PatchChangeKind::Update }],
            status,
            diff_incomplete: None,
            unified_diff: Some(format!("--- a/{path}\n+++ b/{path}\n@@ -3,1 +4,1 @@\n-old\n+pub fn new() {{}}\n")),
            additions: Some(1),
            deletions: Some(1),
        })),
    );
}

fn verify(log: &SessionEventLog, task: &str, id: &str, exit_code: Option<i32>, status: CommandExecutionStatus) {
    item(
        log,
        task,
        id,
        Some(CommandActivity::Verification),
        ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check --locked".into(),
            arguments: None,
            aggregated_output: "secret output".into(),
            exit_code,
            status,
        })),
    );
}

#[test]
fn latest_task_preserves_continuations_and_deduplicates_edits() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "old", "Old request", InputOrigin::User);
    edit(&log, "old", "old-edit", "unrelated.rs", PatchApplyStatus::Completed);
    turn(&log, "new", "Fix storage", InputOrigin::User);
    edit(&log, "new", "failed", "ghost.rs", PatchApplyStatus::Failed);
    edit(&log, "new", "same", "src/storage.rs", PatchApplyStatus::Completed);
    edit(&log, "new", "same", "src/storage.rs", PatchApplyStatus::Completed);
    turn(&log, "new", "Fix storage", InputOrigin::Continuation);
    edit(&log, "new", "again", "src/storage.rs", PatchApplyStatus::Completed);
    verify(&log, "new", "check", Some(0), CommandExecutionStatus::Completed);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.goals.len(), 1);
    assert_eq!(model.goals[0].label, "Fix storage");
    assert_eq!(model.edit_operations, 2);
    assert_eq!(model.changes.len(), 1);
    assert_eq!(model.changes[0].line, Some(4));
    assert_eq!(model.failures.len(), 1);
    assert!(model.verification[0].fresh);
    assert_eq!(model.review_priorities[0].priority, ReviewPriority::High);
    let all = query_explanation(&log, ExplanationScope::Session).unwrap();
    assert_eq!(all.goals.len(), 2);
    assert_eq!(all.changes.len(), 2);
    assert!(render_summary(&model).lines().count() <= 20);
}

#[test]
fn only_completed_exit_zero_verifies_and_mutations_make_checks_stale() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    verify(&log, "a", "fail", Some(1), CommandExecutionStatus::Failed);
    verify(&log, "a", "success", Some(0), CommandExecutionStatus::Completed);
    edit(&log, "a", "edit", "a.rs", PatchApplyStatus::Completed);
    verify(&log, "a", "missing", None, CommandExecutionStatus::Completed);
    verify(&log, "a", "pending", Some(0), CommandExecutionStatus::InProgress);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(
        model.verification.iter().map(|v| v.fact.status.as_str()).collect::<Vec<_>>(),
        vec!["failed", "passed", "unconfirmed", "pending"]
    );
    assert!(model.verification.iter().all(|v| !v.fresh));
    assert!(model.review_priorities.iter().any(|r| r.reason.contains("last mutation")));
}

#[test]
fn retained_snapshot_exceeds_live_buffer_and_evidence_is_paged_and_bound() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "A", InputOrigin::User);
    for n in 0..9999 {
        verify(&log, "a", &format!("v{n}"), Some(0), CommandExecutionStatus::Completed);
    }
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.verification.len(), 9999);
    assert!(render_summary(&model).contains("9997 entries omitted"));
    let reference = &model.verification[0].fact.evidence;
    let page = query_evidence(&log, reference, 0, 17).unwrap();
    assert_eq!(page.text.len(), 17);
    assert!(page.next_offset.is_some());
    let mut changed = reference.clone();
    changed.digest = "wrong".into();
    assert!(query_evidence(&log, &changed, 0, 100).is_err());
    changed = reference.clone();
    changed.session_id = "other".into();
    assert!(query_evidence(&log, &changed, 0, 100).is_err());
    assert_eq!(model.revision, query_explanation(&log, ExplanationScope::Task).unwrap().revision);
}

#[test]
fn offline_html_escapes_hostile_text_and_never_loads_network_resources() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "</pre><script>alert(1)</script>", InputOrigin::User);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    let page = query_evidence(&log, &model.goals[0].evidence, 0, 32768).unwrap();
    let html = render_html(&model, &[page]);
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("default-src 'none'"));
    assert!(html.contains(&format!("href=\"#evidence-{}\"", model.goals[0].evidence.offset)));
    assert!(render_summary(&model).lines().count() <= 20);
}

#[test]
fn recorded_delegation_preserves_ancestry_status_and_failure_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Delegate review", InputOrigin::User);
    let event = ThreadEvent::ItemCompleted(ItemCompletedEvent { item: ThreadItem {
        id: "delegation".into(),
        context: Some(Box::new(ItemContext {
            task_id: "a".into(), turn_id: "turn-a".into(), actor_id: "reviewer".into(), parent_actor_id: Some("root".into()), timestamp: "2026-10-03T00:00:01Z".into(), activity: None,
        })),
        details: ThreadItemDetails::Harness(Box::new(serde_json::from_value(serde_json::json!({
            "event": HarnessEventKind::DelegatedAgentStatus, "status":"failed", "message":"Delegated reviewer: inspect storage", "session_id":"reviewer",
        })).unwrap())),
    }});
    log.append(&event).unwrap();
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.actions.len(), 1);
    assert_eq!(model.actions[0].actor_id.as_deref(), Some("reviewer"));
    assert_eq!(model.actions[0].parent_actor_id.as_deref(), Some("root"));
    assert_eq!(model.actions[0].status, "failed");
    assert_eq!(model.failures[0].evidence, model.actions[0].evidence);
    assert!(
        model
            .graph
            .iter()
            .any(|edge| edge.from == "root" && edge.to == "reviewer" && edge.relation == "delegated agent")
    );
    assert!(render_diagram(&model, 80).contains("root -> reviewer (delegated agent)"));
    assert!(render_html(&model, &[]).contains("root → reviewer"));
}

#[test]
fn offline_sections_link_full_facts_and_preserve_missing_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix storage", InputOrigin::User);
    edit(&log, "a", "edit", "src/storage.rs", PatchApplyStatus::Completed);
    verify(&log, "a", "check", Some(0), CommandExecutionStatus::Completed);
    item(
        &log,
        "a",
        "decision",
        None,
        ThreadItemDetails::Decision(Box::new(DecisionItem {
            summary: "Use retained events".into(),
            rationale: "Keep <untrusted> evidence traceable".into(),
            alternatives: vec!["Extra database".into()],
            evidence_ids: vec!["edit".into()],
        })),
    );
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    let reference = &model.decisions[0].fact.evidence;
    let page = query_evidence(&log, reference, 0, 32768).unwrap();
    let html = render_html(&model, &[page]);
    for section in [
        "Timeline",
        "Captured changes",
        "Decisions",
        "Verification",
        "Review first",
        "Plan evolution",
        "Agent tree",
        "Usage and timing",
        "Recorded action and file graph",
    ] {
        assert!(html.contains(section), "missing {section}");
    }
    assert!(html.contains("Agent-reported rationale: Keep &lt;untrusted&gt; evidence traceable"));
    assert!(html.contains("Alternative: Extra database"));
    assert!(html.contains("Exit code: Some(0). Fresh: true."));
    assert!(html.contains("evidence unavailable in this report"));
    assert!(html.contains(&format!("href=\"#evidence-{}\"", reference.offset)));
    assert!(html.contains("src/storage.rs (changed file)"));
    assert!(html.contains("Recorded edit → src/storage.rs"));
    assert!(!html.contains("<untrusted>"));
    assert!(page_explanation(&model, 0).workspace_diff.is_none());
    let workspace = WorkspaceDiffSnapshot {
        captured_at: "2026-10-03T00:00:00Z".into(),
        text: Some("+<script>human edit</script>".into()),
        truncated: true,
        note: "Ownership is not attributed to the agent.".into(),
    };
    let report = render_html_with_workspace(&model, &[], Some(&workspace));
    assert!(report.contains("+&lt;script&gt;human edit&lt;/script&gt;"));
    assert!(report.contains("Current diff truncated"));
    assert!(!report.contains("<script>"));
    assert_eq!(model.changes.len(), 1);
}

#[test]
fn terminal_summary_escapes_recorded_markdown_and_offline_facts_stay_literal() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "[unsafe](https://example.test) <script> ```", InputOrigin::User);
    edit(&log, "a", "edit", "src/[unsafe](https://example.test).rs", PatchApplyStatus::Completed);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    let summary = render_summary(&model);
    assert!(summary.contains("\\[unsafe\\]\\(https://example\\.test\\)"));
    assert!(!summary.contains("[unsafe](https://example.test)"));
    assert!(summary.contains("[evidence](vtcode-evidence:"));
    let html = render_html(&model, &[]);
    assert!(html.contains("src/[unsafe](https://example.test).rs"));
    assert!(!html.contains("href=\"https://"));
    assert!(!html.contains("<script>"));
    assert!(render_details(&model).contains("src/[unsafe](https://example.test).rs"));
}

#[test]
fn summary_reports_omitted_inspections_without_claiming_no_activity() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Inspect workspace", InputOrigin::User);
    item(
        &log,
        "a",
        "inspect",
        Some(CommandActivity::Inspection),
        ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "rg --files".into(),
            arguments: None,
            aggregated_output: "src/main.rs".into(),
            exit_code: Some(0),
            status: CommandExecutionStatus::Completed,
        })),
    );
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.actions.len(), 1);
    assert!(model.changes.is_empty());
    assert!(model.verification.is_empty());
    let summary = render_summary(&model);
    assert!(summary.contains("1 entry omitted; use /explain --details."));
    assert!(summary.lines().count() <= 20);
}

#[test]
fn replayed_completion_does_not_move_a_check_past_a_later_edit() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    verify(&log, "a", "check", Some(0), CommandExecutionStatus::Completed);
    let original = query_explanation(&log, ExplanationScope::Task).unwrap().verification[0]
        .fact
        .evidence
        .clone();
    edit(&log, "a", "edit", "a.rs", PatchApplyStatus::Completed);
    verify(&log, "a", "check", Some(0), CommandExecutionStatus::Completed);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.verification.len(), 1);
    assert_eq!(model.verification[0].fact.evidence, original);
    assert!(!model.verification[0].fresh);
    verify(&log, "a", "new-check", Some(0), CommandExecutionStatus::Completed);
    assert!(query_explanation(&log, ExplanationScope::Task).unwrap().verification[1].fresh);
}

#[test]
fn failed_and_pending_mutations_invalidate_earlier_checks() {
    for (exit_code, status) in [
        (Some(1), CommandExecutionStatus::Failed),
        (None, CommandExecutionStatus::InProgress),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let log = crate::open(temp.path(), "session", 10000).unwrap();
        turn(&log, "a", "Fix a", InputOrigin::User);
        verify(&log, "a", "check", Some(0), CommandExecutionStatus::Completed);
        item(
            &log,
            "a",
            "mutation",
            Some(CommandActivity::Mutation),
            ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: "write a.rs; exit 1".into(),
                arguments: None,
                aggregated_output: String::new(),
                exit_code,
                status,
            })),
        );
        assert!(!query_explanation(&log, ExplanationScope::Task).unwrap().verification[0].fresh);
    }
}

#[test]
fn file_review_signals_and_line_counts_are_scoped_to_their_own_diff() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    let mut diff = String::from("--- a/api.rs\n+++ b/api.rs\n@@ -0,0 +1,200 @@\n+pub fn api() {}\n");
    for _ in 0..199 {
        diff.push_str("+line\n");
    }
    diff.push_str("--- a/readme.md\n+++ b/readme.md\n@@ -1 +1 @@\n-old\n+new\n");
    item(
        &log,
        "a",
        "edit",
        None,
        ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            changes: vec![
                FileUpdateChange {
                    path: "api.rs".into(),
                    kind: PatchChangeKind::Update,
                },
                FileUpdateChange {
                    path: "readme.md".into(),
                    kind: PatchChangeKind::Update,
                },
            ],
            status: PatchApplyStatus::Completed,
            diff_incomplete: None,
            unified_diff: Some(diff),
            additions: Some(201),
            deletions: Some(1),
        })),
    );
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    let api_signals: Vec<_> = model
        .review_priorities
        .iter()
        .filter(|r| r.fact.path.as_deref() == Some("api.rs"))
        .collect();
    assert!(api_signals.iter().any(|r| r.reason.contains("Public API")));
    assert!(api_signals.iter().any(|r| r.reason.contains("200 changed")));
    assert!(
        !model
            .review_priorities
            .iter()
            .any(|r| r.fact.path.as_deref() == Some("readme.md")
                && (r.reason.contains("Public API") || r.reason.contains("200 changed")))
    );
}

#[test]
fn uncaptured_diffs_are_explicitly_incomplete() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    item(
        &log,
        "a",
        "edit",
        None,
        ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            changes: vec![FileUpdateChange { path: "a.rs".into(), kind: PatchChangeKind::Update }],
            status: PatchApplyStatus::Completed,
            diff_incomplete: None,
            unified_diff: None,
            additions: None,
            deletions: None,
        })),
    );
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert!(model.completeness.warnings.iter().any(|w| w.contains("diff unavailable")));
}

#[test]
fn review_signals_follow_source_positions_and_deduplicate_repeated_edits() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Update module", InputOrigin::User);
    edit(&log, "a", "api", "module.rs", PatchApplyStatus::Completed);
    for id in ["large", "large-again"] {
        item(
            &log,
            "a",
            id,
            None,
            ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                changes: vec![FileUpdateChange {
                    path: "module.rs".into(),
                    kind: PatchChangeKind::Update,
                }],
                status: PatchApplyStatus::Completed,
                diff_incomplete: None,
                unified_diff: Some(format!(
                    "--- a/module.rs\n+++ b/module.rs\n@@ -50,0 +50,200 @@\n{}",
                    "+private_line\n".repeat(200)
                )),
                additions: Some(200),
                deletions: Some(0),
            })),
        );
    }
    verify(&log, "a", "check", Some(0), CommandExecutionStatus::Completed);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.review_priorities.len(), 2);
    assert_eq!(
        model
            .review_priorities
            .iter()
            .map(|signal| signal.fact.line)
            .collect::<Vec<_>>(),
        vec![Some(4), Some(50)]
    );
}

#[test]
fn projection_metadata_is_bounded_redacted_and_does_not_merge_long_paths() {
    let runtime_task = "task-8e934822-4b03-4b0c-a263-6642bfd875ab";
    assert_eq!(public_identity(runtime_task), runtime_task);
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    let identity = format!("password={}{}", "secret-value", "x".repeat(5000));
    turn(&log, &identity, "Fix a", InputOrigin::User);
    let long_path = "x".repeat(5000);
    edit(&log, &identity, "one", &format!("{long_path}a.rs"), PatchApplyStatus::Completed);
    edit(&log, &identity, "two", &format!("{long_path}b.rs"), PatchApplyStatus::Completed);
    item(
        &log,
        &identity,
        &identity,
        None,
        ThreadItemDetails::Decision(Box::new(DecisionItem {
            summary: "Choice".into(),
            rationale: "A public rationale".into(),
            alternatives: vec![],
            evidence_ids: vec![identity.clone()],
        })),
    );
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.changes.len(), 2);
    assert_ne!(model.changes[0].path, model.changes[1].path);
    assert!(model.changes.iter().all(|f| f.label.len() < 1100));
    assert!(model.task_id.as_ref().unwrap().len() <= 256);
    assert!(model.decisions[0].evidence_ids[0].len() <= 256);
    assert!(!serde_json::to_string(&model).unwrap().contains("secret-value"));
    assert!(query_evidence(&log, &model.decisions[0].fact.evidence, 0, 32768).is_ok());
}

#[test]
fn projection_pages_are_byte_bounded_and_cover_every_entry() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    for n in 0..100 {
        item(
            &log,
            "a",
            &format!("d{n}"),
            None,
            ThreadItemDetails::Decision(Box::new(DecisionItem {
                summary: "s".repeat(240),
                rationale: "r".repeat(1000),
                alternatives: vec!["a".repeat(1000); 3],
                evidence_ids: vec!["id".repeat(120); 8],
            })),
        );
    }
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    let mut offset = 0;
    let mut count = 0;
    loop {
        let page = page_explanation(&model, offset);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        count += page.model.decisions.len();
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(count, 100);
}

#[test]
fn evidence_redacts_secret_key_variants_and_narrow_diagrams_fit_wide_text() {
    let mut value = serde_json::json!({"client_secret": "short", "api-key": "short", "privateKey": "short", "nested": {"refresh-token": "short"}, "public": "visible"});
    redact_value(&mut value);
    assert_eq!(value["client_secret"], "[REDACTED]");
    assert_eq!(value["api-key"], "[REDACTED]");
    assert_eq!(value["privateKey"], "[REDACTED]");
    assert_eq!(value["nested"]["refresh-token"], "[REDACTED]");
    assert_eq!(value["public"], "visible");
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    edit(&log, "a", "very-long-action-identity", "表表表表表表表.rs", PatchApplyStatus::Completed);
    verify(&log, "a", "check", Some(0), CommandExecutionStatus::Completed);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert!(
        render_diagram(&model, 8)
            .lines()
            .all(|row| vtcode_commons::preview::display_width(row) <= 8)
    );
}

#[test]
fn incomplete_or_failed_tool_output_cannot_verify_a_completed_invocation() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    item(
        &log,
        "a",
        "tool",
        Some(CommandActivity::Verification),
        ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "unified_exec".into(),
            arguments: Some(serde_json::json!({"cmd": "cargo check"})),
            tool_call_id: None,
            status: ToolCallStatus::Completed,
            outcome: Some(ToolOutcome::Success),
        })),
    );
    let output = |status| {
        ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: "tool".into(),
            tool_call_id: None,
            spool_path: None,
            output: String::new(),
            exit_code: Some(0),
            status,
        }))
    };
    log.append(&ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: ThreadItem {
            id: "output".into(),
            context: Some(Box::new(ItemContext {
                task_id: "a".into(),
                turn_id: "turn-a".into(),
                actor_id: "root".into(),
                parent_actor_id: None,
                timestamp: "2026-10-03T00:00:01Z".into(),
                activity: Some(CommandActivity::Verification),
            })),
            details: output(ToolCallStatus::Completed),
        },
    }))
    .unwrap();
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.verification[0].fact.status, "pending");
    assert!(!model.verification[0].fresh);
    item(&log, "a", "output", Some(CommandActivity::Verification), output(ToolCallStatus::Failed));
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.verification[0].fact.status, "failed or denied");
    assert_eq!(model.failures.len(), 1);
    item(&log, "a", "output", Some(CommandActivity::Verification), output(ToolCallStatus::Completed));
    assert!(query_explanation(&log, ExplanationScope::Task).unwrap().verification[0].fresh);
}

#[test]
fn replay_with_a_new_timestamp_preserves_original_completion_position() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    verify(&log, "a", "check", Some(0), CommandExecutionStatus::Completed);
    edit(&log, "a", "edit", "a.rs", PatchApplyStatus::Completed);
    log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: "check".into(),
            context: Some(Box::new(ItemContext {
                task_id: "a".into(),
                turn_id: "turn-a".into(),
                actor_id: "root".into(),
                parent_actor_id: None,
                timestamp: "2026-10-03T00:00:15Z".into(),
                activity: Some(CommandActivity::Verification),
            })),
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: "cargo check --locked".into(),
                arguments: None,
                aggregated_output: "secret output".into(),
                exit_code: Some(0),
                status: CommandExecutionStatus::Completed,
            })),
        },
    }))
    .unwrap();
    assert!(!query_explanation(&log, ExplanationScope::Task).unwrap().verification[0].fresh);
}

#[test]
fn unknown_exit_evidence_does_not_count_as_repeated_failure() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    turn(&log, "a", "Fix a", InputOrigin::User);
    verify(&log, "a", "unknown-one", None, CommandExecutionStatus::Completed);
    verify(&log, "a", "unknown-two", None, CommandExecutionStatus::Completed);
    item(
        &log,
        "a",
        "unknown-tool",
        Some(CommandActivity::Verification),
        ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "unified_exec".into(),
            arguments: Some(serde_json::json!({"cmd": "cargo check"})),
            tool_call_id: None,
            status: ToolCallStatus::Completed,
            outcome: Some(ToolOutcome::Success),
        })),
    );
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.verification.len(), 3);
    assert!(model.verification.iter().all(|v| v.fact.status == "unconfirmed"));
    assert!(model.failures.is_empty());
    assert!(
        !model
            .review_priorities
            .iter()
            .any(|r| r.reason.contains("Repeated recorded failures"))
    );
    assert!(model.completeness.warnings.iter().any(|w| w.contains("3 recorded operations")));
    verify(&log, "a", "known-failure", None, CommandExecutionStatus::Failed);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.failures.len(), 1);
    assert_eq!(model.failures[0].status, "failed");
    assert!(
        !model
            .review_priorities
            .iter()
            .any(|r| r.reason.contains("Repeated recorded failures"))
    );
    verify(&log, "a", "second-failure", Some(1), CommandExecutionStatus::Failed);
    let model = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(model.failures.len(), 2);
    assert!(
        model
            .review_priorities
            .iter()
            .any(|r| r.reason.contains("Repeated recorded failures"))
    );
}

#[test]
fn async_verifier_aliases_complete_once_and_use_launch_time_for_freshness() {
    for intervening_edit in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let log = crate::open(temp.path(), "session", 10000).unwrap();
        turn(&log, "a", "Fix a", InputOrigin::User);
        item(
            &log,
            "a",
            "launch",
            Some(CommandActivity::Verification),
            ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "unified_exec".into(),
                arguments: Some(serde_json::json!({"cmd": "cargo check"})),
                tool_call_id: None,
                status: ToolCallStatus::Completed,
                outcome: Some(ToolOutcome::Success),
            })),
        );
        let output = |status, exit_code| {
            ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "launch".into(),
                tool_call_id: None,
                spool_path: None,
                output: String::new(),
                exit_code,
                status,
            }))
        };
        item(&log, "a", "launch:output", None, output(ToolCallStatus::Completed, None));
        item(&log, "a", "launch:execution-output", None, output(ToolCallStatus::InProgress, None));
        let pending = query_explanation(&log, ExplanationScope::Task).unwrap();
        assert_eq!(pending.verification.len(), 1);
        assert_eq!(pending.verification[0].fact.status, "pending");
        if intervening_edit {
            edit(&log, "a", "edit", "a.rs", PatchApplyStatus::Completed);
        }
        item(
            &log,
            "a",
            "poll",
            None,
            ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "unified_exec".into(),
                arguments: Some(serde_json::json!({"action": "poll", "session_id": "session-1"})),
                tool_call_id: None,
                status: ToolCallStatus::Completed,
                outcome: Some(ToolOutcome::Success),
            })),
        );
        item(&log, "a", "launch:execution-output", None, output(ToolCallStatus::Completed, Some(0)));
        let completed = query_explanation(&log, ExplanationScope::Task).unwrap();
        assert_eq!(completed.verification.len(), 1);
        assert_eq!(completed.verification[0].fact.status, "passed");
        assert_eq!(completed.verification[0].fresh, !intervening_edit);
        assert_eq!(completed.actions.len(), 2);
        let reference = completed.verification[0].fact.evidence.clone();
        assert_eq!(reference.item_id.as_deref(), Some("launch:execution-output"));
        item(&log, "a", "launch:execution-output", None, output(ToolCallStatus::InProgress, None));
        let replay = query_explanation(&log, ExplanationScope::Task).unwrap();
        assert_eq!(replay.verification[0].fact.evidence, reference);
        assert_eq!(replay.verification[0].fact.status, "passed");
    }
}

#[test]
fn token_breakdowns_preserve_recorded_counts_scope_paging_and_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    for (task, tokens) in [("old", 2), ("new", 5)] {
        let event: TurnStartedEvent = serde_json::from_value(serde_json::json!({
            "context": {"task_id": task, "turn_id": format!("turn-{task}"), "actor_id": "root", "origin": "user", "timestamp": "2026-10-03T00:00:00Z", "goal": task},
            "token_breakdown": {"system_prompt_tokens": tokens, "tool_schema_tokens": 7, "instruction_file_tokens": 11, "message_history_tokens": 13, "cache_read_tokens": 17, "cache_write_tokens": 19, "cache_miss_tokens": 23}
        })).unwrap();
        log.append(&ThreadEvent::TurnStarted(event)).unwrap();
    }
    let task = query_explanation(&log, ExplanationScope::Task).unwrap();
    assert_eq!(task.token_breakdowns.len(), 1);
    assert_eq!(serde_json::to_value(task.token_breakdowns[0].breakdown).unwrap()["system_prompt_tokens"], 5);
    let session = query_explanation(&log, ExplanationScope::Session).unwrap();
    assert_eq!(session.token_breakdowns.len(), 2);
    assert_eq!(serde_json::to_value(session.token_breakdowns[0].breakdown).unwrap()["system_prompt_tokens"], 2);
    assert_eq!(page_explanation(&session, 1).model.token_breakdowns.len(), 1);
    assert!(
        session
            .evidence_references()
            .contains(&session.token_breakdowns[0].fact.evidence)
    );
    assert!(render_details(&task).contains("\"system_prompt_tokens\":5"));
    assert!(render_html(&task, &[]).contains("system_prompt_tokens"));
}

#[test]
fn graph_nodes_distinguish_reused_item_ids_across_tasks_and_actors() {
    use std::collections::BTreeSet;
    let temp = tempfile::tempdir().unwrap();
    let log = crate::open(temp.path(), "session", 10000).unwrap();
    for (task, actor, parent, path) in [
        ("one", "root", None, "a.rs"),
        ("one", "child", Some("root"), "b.rs"),
        ("two", "root", None, "c.rs"),
    ] {
        turn(&log, task, task, InputOrigin::User);
        let context = ItemContext {
            task_id: task.into(),
            turn_id: format!("turn-{task}"),
            actor_id: actor.into(),
            parent_actor_id: parent.map(str::to_owned),
            timestamp: "2026-10-03T00:00:01Z".into(),
            activity: None,
        };
        for (id, details) in [
            (
                "same-check",
                ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: format!("check-{path}"),
                    arguments: None,
                    aggregated_output: String::new(),
                    exit_code: Some(0),
                    status: CommandExecutionStatus::Completed,
                })),
            ),
            (
                "same-edit",
                ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                    changes: vec![FileUpdateChange { path: path.into(), kind: PatchChangeKind::Update }],
                    status: PatchApplyStatus::Completed,
                    unified_diff: None,
                    diff_incomplete: Some(true),
                    additions: None,
                    deletions: None,
                })),
            ),
        ] {
            log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: id.into(),
                    context: Some(Box::new(context.clone())),
                    details,
                },
            }))
            .unwrap();
        }
    }
    let model = query_explanation(&log, ExplanationScope::Session).unwrap();
    assert_eq!(model.actions.len(), 3);
    assert_eq!(model.actions[1].actor_id.as_deref(), Some("child"));
    assert_eq!(model.actions[1].parent_actor_id.as_deref(), Some("root"));
    let edges: Vec<_> = model.graph.iter().filter(|edge| edge.relation == "recorded next").collect();
    assert_eq!(edges.len(), 5);
    assert!(edges.iter().all(|edge| edge.from != edge.to));
    let nodes: BTreeSet<_> = edges.iter().flat_map(|edge| [&edge.from, &edge.to]).collect();
    assert_eq!(nodes.len(), 6);
    for action in &model.actions {
        assert_eq!(
            nodes
                .iter()
                .filter(
                    |node| node.starts_with(&format!("event {}:{} ", action.evidence.offset, action.evidence.digest))
                )
                .count(),
            1
        );
    }
    let file_edges: Vec<_> = model.graph.iter().filter(|edge| edge.relation == "changed file").collect();
    assert_eq!(file_edges.iter().map(|edge| &edge.from).collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(
        file_edges.iter().map(|edge| &edge.to).collect::<BTreeSet<_>>(),
        BTreeSet::from([&"a.rs".to_string(), &"b.rs".to_string(), &"c.rs".to_string()])
    );
    assert!(file_edges.iter().all(|edge| nodes.contains(&edge.from)));
}

#[test]
fn verifier_freshness_uses_execution_start_instead_of_streamed_argument_start() {
    for mutation_before_execution in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let log = crate::open(temp.path(), "session", 10000).unwrap();
        turn(&log, "a", "Fix a", InputOrigin::User);
        let context = ItemContext {
            task_id: "a".into(),
            turn_id: "turn-a".into(),
            actor_id: "root".into(),
            parent_actor_id: None,
            timestamp: "2026-10-03T00:00:01Z".into(),
            activity: Some(CommandActivity::Verification),
        };
        let invocation = |status, outcome| ThreadItem {
            id: "call".into(),
            context: Some(Box::new(context.clone())),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "unified_exec".into(),
                arguments: Some(serde_json::json!({"cmd": "cargo check"})),
                tool_call_id: None,
                status,
                outcome,
            })),
        };
        let output = |status, exit_code| ThreadItem {
            id: "output".into(),
            context: Some(Box::new(context.clone())),
            details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "call".into(),
                tool_call_id: None,
                spool_path: None,
                output: String::new(),
                status,
                exit_code,
            })),
        };
        log.append(&ThreadEvent::ItemStarted(ItemStartedEvent { item: invocation(ToolCallStatus::InProgress, None) }))
            .unwrap();
        if mutation_before_execution {
            edit(&log, "a", "edit", "a.rs", PatchApplyStatus::Completed);
        }
        log.append(&ThreadEvent::ItemStarted(ItemStartedEvent { item: output(ToolCallStatus::InProgress, None) }))
            .unwrap();
        if !mutation_before_execution {
            edit(&log, "a", "edit", "a.rs", PatchApplyStatus::Completed);
        }
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: output(ToolCallStatus::Completed, Some(0)),
        }))
        .unwrap();
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: invocation(ToolCallStatus::Completed, Some(ToolOutcome::Success)),
        }))
        .unwrap();
        let model = query_explanation(&log, ExplanationScope::Task).unwrap();
        assert_eq!(model.verification.len(), 1);
        assert_eq!(model.verification[0].fact.status, "passed");
        assert_eq!(model.verification[0].fresh, mutation_before_execution);
    }
}
