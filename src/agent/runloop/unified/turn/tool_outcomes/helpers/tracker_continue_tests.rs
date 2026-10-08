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

#[test]
fn plan_mode_auto_continue_gate_respects_user_gates_and_budget() {
    // Ready-for-approval is a user gate.
    assert!(!should_queue_plan_mode_auto_continue(true, true, true, true, None, false, 8, 0));
    // Ordinary completed planning turns never auto-continue (interview risk).
    assert!(!should_queue_plan_mode_auto_continue(true, true, false, true, None, false, 8, 0));
    // Recoverable blocked planning continues.
    assert!(should_queue_plan_mode_auto_continue(
        true,
        true,
        false,
        false,
        Some("reached the safety cap"),
        false,
        8,
        0
    ));
    // Planning handoff / verification / blocked-without-reason stay off.
    // Production PLANNING_COMPLETED_TURN_FALLBACK_REASON is recoverable
    // (allow-list after true-handoff deny) so planning auto-continues.
    assert!(should_queue_plan_mode_auto_continue(
        true,
        true,
        false,
        false,
        Some(
            "Planning turn ended via recovery fallback without confirming an approval-ready plan; planning remains active."
        ),
        false,
        8,
        0
    ));
    // Compound permission+recovery stays denied.
    assert!(!should_queue_plan_mode_auto_continue(
        true,
        true,
        false,
        false,
        Some("recovery fallback; permission denied for exec_command"),
        false,
        8,
        0
    ));
    assert!(!should_queue_plan_mode_auto_continue(
        true,
        true,
        false,
        false,
        Some("pending verification"),
        true,
        8,
        0
    ));
    assert!(!should_queue_plan_mode_auto_continue(true, true, false, false, None, false, 8, 0));
    // Kill-switch / zero budget / inactive planning stay off.
    assert!(!should_queue_plan_mode_auto_continue(false, true, false, false, Some("turn budget"), false, 8, 0));
    assert!(!should_queue_plan_mode_auto_continue(true, true, false, false, Some("turn budget"), false, 0, 0));
    assert!(!should_queue_plan_mode_auto_continue(true, false, false, false, Some("turn budget"), false, 8, 0));
}

#[test]
fn plan_mode_auto_continue_recovers_entry_turn_blocked_shapes() {
    // Mid-turn start_planning + mutating attempts trip the blocked-tool fuse;
    // the entry turn must not park at the user Continue prompt.
    let fuse = "Blocked tool-call limit reached after 3 consecutive blocked calls (streak 3, total 3). Last blocked call: 'apply_patch'. Rebase the patch on current file contents and confirm edit approval before retrying. A bounded recovery response will run without more tool calls. History and outputs are retained. Type 'continue' with new guidance, or run `vtcode --resume <session>`; details: .vtcode/tasks/current_blocked.md.";
    assert!(plan_mode_recoverable_block(fuse));
    assert!(should_queue_plan_mode_auto_continue(true, true, false, false, Some(fuse), false, 8, 0));

    let recovery_fuse = "Recovery tool-call limit reached after 3 blocked calls (streak 3, total 3) (last blocked call: 'exec_command'). Check sandbox/approval policy, narrow the command, or request approval instead of retrying verbatim. History and outputs are retained. Type 'continue' with new guidance, or run `vtcode --resume <session>`; details: .vtcode/tasks/current_blocked.md.";
    assert!(plan_mode_recoverable_block(recovery_fuse));
    assert!(should_queue_plan_mode_auto_continue(true, true, false, false, Some(recovery_fuse), false, 8, 0));

    // Tools ended without a published final after planning entry.
    let no_final = "Turn ended without a harness-visible final assistant response, so successful completion could not be confirmed.";
    assert!(plan_mode_recoverable_block(no_final));
    assert!(should_queue_plan_mode_auto_continue(true, true, false, false, Some(no_final), false, 8, 0));

    // Tools outside the named remedy set use the default remedy. Keep that
    // prose free of the deny token `permission` so a fuse trip on e.g.
    // `write_file` still auto-continues plan-mode research.
    let default_remedy_fuse = "Blocked tool-call limit reached after 3 consecutive blocked calls (streak 3, total 3). Last blocked call: 'write_file'. Adjust arguments, policy, or approvals instead of retrying the identical call. A bounded recovery response will run without more tool calls. History and outputs are retained. Type 'continue' with new guidance, or run `vtcode --resume <session>`; details: .vtcode/tasks/current_blocked.md.";
    assert!(plan_mode_recoverable_block(default_remedy_fuse));
    assert!(should_queue_plan_mode_auto_continue(
        true,
        true,
        false,
        false,
        Some(default_remedy_fuse),
        false,
        8,
        0
    ));

    // True permission handoffs still never auto-continue.
    assert!(!plan_mode_recoverable_block(
        "Blocked tool-call limit reached; permission denied for apply_patch; user input required."
    ));
    assert!(!should_queue_plan_mode_auto_continue(
        true,
        true,
        false,
        false,
        Some("permission denied for apply_patch"),
        false,
        8,
        0
    ));
}

#[test]
fn plan_mode_auto_continue_stops_after_consecutive_empty_fallbacks() {
    let reason = Some(
        "Planning turn ended via recovery fallback without confirming an approval-ready plan; planning remains active.",
    );
    assert!(should_queue_plan_mode_auto_continue(true, true, false, false, reason, false, 32, 0));
    assert!(should_queue_plan_mode_auto_continue(true, true, false, false, reason, false, 32, 1));
    assert!(!should_queue_plan_mode_auto_continue(
        true,
        true,
        false,
        false,
        reason,
        false,
        32,
        MAX_PLAN_EMPTY_FALLBACK_AUTO_CONTINUE
    ));
    assert!(!should_queue_plan_mode_auto_continue(true, true, false, false, reason, false, 32, 3));
}

#[test]
fn detects_plan_empty_fallback_text() {
    let empty = "Planning remains active, but this turn ended without a final plan synthesis. The research gathered above is preserved, so the next turn can reuse it without re-reading files. Type `keep planning`.";
    assert!(is_plan_empty_fallback_text(empty));
    // The gate must recognize the production fallback text, not only the fixture.
    assert!(is_plan_empty_fallback_text(
        crate::agent::runloop::unified::turn::turn_loop::PLANNING_COMPLETED_FALLBACK_RESPONSE
    ));
    assert!(!is_plan_empty_fallback_text("Planning turn ended via recovery fallback without confirming plan."));
    assert!(!is_plan_empty_fallback_text(""));
}

#[test]
fn plan_mode_recoverable_block_allow_list() {
    assert!(plan_mode_recoverable_block("turn budget exhausted"));
    assert!(plan_mode_recoverable_block("reached the safety cap"));
    assert!(plan_mode_recoverable_block("tool-free recovery after safety cap"));
    assert!(plan_mode_recoverable_block(
        "Turn ended with a recovery fallback; the requested work was not confirmed."
    ));
    // Production planning fallback must auto-queue (allow-list first).
    assert!(plan_mode_recoverable_block(
        "Planning turn ended via recovery fallback without confirming an approval-ready plan; planning remains active."
    ));
    assert!(plan_mode_recoverable_block(
        "Tool loop budget exhausted before a final response; planning remains active."
    ));
    // Interview / approval / permission handoffs stay denied.
    assert!(!plan_mode_recoverable_block("request_user_input pending"));
    assert!(!plan_mode_recoverable_block("permission required"));
    assert!(!plan_mode_recoverable_block(
        "Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls."
    ));
    // Compound recovery+permission must deny (true handoff first).
    assert!(!plan_mode_recoverable_block("recovery fallback after permission denied for exec_command"));
    assert!(!plan_mode_recoverable_block("tool budget exhausted while awaiting user approval"));
    // Budget-exhausted verification blocks retry, unless a harder handoff
    // signal shares the reason.
    assert!(plan_mode_recoverable_block(
        "Verification is still pending: the turn tool-call budget is exhausted; resume with fresh execution budget."
    ));
    assert!(!plan_mode_recoverable_block(
        "Verification is still pending: the turn tool-call budget is exhausted; permission denied by policy."
    ));
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
    for prompt in [
        plan_mode_continue_follow_up(),
        plan_mode_auto_continue_directive(),
        plan_mode_resume_directive(),
    ] {
        assert!(prompt.contains("no user reply is needed"), "{prompt}");
        assert!(prompt.contains("read-only"), "{prompt}");
        assert!(prompt.contains("code changes wait for plan approval"), "{prompt}");
        assert!(prompt.contains("<proposed_plan>"), "{prompt}");
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
    assert!(!plan_mode_recoverable_block(&reason));
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
    assert!(!should_queue_plan_mode_auto_continue(true, true, false, false, Some(&reason), false, 3, 0));
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
fn production_block_reasons_stay_denied_by_both_classifiers() {
    // The deny-token tables mirror production `Blocked`-reason constants
    // by substring. Feed the real constants (not copies) so a wording
    // change that drops a token fails here instead of silently flipping
    // auto-continue behavior while every hardcoded test stays green.
    for reason in [
        crate::agent::runloop::unified::turn::turn_loop::RECOVERY_CONTRACT_VIOLATION_REASON,
        crate::agent::runloop::unified::turn::turn_loop::PENDING_VERIFICATION_BLOCK_REASON,
    ] {
        assert!(!tracker_auto_continue_is_recoverable_block(Some(reason)), "{reason}");
        assert!(!plan_mode_recoverable_block(reason), "{reason}");
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
    // Budget-exhausted verification blocks queue a bounded fresh-turn retry,
    // even though generic verification blocks do not.
    assert!(should_queue_tracker_auto_continue(
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
    assert!(should_queue_tracker_resume_continuation(true, 8, Some(&incomplete)));
    assert!(!should_queue_tracker_resume_continuation(true, 0, Some(&incomplete)));
    assert!(!should_queue_tracker_resume_continuation(false, 8, Some(&incomplete)));
    assert!(!should_queue_tracker_resume_continuation(true, 8, None));
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
fn plan_mode_recoverable_allows_planning_fallback_constant() {
    // PLANNING_COMPLETED_TURN_FALLBACK_REASON must auto-queue (allow-list first).
    assert!(plan_mode_recoverable_block(
        "Planning turn ended via recovery fallback without confirming an approval-ready plan; planning remains active. The current plan and task state were retained."
    ));
    assert!(plan_mode_recoverable_block(
        "Turn ended with a recovery fallback; the requested work was not confirmed."
    ));
    // Interview / approval / permission handoffs stay denied.
    assert!(!plan_mode_recoverable_block("request_user_input is required for the planning interview"));
    assert!(!plan_mode_recoverable_block("permission denied for exec_command"));
    assert!(!plan_mode_recoverable_block(
        "Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls."
    ));
    assert!(!plan_mode_recoverable_block("recovery fallback; interview waiting on user decision"));
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
