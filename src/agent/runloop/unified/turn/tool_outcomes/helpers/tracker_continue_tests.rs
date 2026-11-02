use super::*;

#[test]
fn parse_incomplete_tracker_items_shapes() {
    assert!(parse_incomplete_tracker_items(&serde_json::json!({"status":"empty"})).is_none());
    let complete = serde_json::json!({
        "status":"ok",
        "checklist":{"items":[{"description":"a","status":"completed"}]}
    });
    assert!(parse_incomplete_tracker_items(&complete).is_none());
    let mixed = serde_json::json!({
        "status":"ok",
        "checklist":{"items":[
            {"index":1,"description":"analyze","status":"completed"},
            {"index":2,"description":"change","status":"in_progress"},
            {"index":3,"description":"verify","status":"pending"}
        ]}
    });
    let items = parse_incomplete_tracker_items(&mixed).expect("incomplete");
    assert_eq!(items, vec!["#2 change (in_progress)".to_string(), "#3 verify (pending)".to_string()]);
}

#[test]
fn tracker_follow_up_lists_items_and_forbids_nudge() {
    let prompt = tracker_continue_follow_up(&["#2 change (pending)".to_string(), "#3 verify (blocked)".to_string()]);
    assert!(prompt.contains("#2 change (pending)"));
    assert!(prompt.contains("#3 verify (blocked)"));
    assert!(prompt.contains("no user reply is needed"));
    assert!(prompt.contains("A status-only recap does not advance the tracker"));
}

#[test]
fn tracker_continue_directive_is_shared_calm_prose() {
    let incomplete = ["#2 change (pending)".to_string(), "#3 verify (pending)".to_string()];
    for label in [TRACKER_RESUME_DIRECTIVE_LABEL, TRACKER_AUTO_CONTINUE_DIRECTIVE_LABEL] {
        let directive = tracker_continue_directive(label, &incomplete);
        assert!(directive.starts_with(&format!("{label}: task_tracker still has incomplete steps:")));
        assert!(directive.contains("#2 change (pending), #3 verify (pending)"));
        assert!(directive.contains("no user reply is needed"));
        assert!(directive.contains("status-only recap does not advance the tracker"));
        assert!(!directive.contains("do not"), "directive states consequences, not prohibitions: {directive}");
    }
}

fn open_planning_auto_continue_gates() -> PlanningAutoContinueGates {
    PlanningAutoContinueGates {
        enabled: true,
        planning_active: true,
        plan_ready_for_approval: false,
        turn_completed: false,
        verification_blocked: false,
        refused: false,
        hard_budget_exhausted: false,
        stop_requested: false,
        cross_turn_turns: 8,
    }
}

#[test]
fn typed_planning_auto_continue_allows_only_synthesis_and_repair_outcomes() {
    use vtcode_core::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot};
    for outcome in [PlanningOutcome::Synthesizing, PlanningOutcome::Repairing] {
        let recovery = PlanningRecoverySnapshot { outcome, requests_used: 1, ..Default::default() };
        assert!(should_queue_plan_mode_auto_continue(&recovery, open_planning_auto_continue_gates()), "{outcome:?}");
    }
    for outcome in [
        PlanningOutcome::Researching,
        PlanningOutcome::AwaitingInput,
        PlanningOutcome::AwaitingApproval,
    ] {
        let recovery = PlanningRecoverySnapshot { outcome, requests_used: 1, ..Default::default() };
        assert!(!should_queue_plan_mode_auto_continue(&recovery, open_planning_auto_continue_gates()), "{outcome:?}");
    }
}

#[test]
fn typed_planning_auto_continue_shares_the_three_request_allowance() {
    use vtcode_core::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot};
    let mut recovery = PlanningRecoverySnapshot {
        outcome: PlanningOutcome::Repairing,
        requests_used: 2,
        ..Default::default()
    };
    assert!(should_queue_plan_mode_auto_continue(&recovery, open_planning_auto_continue_gates()));
    recovery.requests_used = 3;
    assert!(!should_queue_plan_mode_auto_continue(&recovery, open_planning_auto_continue_gates()));
    recovery.requests_used = 4;
    assert!(!should_queue_plan_mode_auto_continue(&recovery, open_planning_auto_continue_gates()));
}

#[test]
fn typed_planning_auto_continue_respects_approval_refusal_stop_and_budget_gates() {
    use vtcode_core::exec::events::planning::{PlanningOutcome, PlanningRecoverySnapshot};
    let recovery = PlanningRecoverySnapshot {
        outcome: PlanningOutcome::Synthesizing,
        ..Default::default()
    };
    let open = open_planning_auto_continue_gates();
    assert!(should_queue_plan_mode_auto_continue(&recovery, open));
    for closed in [
        PlanningAutoContinueGates { enabled: false, ..open },
        PlanningAutoContinueGates { planning_active: false, ..open },
        PlanningAutoContinueGates { plan_ready_for_approval: true, ..open },
        PlanningAutoContinueGates { turn_completed: true, ..open },
        PlanningAutoContinueGates { verification_blocked: true, ..open },
        PlanningAutoContinueGates { refused: true, ..open },
        PlanningAutoContinueGates { hard_budget_exhausted: true, ..open },
        PlanningAutoContinueGates { stop_requested: true, ..open },
        PlanningAutoContinueGates { cross_turn_turns: 0, ..open },
    ] {
        assert!(!should_queue_plan_mode_auto_continue(&recovery, closed));
    }
}

#[test]
fn plan_progress_line_shapes() {
    assert_eq!(plan_progress_line("Release", false, 0, 0), "• Plan Release — research/synthesis");
    assert_eq!(plan_progress_line("Release", false, 2, 4), "• Plan Release — open decisions: 2");
    assert_eq!(plan_progress_line("Release", true, 0, 4), "• Plan Release — ready for approval (4 steps)");
    assert_eq!(plan_progress_line("Release", true, 0, 0), "• Plan Release — ready for approval");
    assert_eq!(plan_progress_line("", true, 0, 0), "• Plan — ready for approval");
    assert_eq!(plan_progress_line("", false, 0, 0), "• Plan — research/synthesis");
    assert_eq!(plan_progress_line("", true, 0, 4), "• Plan — ready for approval (4 steps)");
    assert_eq!(plan_progress_line("", false, 1, 0), "• Plan — open decisions: 1");
}

#[test]
fn plan_mode_continue_messages_keep_planning_read_only_without_a_user_nudge() {
    for prompt in [plan_mode_continue_follow_up(), plan_mode_auto_continue_directive()] {
        assert!(prompt.contains("no user reply is needed"), "{prompt}");
        assert!(prompt.contains("read-only"), "{prompt}");
        assert!(prompt.contains("code changes wait for plan approval"), "{prompt}");
        assert!(prompt.contains("<proposed_plan>"), "{prompt}");
        assert!(prompt.contains("repair the retained draft from saved evidence"), "{prompt}");
        assert!(prompt.contains("Research stays closed"), "{prompt}");
        let normalized = vtcode_core::planning::normalize_plan_intent(&prompt);
        assert!(vtcode_core::planning::matches_stay_intent(&normalized), "{prompt}");
        assert!(!vtcode_core::planning::contains_implementation_cue(&normalized), "{prompt}");
    }
    assert!(plan_mode_continue_follow_up().starts_with(PLAN_MODE_AUTO_CONTINUE_MARKER));
    assert!(plan_mode_auto_continue_directive().starts_with(PLAN_MODE_AUTO_CONTINUE_MARKER));
}

#[test]
fn refusal_notices_never_auto_continue() {
    // A refusal explanation may quote text that matches recoverable
    // tokens ("recovery fallback", "tool budget"); the refusal still wins.
    let reason = format!(
        "{}: the request looked like a recovery fallback for a tool budget bypass. \
         The request was not retried; rephrase it or switch models.",
        refusal::REFUSAL_NOTICE_PREFIX
    );
    assert!(!tracker_auto_continue_is_recoverable_block(Some(&reason)));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(&reason),
        false,
        Some(&["step".to_string()]),
        3,
        false,
        false,
    ));
}

#[test]
fn recoverable_block_classification_allow_list() {
    // Production RECOVERY_CONTRACT_VIOLATION_REASON — must NOT auto-queue
    // despite containing "tool-free".
    assert!(!tracker_auto_continue_is_recoverable_block(Some(
        "Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls."
    )));
    // Blocked with unknown/missing reason is not auto-queued.
    assert!(!tracker_auto_continue_is_recoverable_block(None));
    // Production PENDING_VERIFICATION_BLOCK_REASON.
    assert!(!tracker_auto_continue_is_recoverable_block(Some(
        "Turn blocked after repeated unverified assistant responses; verification is still pending."
    )));
    // Session-vtcode-20261008T094713Z: budget-exhausted verification block
    // resumes on a fresh turn with fresh execution budget.
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Verification is still pending: the turn tool-call budget is exhausted. No verifier was scheduled; resume with fresh execution budget."
    )));
    // Budget exception still denies harder handoffs sharing the same wording.
    assert!(!tracker_auto_continue_is_recoverable_block(Some(
        "Verification is still pending: the turn tool-call budget is exhausted; permission denied by policy."
    )));
    // Production POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON.
    assert!(!tracker_auto_continue_is_recoverable_block(Some(
        "The provider rejected the follow-up because the context exceeded its capacity, and the bounded recovery compaction could not reduce the request."
    )));
    // Recoverable production constants.
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained."
    )));
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Turn blocked after repeated assistant responses reached the safety cap; the latest response was preserved."
    )));
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Post-tool recovery could not confirm the requested work after one bounded tool-enabled retry. The completed tool outputs and resume handoff were retained; retry from the pending step."
    )));
    assert!(tracker_auto_continue_is_recoverable_block(Some("preview budget exhausted")));
    assert!(tracker_auto_continue_is_recoverable_block(Some("Turn blocked due to repeated failing behavior.")));
    // Session/production budget phrases from residual UX work.
    assert!(tracker_auto_continue_is_recoverable_block(Some("Task 7 blocked by the turn's preview budget")));
    assert!(tracker_auto_continue_is_recoverable_block(Some("tool budget ran out")));
    assert!(tracker_auto_continue_is_recoverable_block(Some("hit the per-file read cap")));
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Tool loop budget exhausted before a final response."
    )));
    // True handoffs stay terminal.
    assert!(!tracker_auto_continue_is_recoverable_block(Some(
        "Anti-blind checkpoint: verification is still pending"
    )));
    assert!(!tracker_auto_continue_is_recoverable_block(Some("verification gate remains open")));
    // Unknown / policy handoffs stay terminal.
    assert!(!tracker_auto_continue_is_recoverable_block(Some("some unknown block")));
    assert!(!tracker_auto_continue_is_recoverable_block(Some("exec_command is denied by permission policy")));
    assert!(!tracker_auto_continue_is_recoverable_block(Some(
        "I hit the tool-call safety fuse mid-verification"
    )));
    assert!(!tracker_auto_continue_is_recoverable_block(Some("request_user_input is required")));
    assert!(!tracker_auto_continue_is_recoverable_block(Some(
        "Turn blocked after repeated unverified assistant responses; verification is still pending."
    )));
    // Session/production recoverable vocabulary parity.
    assert!(tracker_auto_continue_is_recoverable_block(Some("Task 7 blocked by the turn's preview budget")));
    assert!(tracker_auto_continue_is_recoverable_block(Some("tool budget ran out")));
    assert!(tracker_auto_continue_is_recoverable_block(Some("per-file read cap")));
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Tool loop budget exhausted before a final response"
    )));
    assert!(tracker_auto_continue_is_recoverable_block(Some("work budget exhausted")));
}

#[test]
fn production_block_reasons_stay_denied_by_tracker_classifier() {
    // The deny-token tables mirror production `Blocked`-reason constants
    // by substring. Feed the real constants (not copies) so a wording
    // change that drops a token fails here instead of silently flipping
    // auto-continue behavior while every hardcoded test stays green.
    for reason in [
        crate::agent::runloop::unified::turn::turn_loop::RECOVERY_CONTRACT_VIOLATION_REASON,
        crate::agent::runloop::unified::turn::turn_loop::PENDING_VERIFICATION_BLOCK_REASON,
    ] {
        assert!(!tracker_auto_continue_is_recoverable_block(Some(reason)), "{reason}");
    }
}

#[test]
fn outer_queue_gate_blocks_unknown_and_contract_violation() {
    let incomplete = ["#2 change (pending)".to_string()];
    // Completed + incomplete tracker → queue when not a handoff/question.
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        true,
        None,
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    // Completed + genuine user question → do not queue past the ask.
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        true,
        None,
        false,
        Some(&incomplete),
        8,
        false,
        true
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        None,
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls."),
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(
            "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained."
        ),
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    // Budget-exhausted verification blocks use the separate verifier-first
    // recovery path rather than spending the tracker continuation budget.
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(
            "Verification is still pending: the turn tool-call budget is exhausted. No verifier was scheduled; resume with fresh execution budget."
        ),
        true,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(crate::agent::runloop::unified::turn::turn_loop::PENDING_VERIFICATION_BLOCK_REASON),
        true,
        Some(&incomplete),
        8,
        false,
        false
    ));
}

#[test]
fn recoverable_blocked_continues_without_tracker_items() {
    // Session evidence: build/auto recovery-fallback and fuse trips parked
    // at Continue when no tracker was active. Recoverable blocked ends must
    // still auto-continue (bounded by cross_turn_turns).
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(
            "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained."
        ),
        false,
        None,
        8,
        false,
        false
    ));
    let empty: &[String] = &[];
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("Turn ended with a recovery fallback; the requested work was not confirmed."),
        false,
        Some(empty),
        8,
        false,
        false
    ));
    // Fuse / no-final shapes (common after read-only policy blocks).
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("Blocked tool-call limit reached after 3 consecutive blocked calls."),
        false,
        None,
        8,
        false,
        false
    ));
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(
            "Turn ended without a harness-visible final assistant response, so successful completion could not be confirmed."
        ),
        false,
        None,
        8,
        false,
        false
    ));
    // Completed without tracker still requires incomplete items.
    assert!(!should_queue_tracker_auto_continue(true, false, true, None, false, None, 8, false, false));
    // Verification / unknown blocked stay off.
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("Turn blocked after repeated unverified assistant responses; verification is still pending."),
        true,
        None,
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(true, false, false, None, false, None, 8, false, false));
}

#[test]
fn outer_queue_gate_respects_planning_verification_and_budget() {
    let incomplete = ["#2 change (pending)".to_string()];
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        true,
        None,
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(
        false,
        false,
        true,
        None,
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        true,
        true,
        None,
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(true, false, true, None, false, None, 8, false, false));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        true,
        None,
        false,
        Some(&incomplete),
        0,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        true,
        None,
        false,
        Some(&incomplete),
        8,
        true,
        false,
        // safety-handoff final text
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("pending verification; type continue"),
        true,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("preview budget exhausted"),
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(
            "Turn ended with a recovery fallback; the requested work was not confirmed. The current plan and task state were retained."
        ),
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(
            "Turn blocked after repeated assistant responses reached the safety cap; the latest response was preserved."
        ),
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("permission denied"),
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some("unknown block reason"),
        false,
        Some(&incomplete),
        8,
        false,
        false
    ));
}

#[test]
fn resume_gate_honors_zero_cross_turn_budget() {
    let incomplete = ["#1 analyze (in_progress)".to_string()];
    assert!(should_queue_tracker_resume_continuation(true, true, 8, Some(&incomplete)));
    assert!(!should_queue_tracker_resume_continuation(true, false, 8, Some(&incomplete)));
    assert!(!should_queue_tracker_resume_continuation(true, true, 0, Some(&incomplete)));
    assert!(!should_queue_tracker_resume_continuation(false, true, 8, Some(&incomplete)));
    assert!(!should_queue_tracker_resume_continuation(true, true, 8, None));
}

#[test]
fn restored_informational_session_does_not_adopt_workspace_tracker() {
    use crate::agent::runloop::unified::turn::context::tracker_continuation_adoption_allowed;

    let incomplete = ["#1 unrelated README work (pending)".to_string()];
    let mut history = vec![
        uni::Message::user("what is vtcode".to_string()),
        uni::Message::assistant("VT Code is a coding assistant.".to_string()),
    ];
    assert!(!should_queue_tracker_resume_continuation(
        true,
        tracker_continuation_adoption_allowed(&history),
        8,
        Some(&incomplete),
    ));
    history.push(uni::Message::user("continue".to_string()));
    assert!(should_queue_tracker_resume_continuation(
        true,
        tracker_continuation_adoption_allowed(&history),
        8,
        Some(&incomplete),
    ));
}

#[test]
fn budget_verification_retry_survives_the_real_final_response_handoff_check() {
    let reason = "Verification is still pending: the turn tool-call budget is exhausted. No verifier was scheduled; resume with fresh execution budget.";
    let text = crate::agent::runloop::unified::turn::turn_loop::format_blocked_turn_final_response(reason);
    // The ordinary classifier must still stop in-turn continuation.
    assert!(vtcode_core::core::agent::completion::tracker_final_text_is_safety_handoff(&text));
    assert!(is_pending_verification_block(reason));
    assert!(verification_block_allows_auto_recovery(reason, &text));
    assert!(!should_queue_tracker_auto_continue(
        true,
        false,
        false,
        Some(reason),
        true,
        None,
        8,
        true,
        vtcode_core::core::agent::completion::tracker_final_text_requires_user_input(&text),
    ));
    for hard_handoff in [
        format!("{text} Permission denied for exec_command."),
        format!("{text} The safety fuse prevents execution."),
        format!("{text} Missing credentials."),
    ] {
        assert!(!verification_block_allows_auto_recovery(reason, &hard_handoff));
    }
    assert!(!verification_block_allows_auto_recovery("unknown block", &text));
    assert!(!is_pending_verification_block(
        "Analysis quoted 'verification is still pending' from an older turn."
    ));
    assert!(verification_block_allows_auto_recovery(
        crate::agent::runloop::unified::turn::turn_loop::PENDING_VERIFICATION_BLOCK_REASON,
        &text,
    ));
    assert!(!verification_block_allows_auto_recovery(&format!("{reason} Permission denied."), &text));
    assert!(!verification_block_allows_auto_recovery(reason, "Which verifier should I run?"));
    let mut stats = crate::agent::runloop::unified::state::SessionStats::default();
    // This path gets one corrective pass, independently of the tracker limit.
    assert!(stats.record_verification_auto_recovery_turn_with_limit(verification_cross_turn_turns(None)));
    assert!(!stats.record_verification_auto_recovery_turn_with_limit(verification_cross_turn_turns(None)));
    assert_eq!(stats.tracker_continuation_turns(), 0);
}

#[test]
fn tracker_config_defaults() {
    assert!(tracker_auto_continue_enabled(None));
    assert_eq!(tracker_cross_turn_turns(None), 32);
}

#[test]
fn recoverable_block_includes_tool_budget_and_plan_fallback_shapes() {
    // Production TOOL_LOOP_LIMIT_RECOVERY_REASON.
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Tool loop budget exhausted before a final response. Tools are disabled for one bounded synthesis pass."
    )));
    // Tool-call budget / follow-up recovery constants.
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Tool follow-up failed. Tools disabled; respond with text using context and recent tool outputs."
    )));
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Turn ended without a harness-visible final assistant response, so successful completion could not be confirmed."
    )));
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Approved-plan execution stopped after recovery was exhausted. The approved plan and task checklist were retained."
    )));
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Per-turn tool limit reached (max: 32). Wait or adjust config."
    )));
    // Planning fallback contains "recovery fallback" (recoverable token);
    // outer tracker queue is separately gated by planning_active.
    assert!(tracker_auto_continue_is_recoverable_block(Some(
        "Planning turn ended via recovery fallback without confirming an approval-ready plan; planning remains active."
    )));
}

#[test]
fn session_stats_progress_resets_tracker_budget() {
    let mut stats = crate::agent::runloop::unified::state::SessionStats::default();
    assert!(stats.record_tracker_continuation_turn_with_limit(32));
    assert!(stats.record_tracker_continuation_turn_with_limit(32));
    assert_eq!(stats.tracker_continuation_turns(), 2);
    // No first observation yet — initial 0 → 0 is not progress.
    assert!(!stats.note_tracker_completed_count(0));
    assert_eq!(stats.tracker_continuation_turns(), 2);
    // Progress → reset episode budget.
    assert!(stats.note_tracker_completed_count(1));
    stats.reset_tracker_continuation_budget();
    assert_eq!(stats.tracker_continuation_turns(), 0);
    // Same count is not further progress.
    assert!(!stats.note_tracker_completed_count(1));
    // Tracker recreate and re-completing the same number of items do not
    // restore the budget; only a new completion high-water mark does.
    assert!(stats.record_tracker_continuation_turn_with_limit(32));
    assert!(!stats.note_tracker_completed_count(0));
    assert!(!stats.note_tracker_completed_count(1));
    assert_eq!(stats.tracker_continuation_turns(), 1);
    assert!(stats.note_tracker_completed_count(2));
    stats.reset_tracker_continuation_budget();
    assert_eq!(stats.tracker_continuation_turns(), 0);
}

#[test]
fn tracker_status_churn_cannot_keep_continuation_budget_alive() {
    use crate::agent::runloop::unified::state::{FollowUpPromptAction, SessionStats};

    let mut stats = SessionStats::default();
    assert!(stats.note_tracker_completed_count(1));
    let follow_up = tracker_continue_follow_up(&["#2 README (blocked)".to_owned()]);
    for completed in [0, 1, 0] {
        assert!(stats.record_tracker_continuation_turn_with_limit(3));
        assert_eq!(stats.register_follow_up_prompt(&follow_up), FollowUpPromptAction::None);
        assert!(!stats.note_tracker_completed_count(completed));
    }
    assert_eq!(stats.tracker_continuation_turns(), 3);
    assert!(!stats.record_tracker_continuation_turn_with_limit(3));
    assert!(!stats.note_tracker_completed_count(1));
    assert_eq!(stats.register_follow_up_prompt("Start the next task"), FollowUpPromptAction::None);
    assert_eq!(stats.tracker_continuation_turns(), 0);
    assert!(stats.note_tracker_completed_count(1), "a new user request starts a fresh progress episode");
}

#[test]
fn tracker_probe_cache_clears_on_complete_and_keeps_on_unavailable() {
    let mut cache: Option<Vec<String>> = Some(vec!["#1 a (pending)".to_string()]);
    // Complete is an authoritative clear.
    let effective = apply_tracker_probe_to_cache(&mut cache, TrackerProbeOutcome::Complete);
    assert!(effective.is_none());
    assert!(cache.is_none());
    // Incomplete replaces the cache.
    let effective = apply_tracker_probe_to_cache(
        &mut cache,
        TrackerProbeOutcome::Incomplete(vec!["#2 b (in_progress)".to_string()]),
    );
    assert_eq!(effective, Some(["#2 b (in_progress)".to_string()].as_slice()));
    // Unavailable keeps the last incomplete set.
    let effective = apply_tracker_probe_to_cache(&mut cache, TrackerProbeOutcome::Unavailable);
    assert_eq!(effective, Some(["#2 b (in_progress)".to_string()].as_slice()));
    // Parse distinguishes complete vs incomplete vs unavailable shapes.
    let complete = serde_json::json!({
        "status": "ok",
        "checklist": {"items": [{"index": 1, "description": "a", "status": "completed"}]}
    });
    assert_eq!(tracker_probe_outcome(&complete), TrackerProbeOutcome::Complete);
    assert!(parse_incomplete_tracker_items(&complete).is_none());
    let incomplete = serde_json::json!({
        "status": "ok",
        "checklist": {"items": [{"index": 1, "description": "a", "status": "pending"}]}
    });
    assert!(matches!(tracker_probe_outcome(&incomplete), TrackerProbeOutcome::Incomplete(_)));
    assert_eq!(tracker_probe_outcome(&serde_json::json!({"status": "empty"})), TrackerProbeOutcome::Complete);
    assert_eq!(tracker_probe_outcome(&serde_json::json!({"no_status": true})), TrackerProbeOutcome::Unavailable);
    // Checklist without items is malformed → Unavailable (keep cache), not Complete.
    assert_eq!(
        tracker_probe_outcome(&serde_json::json!({"status": "ok", "checklist": {}})),
        TrackerProbeOutcome::Unavailable
    );
}

#[test]
fn session_stats_apply_tracker_probe_clears_stale_incomplete() {
    let mut stats = crate::agent::runloop::unified::state::SessionStats::default();
    let after_incomplete =
        stats.apply_tracker_probe(TrackerProbeOutcome::Incomplete(vec!["#1 a (pending)".to_string()]));
    assert_eq!(after_incomplete, Some(["#1 a (pending)".to_string()].as_slice()));
    // Successful complete must clear — do not auto-continue after tracker finishes.
    assert!(stats.apply_tracker_probe(TrackerProbeOutcome::Complete).is_none());
    // Unavailable after a new incomplete keeps that incomplete set.
    stats.apply_tracker_probe(TrackerProbeOutcome::Incomplete(vec!["#2 b (pending)".to_string()]));
    assert_eq!(
        stats.apply_tracker_probe(TrackerProbeOutcome::Unavailable),
        Some(["#2 b (pending)".to_string()].as_slice())
    );
}

#[test]
fn parse_tracker_completed_count_from_list_payload() {
    let payload = serde_json::json!({
        "status": "ok",
        "checklist": {
            "items": [
                {"index": 1, "description": "a", "status": "completed"},
                {"index": 2, "description": "b", "status": "in_progress"},
                {"index": 3, "description": "c", "status": "pending"},
            ]
        }
    });
    assert_eq!(parse_tracker_completed_count(&payload), 1);
    assert_eq!(parse_incomplete_tracker_items(&payload).map(|items| items.len()), Some(2));
    let empty = serde_json::json!({"status": "empty"});
    assert_eq!(parse_tracker_completed_count(&empty), 0);
}

#[test]
fn session_stats_resets_tracker_budget_with_verification_episode() {
    let mut stats = crate::agent::runloop::unified::state::SessionStats::default();
    assert!(stats.record_tracker_continuation_turn_with_limit(8));
    assert!(stats.record_tracker_continuation_turn_with_limit(8));
    assert_eq!(stats.tracker_continuation_turns(), 2);
    // Verification-episode reset must NOT wipe tracker/plan continuation budgets.
    stats.reset_verification_recovery_episode();
    assert_eq!(stats.tracker_continuation_turns(), 2);
    assert!(stats.record_plan_continuation_turn_with_limit(1));
    assert!(!stats.record_plan_continuation_turn_with_limit(1));
    stats.reset_tracker_continuation_budget();
    assert_eq!(stats.tracker_continuation_turns(), 0);
    assert_eq!(stats.plan_continuation_turns(), 1);
    stats.reset_plan_continuation_budget();
    assert_eq!(stats.plan_continuation_turns(), 0);
}
