use super::*;

#[test]
fn follow_up_prompts_force_conclusion_after_stall() {
    let mut stats = SessionStats::default();
    stats.mark_turn_stalled(true, Some("turn blocked".to_string()));

    let action = stats.register_follow_up_prompt("continue");
    assert_eq!(action, FollowUpPromptAction::RecoverFromStall { stall_reason: Some("turn blocked".to_string()) });
    assert!(stats.turn_stalled());
    assert_eq!(stats.turn_stall_reason(), Some("turn blocked"));
}
#[test]
fn verification_pending_survives_stall_recovery_until_explicitly_cleared() {
    let mut stats = SessionStats::default();
    stats.set_verification_snapshot((true, 0));
    stats.mark_turn_stalled(true, Some("verification pending".to_string()));
    stats.mark_turn_stalled(false, None);

    assert!(stats.verification_snapshot().0);
    stats.set_verification_snapshot((false, 0));
    assert!(!stats.verification_snapshot().0);
}
#[test]
fn tool_loop_grant_preauthorized_latches_only_after_explicit_grant() {
    let mut stats = SessionStats::default();
    assert!(!stats.tool_loop_grant_preauthorized(), "session starts unlatched");

    stats.mark_tool_loop_grant_preauthorized();
    assert!(stats.tool_loop_grant_preauthorized());

    // Idempotent: later grants do not flip it back.
    stats.mark_tool_loop_grant_preauthorized();
    assert!(stats.tool_loop_grant_preauthorized());
}
#[test]
fn tool_loop_grant_preauthorized_survives_fresh_execution_in_session() {
    let mut stats = SessionStats::default();
    stats.mark_tool_loop_grant_preauthorized();
    stats.reset_for_fresh_execution();
    assert!(
        stats.tool_loop_grant_preauthorized(),
        "the one-time HITL latch is process-session scoped, not conversation-context scoped"
    );
}
#[test]
fn preflight_fresh_execution_clears_validation_without_restoring_continuations() {
    let mut stats = SessionStats::default();
    stats.record_tracker_continuation_turn_with_limit(3);
    stats.record_plan_continuation_turn_with_limit(3);
    stats
        .preflight_recovery
        .reject("task_tracker", ValidationCategory::Arguments, None, 6, false);
    stats.preflight_recovery.synthesis();
    stats.preflight_recovery.begin_corrective_turn();
    stats.preflight_recovery.finish_corrective_turn();
    assert!(stats.preflight_recovery.exhausted());

    stats.reset_for_fresh_execution();

    assert!(stats.preflight_recovery.is_clear());
    assert!(!stats.preflight_recovery.exhausted());
    assert_eq!(stats.tracker_continuation_turns(), 1);
    assert_eq!(stats.plan_continuation_turns(), 1);
}
#[test]
fn verification_auto_recovery_turns_are_bounded_and_reset_on_new_request() {
    use crate::agent::runloop::unified::turn::tool_outcomes::helpers::MAX_VERIFICATION_AUTO_RECOVERY_TURNS;

    let mut stats = SessionStats::default();
    for _ in 0..MAX_VERIFICATION_AUTO_RECOVERY_TURNS {
        assert!(stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
    }
    assert!(!stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
    assert_eq!(stats.verification_auto_recovery_turns(), MAX_VERIFICATION_AUTO_RECOVERY_TURNS);

    stats.reset_verification_recovery_episode();
    assert_eq!(stats.verification_auto_recovery_turns(), MAX_VERIFICATION_AUTO_RECOVERY_TURNS);
    assert!(!stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));

    // A fresh non-follow-up user request resets the cross-turn budget.
    assert_eq!(stats.register_follow_up_prompt("run tests and summarize"), FollowUpPromptAction::None);
    assert_eq!(stats.verification_auto_recovery_turns(), 0);
}
#[test]
fn verification_auto_failure_escalation_counts_and_resets_as_episode() {
    let mut stats = SessionStats::default();
    assert_eq!(stats.verification_consecutive_failures(), 0);
    assert!(stats.last_verification_failure().is_none());

    let first = stats.record_verification_auto_failure("cargo check --locked".to_string(), "error[A]: boom");
    assert_eq!(first, 1);
    let second = stats.record_verification_auto_failure("cargo check --locked".to_string(), "error[A]: boom again");
    assert_eq!(second, 2);
    let summary = stats.last_verification_failure().expect("failure tail must be kept");
    assert_eq!(summary.command, "cargo check --locked");
    assert_eq!(summary.consecutive_failures, 2);
    assert!(summary.excerpt_tail.contains("boom again"));

    // Long output is bounded to the tail at record time.
    let long = format!("x{} tail-marker", "y".repeat(5000));
    stats.record_verification_auto_failure("pytest -q".to_string(), &long);
    let bounded = stats.last_verification_failure().expect("bounded tail must be kept");
    assert!(bounded.excerpt_tail.chars().count() <= 2000);
    assert!(bounded.excerpt_tail.contains("tail-marker"));
    assert!(!bounded.excerpt_tail.starts_with('x'), "head must be elided, not kept");

    stats.record_verification_auto_success();
    assert_eq!(stats.verification_consecutive_failures(), 0);
    assert!(stats.last_verification_failure().is_none());

    stats.record_verification_auto_failure("cargo check --locked".to_string(), "boom");
    stats.reset_verification_recovery_episode();
    assert_eq!(stats.verification_consecutive_failures(), 0);
    assert_eq!(stats.verification_auto_recovery_turns(), 0);
    assert!(stats.last_verification_failure().is_none());
}
#[test]
fn verification_recovery_is_not_replenished_by_internal_progress() {
    let mut stats = SessionStats::default();
    assert!(stats.record_verification_auto_recovery_turn_with_limit(1));
    stats.record_verification_auto_failure("cargo check".into(), "{\"exit_code\":1}");
    stats.reset_verification_recovery_episode();
    stats.register_follow_up_prompt(
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::TRACKER_CONTINUE_FOLLOW_UP_PREFIX,
    );
    assert_eq!(stats.verification_auto_recovery_turns(), 1);
    assert!(!stats.record_verification_auto_recovery_turn_with_limit(1));
    stats.register_follow_up_prompt("Please implement another request");
    assert_eq!(stats.verification_auto_recovery_turns(), 0);
    assert!(stats.record_verification_auto_recovery_turn_with_limit(1));
}
#[test]
fn missing_executable_hint_names_the_binary_across_toolchains() {
    let python_traceback = "Traceback (most recent call last):\n  File \"scripts/check_markdown.py\", line 74, in <module>\nFileNotFoundError: [Errno 2] No such file or directory: 'npx'\n";
    let cases = [
        (python_traceback, "npx"),
        ("npx: command not found", "npx"),
        ("/bin/sh: npx: command not found", "npx"),
        ("sh: 1: npx: not found", "npx"),
        ("Error: spawn npx ENOENT", "npx"),
        ("exec: \"bun\": executable file not found in $PATH", "bun"),
        ("error: cannot run the Markdown linter: 'node' was not found on PATH.", "node"),
    ];
    for (output, binary) in cases {
        let hint = missing_executable_hint(output).unwrap_or_else(|| panic!("must detect: {output}"));
        assert!(hint.contains(&format!("`{binary}`")), "hint names {binary}: {hint}");
        assert!(hint.contains("environment problem"), "hint reframes as environmental: {hint}");
        assert!(hint.contains("verify"), "hint points at tracker metadata: {hint}");
    }
}
#[test]
fn missing_executable_hint_detects_json_escaped_tool_results() {
    let response = r#"{"exit_code":1,"output":"Traceback (most recent call last):\nFileNotFoundError: [Errno 2] No such file or directory: 'npx'\n"}"#;
    let hint = missing_executable_hint(response).expect("JSON-escaped output must still match");
    assert!(hint.contains("`npx`"), "hint names the binary: {hint}");
}
#[test]
fn missing_executable_hint_falls_back_to_generic_guidance_without_a_name() {
    for output in ["FileNotFoundError", "command not found", "Error: spawn ENOENT"] {
        let hint = missing_executable_hint(output).unwrap_or_else(|| panic!("must detect: {output}"));
        assert!(hint.contains("A required executable"), "generic subject: {hint}");
        assert!(!hint.contains("``"), "no empty code span: {hint}");
    }
}
#[test]
fn missing_executable_hint_ignores_genuine_check_failures() {
    for output in [
        "cat: missing.txt: No such file or directory",
        "README.md:1 error MD041/first-line-heading",
        "Error: ENOENT: no such file or directory, open '/tmp/x'",
        "read-only file system",
        "sh: 1: Syntax error: \"(\" unexpected",
        "error: foo: not found",
    ] {
        assert!(missing_executable_hint(output).is_none(), "must not match: {output}");
    }
}
