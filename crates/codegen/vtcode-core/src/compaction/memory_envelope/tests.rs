use super::extract_compaction_summary;
use super::{
    SessionMemoryEnvelope, TaskTrackerSnapshot, build_session_memory_envelope, parse_task_tracker_snapshot,
    resolve_compaction_threshold_with_reserve,
};
use crate::llm::provider::Message;
use std::path::Path;

#[test]
fn default_compaction_trigger_uses_ratio_of_prompt_budget() {
    // 100_000 context, 4_096 reserve → prompt budget 95_904.
    // Default trigger is 75% of the prompt budget, not the full budget.
    let threshold = resolve_compaction_threshold_with_reserve(None, 100_000, 4_096).expect("threshold");
    let prompt_budget = 100_000u64 - 4_096;
    let expected = prompt_budget * 75 / 100;
    assert_eq!(threshold, expected);
    // Keep the integer percent in sync with the shared ratio constant.
    assert!((vtcode_config::constants::context::DEFAULT_COMPACTION_TRIGGER_RATIO - 0.75).abs() < f64::EPSILON);
    assert!(threshold < prompt_budget, "default trigger must fire before the full prompt budget");
    // Explicit config still wins and is capped at the prompt budget.
    assert_eq!(resolve_compaction_threshold_with_reserve(Some(10_000), 100_000, 4_096), Some(10_000));
    assert_eq!(resolve_compaction_threshold_with_reserve(Some(200_000), 100_000, 4_096), Some(prompt_budget));
}

fn tracker_snapshot(summary: &str, objective: &str, todo: &[&str]) -> TaskTrackerSnapshot {
    let markdown = format!("# {objective}\n\n- [ ] {}\n", todo.join("\n- [ ] "));
    let mut snap = parse_task_tracker_snapshot(&markdown);
    snap.summary = Some(summary.to_string());
    snap.objective = Some(objective.to_string());
    snap.verification_todo = todo.iter().map(|s| (*s).to_string()).collect();
    snap
}

#[test]
fn envelope_prefers_live_objective_over_stale_prior() {
    let prior = SessionMemoryEnvelope {
        session_id: "sess-a".to_string(),
        objective: Some("1789-mighty-harbor".to_string()),
        task_summary: Some("mighty-harbor: README checklist".to_string()),
        verification_todo: vec!["mighty todo".to_string()],
        ..Default::default()
    };
    let live = tracker_snapshot("bright-squid analyze fix", "1789-bright-squid", &["cargo check analyze"]);

    let envelope = build_session_memory_envelope(
        "sess-b",
        Path::new("."),
        &[],
        &[],
        "Working on bright-squid".to_string(),
        None,
        Some(&prior),
        &live,
        None,
    );

    assert_eq!(envelope.objective.as_deref(), Some("1789-bright-squid"));
    assert!(
        envelope.task_summary.as_deref().unwrap_or_default().contains("bright-squid"),
        "task_summary must not keep prior-task narrative: {:?}",
        envelope.task_summary
    );
    assert_eq!(envelope.verification_todo, vec!["cargo check analyze".to_string()]);
}

#[test]
fn envelope_objective_change_drops_prior_task_summary_when_live_empty() {
    let prior = SessionMemoryEnvelope {
        session_id: "sess-a".to_string(),
        objective: Some("task-a".to_string()),
        task_summary: Some("task-a: stale checklist".to_string()),
        verification_todo: vec!["stale".to_string()],
        ..Default::default()
    };
    let live = TaskTrackerSnapshot {
        objective: Some("task-b".to_string()),
        summary: None,
        ..Default::default()
    };

    let envelope = build_session_memory_envelope(
        "sess-b",
        Path::new("."),
        &[],
        &[],
        "summary".to_string(),
        None,
        Some(&prior),
        &live,
        None,
    );

    assert_eq!(envelope.objective.as_deref(), Some("task-b"));
    assert_eq!(envelope.task_summary, None);
    assert!(envelope.verification_todo.is_empty());
}

#[test]
fn envelope_same_objective_keeps_todo_merge() {
    let prior = SessionMemoryEnvelope {
        session_id: "sess-a".to_string(),
        objective: Some("task-1".to_string()),
        task_summary: Some("task-1: shared".to_string()),
        verification_todo: vec!["keep me".to_string()],
        ..Default::default()
    };
    let live = tracker_snapshot("task-1 continued", "task-1", &["new item"]);

    let envelope = build_session_memory_envelope(
        "sess-a",
        Path::new("."),
        &[],
        &[],
        "summary".to_string(),
        None,
        Some(&prior),
        &live,
        None,
    );

    assert_eq!(envelope.objective.as_deref(), Some("task-1"));
    assert!(envelope.verification_todo.contains(&"keep me".to_string()));
    assert!(envelope.verification_todo.contains(&"new item".to_string()));
}

#[test]
fn envelope_stale_artifact_resets_prior_constraints() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let tasks_dir = workspace.path().join(".vtcode").join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
    std::fs::create_dir_all(workspace.path().join(".vtcode").join("sessions").join("sess-stale"))
        .expect("session dir anchors the artifact cutoff");
    let spec_path = tasks_dir.join("current_spec.md");
    std::fs::write(&spec_path, "# Spec\n- Do not touch prod config\n").expect("write stale spec");
    let stale = std::time::SystemTime::now() - std::time::Duration::from_secs(48 * 60 * 60);
    let file = std::fs::File::options().write(true).open(&spec_path).expect("open");
    file.set_modified(stale).expect("set mtime");

    let prior = SessionMemoryEnvelope {
        session_id: "sess-stale".to_string(),
        constraints: vec!["Do not touch prod config".to_string(), "Keep user budget".to_string()],
        ..Default::default()
    };

    let envelope = build_session_memory_envelope(
        "sess-stale",
        workspace.path(),
        &[],
        &[],
        "summary".to_string(),
        None,
        Some(&prior),
        &tracker_snapshot("continuing", "sess-stale", &["cargo check"]),
        None,
    );

    assert!(envelope.spec_summary.is_none(), "stale spec must not describe the session");
    assert!(
        envelope.constraints.is_empty(),
        "a stale artifact resets the constraints channel so its lines cannot re-enter: {:?}",
        envelope.constraints
    );
}

#[test]
fn envelope_fresh_artifact_merges_prior_constraints() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let tasks_dir = workspace.path().join(".vtcode").join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
    std::fs::create_dir_all(workspace.path().join(".vtcode").join("sessions").join("sess-fresh"))
        .expect("session dir anchors the artifact cutoff");
    std::fs::write(tasks_dir.join("current_spec.md"), "# Spec\n- Keep coverage at 70%\n").expect("write fresh spec");

    let prior = SessionMemoryEnvelope {
        session_id: "sess-fresh".to_string(),
        constraints: vec!["Do not redesign the harness".to_string()],
        ..Default::default()
    };

    let envelope = build_session_memory_envelope(
        "sess-fresh",
        workspace.path(),
        &[],
        &[],
        "summary".to_string(),
        None,
        Some(&prior),
        &tracker_snapshot("continuing", "sess-fresh", &["cargo check"]),
        None,
    );

    assert!(
        envelope.constraints.contains(&"Do not redesign the harness".to_string()),
        "fresh artifacts keep the continuity merge of inherited constraints: {:?}",
        envelope.constraints
    );
    assert!(
        envelope.constraints.contains(&"Keep coverage at 70%".to_string()),
        "fresh artifact constraints are extracted: {:?}",
        envelope.constraints
    );
}

#[test]
fn local_summary_takes_precedence_over_retained_provider_detail() {
    let compacted = vec![
        Message::system("Previous conversation summary:\nnew local summary".to_string()),
        Message::user("latest request".to_string()),
        Message::assistant(String::new()).with_reasoning_details(Some(vec![serde_json::json!({
            "type": "compaction",
            "content": "stale provider summary",
        })])),
    ];

    assert_eq!(extract_compaction_summary(&compacted, &[]), "new local summary");
}

#[test]
fn split_verify_command_tail_splits_and_strips_brackets() {
    let tail = " [cargo nextest run -E 'test(turn_loop_helpers) or test(tool_outcomes) or test(blocked_handoff)'] and verify: [cargo check --locked";
    let commands = super::split_verify_command_tail(tail);
    assert_eq!(
        commands,
        vec![
            "cargo nextest run -E 'test(turn_loop_helpers) or test(tool_outcomes) or test(blocked_handoff)'"
                .to_string(),
            "cargo check --locked".to_string(),
        ]
    );
}

#[test]
fn split_verify_command_tail_keeps_legitimate_inner_brackets() {
    // Only one outer pair is stripped; array/subscript syntax inside the
    // command must survive.
    let commands = super::split_verify_command_tail(" cargo test --features 'a[b]' ");
    assert_eq!(commands, vec!["cargo test --features 'a[b]'".to_string()]);
}

#[test]
fn structured_verify_commands_do_not_splice_bracketed_pairs() {
    let content =
        "# Fix\n\n- [x] step\n  files: src/x.rs\n  verify: [cargo check --locked] and verify: [cargo fmt --check\n";
    let commands = super::collect_structured_verify_commands(content);
    assert_eq!(commands, vec!["cargo check --locked".to_string(), "cargo fmt --check".to_string()]);
    let summary = super::extract_verification_summary(content, &[]).expect("summary");
    assert!(!summary.contains("] and verify:"), "splice marker must not leak: {summary}");
}
