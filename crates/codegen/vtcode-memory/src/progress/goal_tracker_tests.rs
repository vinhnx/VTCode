use super::*;

fn make_tracker() -> GoalTracker {
    GoalTracker::new(PathBuf::from("/tmp/test-goal-session"))
}

fn activate_tracker(t: &mut GoalTracker) {
    t.create_goal("goal-1".into(), "Build a widget".into(), Some(100_000), 0, "2026-01-01T00:00:00Z".into(), None);
}

#[test]
fn create_goal_activates_and_starts_timer() {
    let mut t = make_tracker();
    activate_tracker(&mut t);

    assert!(t.is_active());
    assert_eq!(t.phase(), Some(GoalPhase::Executing));
    assert_eq!(t.status(), Some(GoalStatus::Active));
    assert_eq!(t.objective(), Some("Build a widget"));
    assert_eq!(t.token_budget(), Some(100_000));
    assert!(t.active_since.is_some());
}

#[test]
fn lifecycle_transitions_record_history_events() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    assert!(
        matches!(t.snapshot().unwrap().history.last().map(|e| &e.event), Some(GoalEvent::GoalCreated)),
        "create_goal must record GoalCreated"
    );

    assert!(t.pause(GoalPauseReason::User));
    {
        let last = t.snapshot().unwrap().history.last().unwrap();
        assert!(matches!(last.event, GoalEvent::GoalPaused));
        assert_eq!(last.detail.as_deref(), Some("user"), "pause records its cause as the history detail");
    }

    assert!(t.resume());
    assert!(matches!(t.snapshot().unwrap().history.last().map(|e| &e.event), Some(GoalEvent::GoalResumed)));

    assert!(t.complete());
    let o = t.snapshot().unwrap();
    assert!(matches!(o.history.last().map(|e| &e.event), Some(GoalEvent::GoalCompleted)));
}

#[test]
fn pause_only_from_active() {
    let mut t = make_tracker();
    activate_tracker(&mut t);

    assert!(t.pause(GoalPauseReason::User));
    assert_eq!(t.status(), Some(GoalStatus::UserPaused));
    assert!(!t.is_active());

    assert!(t.resume());
    assert_eq!(t.status(), Some(GoalStatus::Active));
    assert!(t.is_active());
}

#[test]
fn pause_from_complete_is_noop() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.complete();

    assert!(!t.pause(GoalPauseReason::User));
    assert_eq!(t.status(), Some(GoalStatus::Complete));
}

#[test]
fn resume_only_from_paused_variants() {
    let mut t = make_tracker();
    activate_tracker(&mut t);

    assert!(!t.resume());
    assert_eq!(t.status(), Some(GoalStatus::Active));

    t.budget_limit();
    assert!(!t.resume());
    assert_eq!(t.status(), Some(GoalStatus::BudgetLimited));
}

#[test]
fn complete_from_active_succeeds() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.set_current_subagent(Some("sub-1".into()), Some("worker".into()));

    assert!(t.complete());
    assert_eq!(t.status(), Some(GoalStatus::Complete));
    assert!(t.current_subagent_id().is_none());
}

#[test]
fn complete_from_paused_succeeds() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.pause(GoalPauseReason::User);

    assert!(t.complete());
    assert_eq!(t.status(), Some(GoalStatus::Complete));
}

#[test]
fn complete_from_blocked_succeeds() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.pause(GoalPauseReason::Verification);
    assert!(t.complete());
    assert_eq!(t.status(), Some(GoalStatus::Complete));
}

#[test]
fn budget_limit_from_active_succeeds() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.set_phase(GoalPhase::Executing);

    assert!(t.budget_limit());
    assert_eq!(t.status(), Some(GoalStatus::BudgetLimited));
    assert_eq!(t.phase(), Some(GoalPhase::Idle));
}

#[test]
fn pause_reason_maps_to_correct_status() {
    let mut t = make_tracker();
    activate_tracker(&mut t);

    assert!(t.pause(GoalPauseReason::User));
    assert_eq!(t.status(), Some(GoalStatus::UserPaused));

    t.resume();
    assert!(t.pause(GoalPauseReason::BackOff));
    assert_eq!(t.status(), Some(GoalStatus::BackOffPaused));

    t.resume();
    assert!(t.pause(GoalPauseReason::NoProgress));
    assert_eq!(t.status(), Some(GoalStatus::NoProgressPaused));

    t.resume();
    assert!(t.pause_with_message(GoalPauseReason::Infra, "Turn failed: rate limit".into()));
    assert_eq!(t.status(), Some(GoalStatus::InfraPaused));
}

#[test]
fn is_paused_matches_all_paused_variants() {
    assert!(GoalStatus::UserPaused.is_paused());
    assert!(GoalStatus::BackOffPaused.is_paused());
    assert!(GoalStatus::NoProgressPaused.is_paused());
    assert!(GoalStatus::InfraPaused.is_paused());
    assert!(GoalStatus::Blocked.is_paused());
    assert!(!GoalStatus::Active.is_paused());
    assert!(!GoalStatus::Complete.is_paused());
    assert!(!GoalStatus::BudgetLimited.is_paused());
}

#[test]
fn no_progress_paused_round_trips_distinctly_from_back_off() {
    assert_eq!(GoalStatus::from_wire_str("no_progress_paused"), GoalStatus::NoProgressPaused);
    let json = serde_json::to_string(&GoalStatus::NoProgressPaused).unwrap();
    assert_eq!(json, "\"no_progress_paused\"");
    let back: GoalStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, GoalStatus::NoProgressPaused);
    assert_eq!(GoalStatus::from_wire_str("back_off_paused"), GoalStatus::BackOffPaused);
    assert_ne!(GoalStatus::NoProgressPaused, GoalStatus::BackOffPaused);
}

#[test]
fn resume_from_user_paused() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.pause(GoalPauseReason::User);
    assert!(t.resume());
    assert_eq!(t.status(), Some(GoalStatus::Active));
}

#[test]
fn resume_from_infra_paused() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.pause_with_message(GoalPauseReason::Infra, "Turn failed: auth".into());
    assert_eq!(t.snapshot().and_then(|o| o.pause_message.clone()), Some("Turn failed: auth".into()));
    assert!(t.resume());
    assert_eq!(t.status(), Some(GoalStatus::Active));
    assert!(t.snapshot().unwrap().pause_message.is_none());
}

#[test]
fn pause_with_verification_reason_transitions_to_blocked() {
    let mut t = make_tracker();
    activate_tracker(&mut t);

    assert!(t.pause(GoalPauseReason::Verification));
    assert_eq!(t.status(), Some(GoalStatus::Blocked));
    assert!(t.status().unwrap().is_paused());
}

#[test]
fn resume_from_blocked_transitions_to_active() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.pause(GoalPauseReason::Verification);
    assert!(t.resume());
    assert_eq!(t.status(), Some(GoalStatus::Active));
}

#[test]
fn unknown_future_paused_status_deserializes_to_user_paused() {
    let parsed: GoalStatus = serde_json::from_str(r#""error_paused""#).unwrap();
    assert_eq!(parsed, GoalStatus::UserPaused);
}

#[test]
fn unknown_non_paused_status_deserializes_to_user_paused_not_active() {
    for wire in [r#""quarantined""#, r#""v9_super_active""#, r#""""#] {
        let parsed: GoalStatus = serde_json::from_str(wire).unwrap();
        assert_eq!(parsed, GoalStatus::UserPaused, "wire {wire}");
    }
    assert_eq!(GoalStatus::from_wire_str("not-a-status"), GoalStatus::UserPaused,);
}

#[test]
fn legacy_pascal_case_paused_deserializes_to_user_paused() {
    let legacy = r#""Paused""#;
    let parsed: GoalStatus = serde_json::from_str(legacy).unwrap();
    assert_eq!(parsed, GoalStatus::UserPaused);
}

#[test]
fn legacy_pascal_case_other_variants_deserialize() {
    for (legacy, expected) in [
        (r#""Active""#, GoalStatus::Active),
        (r#""BudgetLimited""#, GoalStatus::BudgetLimited),
        (r#""Complete""#, GoalStatus::Complete),
    ] {
        let parsed: GoalStatus = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed, expected, "legacy {legacy} must parse");
    }
}

#[test]
fn legacy_infra_paused_deserializes() {
    let parsed: GoalStatus = serde_json::from_str(r#""infra_paused""#).unwrap();
    assert_eq!(parsed, GoalStatus::InfraPaused);
}

#[test]
fn goal_event_unknown_string_deserializes_to_unknown() {
    let unknown: GoalEvent = serde_json::from_str("\"some_future_event\"").unwrap();
    assert!(matches!(unknown, GoalEvent::Unknown));
    let known: GoalEvent = serde_json::from_str("\"goal_paused\"").unwrap();
    assert!(matches!(known, GoalEvent::GoalPaused));
}

#[test]
fn record_classifier_stall_trips_on_two_consecutive_identical_fingerprints() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    assert!(!t.record_classifier_stall("fp-a"), "first occurrence of a fingerprint is not a stall");
    assert!(t.record_classifier_stall("fp-a"), "the same fingerprint twice running trips the stall early-exit");
    assert_eq!(t.snapshot().unwrap().classifier_stall_count, 2);
}

#[test]
fn record_classifier_stall_resets_when_fingerprint_changes() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    assert!(!t.record_classifier_stall("fp-a"));
    assert!(t.record_classifier_stall("fp-a"));
    assert!(
        !t.record_classifier_stall("fp-b"),
        "a different fingerprint resets the streak to its first occurrence"
    );
    assert_eq!(t.snapshot().unwrap().classifier_stall_count, 1);
    assert!(t.record_classifier_stall("fp-b"), "the new fingerprint then trips on its own second occurrence");
}

#[test]
fn reset_classifier_stall_clears_streak_so_next_occurrence_is_first() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    assert!(!t.record_classifier_stall("fp-a"));
    assert!(t.record_classifier_stall("fp-a"));
    t.reset_classifier_stall();
    {
        let o = t.snapshot().unwrap();
        assert_eq!(o.classifier_stall_count, 0);
        assert!(o.last_gap_fingerprint.is_none());
    }
    assert!(!t.record_classifier_stall("fp-a"), "after reset, a repeat of the old fingerprint must not re-stall");
}

#[test]
fn full_lifecycle_create_to_complete() {
    let mut t = make_tracker();
    activate_tracker(&mut t);

    t.set_phase(GoalPhase::Executing);
    assert_eq!(t.phase(), Some(GoalPhase::Executing));

    t.set_current_subagent(Some("sub-1".into()), Some("worker".into()));
    assert_eq!(t.current_subagent_id(), Some("sub-1"));

    assert!(t.complete());
    assert_eq!(t.status(), Some(GoalStatus::Complete));
    assert_eq!(t.phase(), Some(GoalPhase::Idle));
    assert!(t.current_subagent_id().is_none());
    assert!(t.active_since.is_none());
}

#[test]
fn serde_round_trip_preserves_data() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    t.set_phase(GoalPhase::Executing);

    let original = t.snapshot().unwrap().clone();
    let json = serde_json::to_string(&original).unwrap();
    let restored: GoalOrchestration = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.goal_id, original.goal_id);
    assert_eq!(restored.objective, original.objective);
    assert_eq!(restored.status, original.status);
    assert_eq!(restored.phase, original.phase);
}

#[test]
fn record_not_achieved_streak_increments_and_returns_new_count() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    assert_eq!(t.record_not_achieved_streak(), 1);
    assert_eq!(t.record_not_achieved_streak(), 2);
    assert_eq!(t.record_not_achieved_streak(), 3);
    assert_eq!(t.snapshot().unwrap().consecutive_not_achieved, 3);
}

#[test]
fn claim_strategist_fire_marks_current_streak() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    let _ = t.record_not_achieved_streak();
    let _ = t.record_not_achieved_streak();
    assert_eq!(t.claim_strategist_fire(|_, _| true), Some(2));
    let o = t.snapshot().unwrap();
    assert_eq!((o.consecutive_not_achieved, o.last_strategist_fired_at), (2, 2));
}

#[test]
fn claim_strategist_fire_skips_and_preserves_state_when_predicate_false() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    let _ = t.record_not_achieved_streak();
    assert_eq!(t.claim_strategist_fire(|_, _| false), None);
    let o = t.snapshot().unwrap();
    assert_eq!(o.last_strategist_fired_at, 0, "no fire => marker untouched");
    assert_eq!(o.strategist_cap_bonus, 0, "no fire => no cap bonus");
}

#[test]
fn strategist_fire_grants_cap_bonus_then_reset_clears_it() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    assert_eq!(t.snapshot().unwrap().strategist_cap_bonus, 0);
    let _ = t.record_not_achieved_streak();
    let _ = t.record_not_achieved_streak();
    let _ = t.claim_strategist_fire(|_, _| true);
    assert_eq!(t.snapshot().unwrap().strategist_cap_bonus, 3,);
    t.reset_strategist_state();
    assert_eq!(t.snapshot().unwrap().strategist_cap_bonus, 0);
}

#[test]
fn reset_strategist_state_clears_streak_marker_and_recommendation() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    let _ = t.record_not_achieved_streak();
    let _ = t.record_not_achieved_streak();
    let _ = t.claim_strategist_fire(|_, _| true);
    t.record_strategy_recommendation("/tmp/goal/strategy.md".into(), "split it".into());

    t.reset_strategist_state();

    let o = t.snapshot().unwrap();
    assert_eq!(o.consecutive_not_achieved, 0);
    assert_eq!(o.last_strategist_fired_at, 0);
    assert!(o.last_strategy_path.is_none());
    assert!(o.last_strategy_recommendation.is_none());
}

#[test]
fn elapsed_ms_accrues_while_active_and_freezes_when_paused() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let running = t.elapsed_ms();
    assert!(running > 0, "active goal must accrue wall-clock time");

    assert!(t.pause(GoalPauseReason::User));
    let frozen = t.elapsed_ms();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(t.elapsed_ms(), frozen, "paused goal must not accrue time");
}

#[test]
fn account_elapsed_flushes_baseline_without_double_counting() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    std::thread::sleep(std::time::Duration::from_millis(150));
    t.account_elapsed();
    let after_first_flush = t.elapsed_ms();
    t.account_elapsed();
    let second_increment = t.elapsed_ms().saturating_sub(after_first_flush);
    assert!(
        second_increment < after_first_flush,
        "immediate second flush must add ~0ms, got {second_increment}ms on top of {after_first_flush}ms"
    );
}

#[test]
fn record_token_observation_tracks_monotonic_high_water() {
    let mut t = make_tracker();
    activate_tracker(&mut t);
    assert_eq!(t.tokens_used(), 0);
    t.record_token_observation(500);
    assert_eq!(t.tokens_used(), 500);
    t.record_token_observation(300);
    assert_eq!(t.tokens_used(), 500, "high-water mark must not decrease");
    t.record_token_observation(700);
    assert_eq!(t.tokens_used(), 700);

    let mut fresh = make_tracker();
    activate_tracker(&mut fresh);
    fresh.record_token_observation(-50);
    assert_eq!(fresh.tokens_used(), 0, "usage floors at zero");
}

#[test]
fn set_objective_replaces_in_place_and_trims() {
    let mut t = make_tracker();
    assert!(!t.set_objective("no goal"), "must fail without a goal");
    activate_tracker(&mut t);
    assert!(t.set_objective("  Ship it  "));
    assert_eq!(t.objective(), Some("Ship it"));
    assert_eq!(t.status(), Some(GoalStatus::Active), "edit must preserve status");
}

#[test]
fn goal_status_labels_cover_every_variant() {
    assert_eq!(GoalStatus::Active.display_name(), "Active");
    assert_eq!(GoalStatus::UserPaused.display_name(), "Paused");
    assert_eq!(GoalStatus::BackOffPaused.display_name(), "Paused (backing off)");
    assert_eq!(GoalStatus::NoProgressPaused.display_name(), "Paused (no progress)");
    assert_eq!(GoalStatus::InfraPaused.display_name(), "Paused (infra error)");
    assert_eq!(GoalStatus::Blocked.display_name(), "Blocked");
    assert_eq!(GoalStatus::BudgetLimited.display_name(), "Budget limited");
    assert_eq!(GoalStatus::Complete.display_name(), "Complete");
}

#[test]
fn persist_and_restore_round_trip_preserves_paused_goal() {
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session-1");
    let mut t = GoalTracker::new(session_dir.clone());
    t.create_goal("goal-1".into(), "Build a widget".into(), Some(100_000), 0, "2026-01-01T00:00:00Z".into(), None);
    t.record_token_observation(250);
    assert!(t.pause_with_message(GoalPauseReason::Infra, "Turn failed: auth".into()));
    t.persist().unwrap();

    let restored = GoalTracker::restore(session_dir).unwrap();
    assert!(restored.has_goal());
    assert_eq!(restored.goal_id(), Some("goal-1"));
    assert_eq!(restored.status(), Some(GoalStatus::InfraPaused));
    assert_eq!(restored.objective(), Some("Build a widget"));
    assert_eq!(restored.token_budget(), Some(100_000));
    assert_eq!(restored.tokens_used(), 250);
    assert_eq!(restored.pause_message(), Some("Turn failed: auth"));
    assert!(!restored.is_active());
}

#[test]
fn restore_without_snapshot_returns_empty_tracker() {
    let dir = tempfile::tempdir().unwrap();
    let restored = GoalTracker::restore(dir.path().join("fresh")).unwrap();
    assert!(!restored.has_goal());
    assert_eq!(restored.status(), None);
}

#[test]
fn persist_after_clear_removes_snapshot_so_goal_never_resurrects() {
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session-1");
    let mut t = GoalTracker::new(session_dir.clone());
    t.create_goal("goal-1".into(), "Build a widget".into(), None, 0, "2026-01-01T00:00:00Z".into(), None);
    t.persist().unwrap();
    assert!(session_dir.join("goal").join("goal.json").exists());

    t.clear();
    t.persist().unwrap();
    let restored = GoalTracker::restore(session_dir).unwrap();
    assert!(!restored.has_goal());
}

#[test]
fn to_goal_updated_event_maps_state_and_cleared() {
    let empty = make_tracker();
    let cleared = empty.to_goal_updated_event();
    assert!(
        matches!(cleared, vtcode_exec_events::ThreadEvent::GoalUpdated(e) if e.goal.is_none()),
        "tracker without a goal must emit a cleared payload"
    );

    let mut t = make_tracker();
    activate_tracker(&mut t);
    match t.to_goal_updated_event() {
        vtcode_exec_events::ThreadEvent::GoalUpdated(e) => {
            let g = e.goal.expect("active goal must emit a snapshot");
            assert_eq!(g.goal_id, "goal-1");
            assert_eq!(g.state, vtcode_exec_events::goal::GoalState::Active);
            assert_eq!(g.objective, "Build a widget");
            assert_eq!(g.token_budget, Some(100_000));
        }
        other => panic!("expected GoalUpdated, got {other:?}"),
    }

    t.pause(GoalPauseReason::BackOff);
    let paused = t.to_goal_updated_event();
    assert!(
        matches!(paused, vtcode_exec_events::ThreadEvent::GoalUpdated(e) if e.goal.as_ref().is_some_and(|g| g.state == vtcode_exec_events::goal::GoalState::Paused)),
        "fine-grained pause reasons must collapse to Paused on the wire"
    );
}

#[test]
fn restore_demotes_in_flight_active_goal_to_paused() {
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session-1");
    let mut t = GoalTracker::new(session_dir.clone());
    t.create_goal("goal-1".into(), "Build a widget".into(), None, 0, "2026-01-01T00:00:00Z".into(), None);
    t.persist().unwrap();

    let restored = GoalTracker::restore(session_dir).unwrap();
    assert_eq!(restored.status(), Some(GoalStatus::UserPaused));
    assert!(!restored.is_active());
}
