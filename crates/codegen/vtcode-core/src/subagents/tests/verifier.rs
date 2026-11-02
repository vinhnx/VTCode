use super::*;

#[test]
fn parse_verifier_decision_reads_explicit_decision_line() {
    assert_eq!(parse_verifier_decision("Looks fine.\nDecision: APPROVED"), Some(true));
    assert_eq!(parse_verifier_decision("**Decision:** REJECT\n"), Some(false));
    assert_eq!(parse_verifier_decision("- Decision: `approve`"), Some(true));
    assert_eq!(
        parse_verifier_decision("Decision: APPROVED\nno unsafe code was blocked"),
        Some(true),
        "prose keywords must not override the decision line"
    );
    assert_eq!(parse_verifier_decision("approved, no issues found"), None);
    assert_eq!(parse_verifier_decision("Decision: APPROVED."), Some(true));
}
#[test]
fn parse_verifier_decision_fails_closed_on_unclear_or_negated_value() {
    for summary in [
        "Decision: NOT APPROVED",
        "Decision: not approve",
        "**Decision:** Not Approved",
        "Decision: approved? no",
        "Decision: can't approve",
        "Decision: pending",
        "Decision:",
        "Decision: disapproved",
    ] {
        assert_eq!(parse_verifier_decision(summary), Some(false), "{summary:?}");
    }
    assert_eq!(
        parse_verifier_decision("Decision: APPROVED\nDecision: NOT APPROVED"),
        Some(false),
        "the last decision line wins"
    );
}
#[test]
fn heuristic_verifier_approval_rejects_negated_approval() {
    assert!(!heuristic_verifier_approval("The change is not approved.", &[]));
    assert!(!heuristic_verifier_approval("I do not approve this change", &[]));
    assert!(!heuristic_verifier_approval("Approved.", &["ISSUE: a.rs:1 bug".to_string()]));
    assert!(!heuristic_verifier_approval("Unclear; could not inspect the files.", &[]));
    assert!(heuristic_verifier_approval("Approved, no issues found.", &[]));
}
#[test]
fn extract_issues_reads_only_structured_issue_lines() {
    let summary = "- ISSUE: src/lib.rs:3 missing bounds check\n\
                   1. issue: src/a.rs:9 wrong default\n\
                   * **ISSUE:** src/b.rs:1 stale doc\n\
                   ISSUE: src/c.rs:2 unhandled error\n\
                   Reasoning: no error: all tests pass\n\
                   error: expected `;`, found `}` (quoted compiler output)\n\
                   - the problem: none\n\
                   Verdict mentions REJECT: only in prose\n\
                   - ISSUE:\n\
                   Decision: APPROVED";
    assert_eq!(
        extract_issues_from_summary(summary),
        vec![
            "ISSUE: src/lib.rs:3 missing bounds check".to_string(),
            "ISSUE: src/a.rs:9 wrong default".to_string(),
            "ISSUE: src/b.rs:1 stale doc".to_string(),
            "ISSUE: src/c.rs:2 unhandled error".to_string(),
        ]
    );
    assert!(extract_issues_from_summary("Reasoning: no error: all tests pass\nDecision: APPROVED").is_empty());
}
