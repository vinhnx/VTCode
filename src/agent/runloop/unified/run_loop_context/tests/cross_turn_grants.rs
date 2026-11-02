use super::*;

// --- CrossTurnTracker tests ---

#[test]
fn cross_turn_tracker_no_warning_on_first_turn() {
    let mut tracker = CrossTurnTracker::new();
    let read_sigs = vec!["apply_patch::read::src/main.rs".to_string()];
    let written = HashSet::new();
    assert!(tracker.seal_turn(&read_sigs, &written, None, false).is_none());
}
#[test]
fn cross_turn_tracker_detects_repeated_turn() {
    let mut tracker = CrossTurnTracker::new();
    let read_sigs = vec!["apply_patch::read::src/main.rs".to_string()];
    let written = HashSet::new();

    // First turn: no warning
    assert!(tracker.seal_turn(&read_sigs, &written, None, false).is_none());

    // Second turn with same signatures: cross-turn loop detected
    let warning = tracker.seal_turn(&read_sigs, &written, None, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("Cross-turn loop detected"));
}
#[test]
fn cross_turn_tracker_no_false_positive_different_turns() {
    let mut tracker = CrossTurnTracker::new();
    let read_sigs_1 = vec!["apply_patch::read::src/main.rs".to_string()];
    let read_sigs_2 = vec!["apply_patch::read::src/lib.rs".to_string()];
    let written = HashSet::new();

    assert!(tracker.seal_turn(&read_sigs_1, &written, None, false).is_none());
    assert!(tracker.seal_turn(&read_sigs_2, &written, None, false).is_none());
}
#[test]
fn cross_turn_tracker_stuck_no_progress() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    // Use different signatures each turn to avoid cross-turn loop detection
    // and isolate the stuck (zero-mutation) detection.
    let sigs_1 = vec!["apply_patch::read::src/a.rs".to_string()];
    let sigs_2 = vec!["apply_patch::read::src/b.rs".to_string()];
    let sigs_3 = vec!["apply_patch::read::src/c.rs".to_string()];

    assert!(tracker.seal_turn(&sigs_1, &written, None, false).is_none());
    assert!(tracker.seal_turn(&sigs_2, &written, None, false).is_none());

    // Third consecutive read-only turn: stuck warning
    let warning = tracker.seal_turn(&sigs_3, &written, None, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("No progress detected"));
}
#[test]
fn cross_turn_tracker_mutation_resets_stuck_counter() {
    let mut tracker = CrossTurnTracker::new();
    let empty_written = HashSet::new();

    // Two read-only turns with different signatures (avoid cross-turn loop)
    let sigs_a = vec!["apply_patch::read::src/a.rs".to_string()];
    let sigs_b = vec!["apply_patch::read::src/b.rs".to_string()];
    assert!(tracker.seal_turn(&sigs_a, &empty_written, None, false).is_none());
    assert!(tracker.seal_turn(&sigs_b, &empty_written, None, false).is_none());

    // A mutating turn resets the counter
    let mut written = HashSet::new();
    written.insert("src/main.rs".to_string());
    let sigs_c = vec!["apply_patch::read::src/c.rs".to_string()];
    assert!(tracker.seal_turn(&sigs_c, &written, None, false).is_none());

    // Two more read-only turns: no stuck warning (counter was reset)
    let sigs_d = vec!["apply_patch::read::src/d.rs".to_string()];
    let sigs_e = vec!["apply_patch::read::src/e.rs".to_string()];
    assert!(tracker.seal_turn(&sigs_d, &empty_written, None, false).is_none());
    assert!(tracker.seal_turn(&sigs_e, &empty_written, None, false).is_none());
}
#[test]
fn cross_turn_tracker_command_execution_resets_stuck_counter() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    assert!(tracker.seal_turn(&["read::a".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&["read::b".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&[], &written, Some("exec::cargo-check"), false).is_none());
    assert_eq!(tracker.zero_mutation_turns(), 0);

    assert!(tracker.seal_turn(&["read::c".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&["read::d".to_string()], &written, None, false).is_none());
}
#[test]
fn cross_turn_tracker_out_of_band_progress_resets_stuck_counter() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    assert!(tracker.seal_turn(&["read::a".to_string()], &written, None, false).is_none());
    assert!(tracker.seal_turn(&["read::b".to_string()], &written, None, false).is_none());
    assert_eq!(tracker.zero_mutation_turns(), 2);

    assert!(
        tracker
            .seal_turn_with_progress(&[], &written, None, None, true, false)
            .is_none()
    );
    assert_eq!(tracker.zero_mutation_turns(), 0);

    assert!(tracker.seal_turn(&["read::c".to_string()], &written, None, false).is_none());
    assert_eq!(tracker.zero_mutation_turns(), 1);
}
#[test]
fn harness_state_separates_guarded_and_admitted_shell_signatures() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);

    state.record_shell_command_run("exec_command::blocked".to_string());
    assert_eq!(state.last_admitted_shell_command_signature, None);

    state.record_admitted_shell_command("exec_command::admitted".to_string());
    assert_eq!(state.last_admitted_shell_command_signature.as_deref(), Some("exec_command::admitted"));
}
#[test]
fn harness_state_records_failed_shell_key() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".to_string()), TurnId("turn-1".to_string()), 2, 10, 1);
    assert_eq!(state.last_failed_shell_key(), None);

    state.record_failed_shell_command("command_session::cargo check".to_string(), "error: bad manifest".to_string());
    assert_eq!(state.last_failed_shell_key(), Some("command_session::cargo check::err::error: bad manifest"));
}
#[test]
fn cross_turn_tracker_empty_turn_no_warning() {
    let mut tracker = CrossTurnTracker::new();
    let empty_sigs: Vec<String> = Vec::new();
    let empty_written = HashSet::new();

    // Empty turns should not trigger warnings or corrupt state
    assert!(tracker.seal_turn(&empty_sigs, &empty_written, None, false).is_none());
    assert!(tracker.seal_turn(&empty_sigs, &empty_written, None, false).is_none());
}
#[test]
fn cross_turn_tracker_identical_shell_failure_warns_on_third_turn() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();
    let key = Some("command_session::cargo check::err::error: failed to load manifest");

    // Vary the read sets so the fingerprint loop detector stays quiet;
    // the identical-failure streak must fire regardless.
    let sigs_a = vec!["code_search::alpha".to_string()];
    let sigs_b = vec!["code_search::beta".to_string()];
    let sigs_c = vec!["code_search::gamma".to_string()];
    assert!(
        tracker
            .seal_turn_with_progress(&sigs_a, &written, None, key, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&sigs_b, &written, None, key, false, false)
            .is_none()
    );
    let warning = tracker.seal_turn_with_progress(&sigs_c, &written, None, key, false, false);
    assert!(warning.is_some());
    let warning = warning.unwrap();
    assert!(warning.contains("Identical shell failure"), "warning names the pattern: {warning}");
    assert!(warning.contains("cargo check"), "warning names the command: {warning}");
}
#[test]
fn cross_turn_tracker_failed_shell_streak_resets_on_change_or_success() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();
    let empty: Vec<String> = Vec::new();
    let key_a = Some("command_session::cargo check::err::error: bad manifest");
    let key_b = Some("command_session::cargo check::err::error: missing crate");

    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_a, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_a, false, false)
            .is_none()
    );
    // Changed error text restarts the streak: no warning on what would
    // otherwise be the third consecutive failure turn.
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_b, false, false)
            .is_none()
    );
    // A clean turn clears the streak entirely.
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, None, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_b, false, false)
            .is_none()
    );
    assert!(
        tracker
            .seal_turn_with_progress(&empty, &written, None, key_b, false, false)
            .is_none()
    );
    let warning = tracker.seal_turn_with_progress(&empty, &written, None, key_b, false, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("Identical shell failure"));
}
#[test]
fn cross_turn_tracker_order_independent_fingerprint() {
    let mut tracker = CrossTurnTracker::new();
    let written = HashSet::new();

    // Same signatures in different order should produce same fingerprint
    let sigs_a = vec![
        "apply_patch::read::src/main.rs".to_string(),
        "code_search::grep::fn".to_string(),
    ];
    let sigs_b = vec![
        "code_search::grep::fn".to_string(),
        "apply_patch::read::src/main.rs".to_string(),
    ];

    assert!(tracker.seal_turn(&sigs_a, &written, None, false).is_none());
    let warning = tracker.seal_turn(&sigs_b, &written, None, false);
    assert!(warning.is_some());
    assert!(warning.unwrap().contains("Cross-turn loop detected"));
}
#[test]
fn full_auto_loop_grants_require_runtime_and_config_and_opt_in() {
    assert_eq!(SESSION_LIMIT_AUTO_GRANT_INCREMENT, 100);

    // No config at all: interactive sessions keep prompting.
    assert!(!full_auto_loop_grants_enabled(true, None));
    assert!(!full_auto_loop_grants_enabled(false, None));

    let mut cfg = VTCodeConfig::default();
    // Default config has full-auto disabled: no auto-grant either way.
    assert!(!full_auto_loop_grants_enabled(true, Some(&cfg)));
    assert!(!full_auto_loop_grants_enabled(false, Some(&cfg)));

    // Full-auto runtime + enabled config grants by default (opt-out flag on).
    cfg.automation.full_auto.enabled = true;
    assert!(full_auto_loop_grants_enabled(true, Some(&cfg)));
    // Ordinary runtime never grants, even with full-auto configured.
    assert!(!full_auto_loop_grants_enabled(false, Some(&cfg)));

    // Explicit opt-out restores prompting in full-auto runs.
    cfg.automation.full_auto.auto_grant_tool_limits = false;
    assert!(!full_auto_loop_grants_enabled(true, Some(&cfg)));
}
#[test]
fn failure_diagnosis_memo_round_trips_and_caps_model_calls() {
    let mut state = HarnessTurnState::new(TurnRunId("r".into()), TurnId("t".into()), 4, 60, 1);
    let key = DiagnosisMemoKey {
        tool: CompactStr::from("exec_command"),
        evidence: "exit 1".to_string(),
    };
    assert!(state.failure_diagnosis_memo_get(&key).is_none());
    state.failure_diagnosis_memo_put(
        key.clone(),
        DiagnosisMemoEntry {
            observed: CompactStr::from("obs"),
            likely_cause: CompactStr::from("cause"),
            next_action: CompactStr::from("act"),
        },
    );
    let hit = state.failure_diagnosis_memo_get(&key).expect("memo hit");
    assert_eq!(hit.observed.as_str(), "obs");
    assert_eq!(hit.likely_cause.as_str(), "cause");
    assert_eq!(hit.next_action.as_str(), "act");

    assert!(state.can_spend_failure_diagnosis_model_call());
    for _ in 0..3 {
        state.record_failure_diagnosis_model_call();
    }
    assert!(!state.can_spend_failure_diagnosis_model_call(), "per-turn model diagnosis budget must stop at 3");
}
#[test]
fn auto_permission_probe_budget_stops_model_calls_per_turn() {
    let mut state = HarnessTurnState::new(TurnRunId("run-1".into()), TurnId("turn-1".into()), 120, 10, 1);

    assert!(state.can_spend_auto_permission_probe_model_call());
    for _ in 0..3 {
        state.record_auto_permission_probe_model_call();
    }
    assert!(
        !state.can_spend_auto_permission_probe_model_call(),
        "per-turn prompt-injection probe budget must stop at 3"
    );
}
