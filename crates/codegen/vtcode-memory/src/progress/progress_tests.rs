use super::*;

fn sample_ledger() -> ProgressLedger {
    let mut l = ProgressLedger::new("s1", "ship the feature");
    l.set_milestones(vec![
        Milestone {
            id: "1".into(),
            description: "design".into(),
            status: MilestoneStatus::Done,
        },
        Milestone {
            id: "2".into(),
            description: "implement".into(),
            status: MilestoneStatus::InProgress,
        },
        Milestone {
            id: "3".into(),
            description: "verify".into(),
            status: MilestoneStatus::Pending,
        },
    ]);
    l
}

#[test]
fn completion_ratio_reflects_terminal_milestones() {
    let l = sample_ledger();
    assert!((l.completion_ratio() - 1.0 / 3.0).abs() < f32::EPSILON);
    assert!(!l.is_complete());
}

#[test]
fn empty_ledger_is_complete() {
    let l = ProgressLedger::new("s", "goal");
    assert!(l.is_complete());
    assert!((l.completion_ratio() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn advance_clears_stall_and_bumps_confidence() {
    let mut l = sample_ledger();
    l.note_stall();
    assert!(l.is_stalled());
    let before = l.confidence;
    l.note_advance();
    assert!(!l.is_stalled());
    assert!(l.confidence >= before);
}

#[test]
fn persistence_round_trips() {
    let tmp = std::env::temp_dir().join(format!("vtcode-prog-{}", std::process::id()));
    let ws = tmp.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let mut l = sample_ledger();
    l.note_stall();
    save_progress(&ws, "s1", &l).unwrap();
    let loaded = load_progress(&ws, "s1").unwrap().expect("ledger present");
    assert_eq!(loaded, l);
    assert!(loaded.is_stalled());
    assert!(load_progress(&ws, "absent").unwrap().is_none());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn handoff_metadata_defaults_to_none() {
    let l = ProgressLedger::new("s1", "goal");
    assert!(l.previous_session_id.is_none());
    assert!(l.handoff_summary.is_none());
    assert!(l.known_issues.is_empty());
    assert!(l.git_checkpoint.is_none());
}

#[test]
fn set_handoff_records_metadata() {
    let mut l = ProgressLedger::new("s2", "goal");
    l.set_handoff("s1", "implemented login", Some("abc123".to_string()));
    assert_eq!(l.previous_session_id.as_deref(), Some("s1"));
    assert_eq!(l.handoff_summary.as_deref(), Some("implemented login"));
    assert_eq!(l.git_checkpoint.as_deref(), Some("abc123"));
}

#[test]
fn add_known_issue_accumulates() {
    let mut l = ProgressLedger::new("s3", "goal");
    l.add_known_issue("rate limiting missing");
    l.add_known_issue("no error handling for timeouts");
    assert_eq!(l.known_issues.len(), 2);
    assert_eq!(l.known_issues[0], "rate limiting missing");
}

#[test]
fn handoff_metadata_survives_persistence() {
    let tmp = std::env::temp_dir().join(format!("vtcode-prog-{}", std::process::id()));
    let ws = tmp.join("ws");
    std::fs::create_dir_all(&ws).unwrap();

    let mut l = sample_ledger();
    l.set_handoff("prev-session", "built auth", Some("def456".to_string()));
    l.add_known_issue("tests are flaky");

    save_progress(&ws, "s4", &l).unwrap();
    let loaded = load_progress(&ws, "s4").unwrap().expect("present");
    assert_eq!(loaded.previous_session_id.as_deref(), Some("prev-session"));
    assert_eq!(loaded.handoff_summary.as_deref(), Some("built auth"));
    assert_eq!(loaded.git_checkpoint.as_deref(), Some("def456"));
    assert_eq!(loaded.known_issues, vec!["tests are flaky"]);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn to_markdown_includes_handoff_metadata() {
    let mut l = ProgressLedger::new("s5", "build feature");
    l.set_handoff("s4", "implemented core", Some("abc123".to_string()));
    l.add_known_issue("missing error handling");

    let md = l.to_markdown();
    assert!(md.contains("Handed off from:** s4"));
    assert!(md.contains("Handoff summary:** implemented core"));
    assert!(md.contains("Git checkpoint:** `abc123`"));
    assert!(md.contains("- missing error handling"));
}

#[test]
fn to_markdown_omits_handoff_when_absent() {
    let l = ProgressLedger::new("s6", "goal");
    let md = l.to_markdown();
    assert!(!md.contains("Handed off from"));
    assert!(!md.contains("Handoff summary"));
    assert!(!md.contains("Git checkpoint"));
    assert!(!md.contains("Known Issues"));
}
