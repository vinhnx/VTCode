use super::*;

#[test]
fn contains_user_input_request_detects_various_patterns() {
    assert!(contains_user_input_request("how would you like to proceed?"));
    assert!(contains_user_input_request("what would you like me to do?"));
    assert!(contains_user_input_request("which option should i choose?"));
    assert!(contains_user_input_request("shall i continue?"));
    assert!(contains_user_input_request("do you want me to retry?"));
    assert!(contains_user_input_request("please confirm and i'll retry"));
    assert!(contains_user_input_request("let me know how to proceed"));
    assert!(contains_user_input_request("let me know what you'd like to dig into next"));
    assert!(contains_user_input_request("tell me which area you want next"));
    assert!(contains_user_input_request("waiting for your approval"));
    assert!(!contains_user_input_request(
        "The compiler errors tell me what to fix next. Let me update the parser branch now."
    ));
    assert!(!contains_user_input_request("Let me check whether this should initialize the cache before use."));
    assert!(!contains_user_input_request("let me fix the next issue"));
    assert!(!contains_user_input_request("i'll apply these changes now"));
}
#[test]
fn contains_user_input_request_detects_closing_offers() {
    assert!(contains_user_input_request(
        "verified and done. if you want a deeper pass, say the word and i'll do a larger pass."
    ));
    assert!(contains_user_input_request("all checks pass; happy to iterate further."));
    assert!(contains_user_input_request("the docs are updated — just ask if you want the full diff."));
}
#[test]
fn verified_recap_with_trailing_offer_does_not_continue() {
    // Regression guard for session-vtcode-20260912T083718Z: a conclusive
    // verification recap that closes by offering optional follow-up work
    // ("say the word and I'll do a larger pass") was relaxed-continued,
    // re-prompted into a second recap, and the turn ended Blocked on the
    // text cap after the verification gate had already cleared.
    let mut recap = String::from(
        "Verification passed: `cargo fmt --all -- --check && cargo check --locked -p vtcode` → exit 0.\n\n\
         **Recap**\n\n\
         **Changed (README.md):** tightened Overview prose, sharpened the pillar table, \
         streamlined Quick start phrasing, condensed the Documentation table, compacted the \
         Providers paragraph, and fixed a hyphen→em-dash in the TUI tip. Contributor and \
         sponsor HTML blocks preserved verbatim.\n\n\
         **Verified:** all referenced doc paths exist; the fmt and check commands exit 0.\n\n",
    );
    while recap.len() <= 800 {
        recap.push_str("Extra verified detail line that keeps the recap above the interim length cap.\n");
    }
    recap.push_str(
        "**Remaining risk:** none functional. If you want a deeper revamp (restructured \
         sections, feature highlights, refreshed screenshots), say the word and I'll do a \
         larger pass.",
    );
    assert!(recap.len() > 800, "recap must exceed the interim length cap to reach the relaxed path");

    let history = vec![
        uni::Message::user("polish the README intro".to_string()),
        uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "exec_command".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "Finished `dev` profile".to_string()),
    ];

    assert!(
        !evaluate_interim_text_continuation(true, false, &history, &recap, 0).should_continue,
        "a conclusive recap closing with an optional-work offer must end the turn"
    );
}
#[test]
fn tracker_incomplete_override_continues_status_recaps() {
    use super::apply_tracker_continuation_override;
    let history: Vec<uni::Message> = Vec::new();
    let recap = "## Status\nBlocked by wall-clock budget. Next step on resume: read design/diff.rs.";
    let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
    assert!(!base.should_continue);
    let overridden = apply_tracker_continuation_override(base, true, false, recap);
    assert!(overridden.should_continue);
    assert_eq!(overridden.reason, "tracker_incomplete_continuation");
    assert!(!overridden.is_relaxed_continuation);

    let question = "Should I proceed with the remaining steps?";
    let q_base = evaluate_interim_text_continuation(true, false, &history, question, 0);
    let q_over = apply_tracker_continuation_override(q_base, true, false, question);
    assert!(!q_over.should_continue);

    let plan_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, true, &history, recap, 0),
        true,
        true,
        recap,
    );
    assert!(!plan_over.should_continue);

    let complete_tracker = apply_tracker_continuation_override(base, false, false, recap);
    assert!(!complete_tracker.should_continue);

    // Permission/safety handoffs stay terminal for the user.
    for blocker in [
        "Permission denied for exec_command. Next step: retry after access is granted.",
        "Access denied to the sandbox; policy block.",
        "I hit the tool-call safety fuse mid-verification; policy block.",
        "Requires manual intervention from the workspace owner.",
        "Missing credentials for the provider route.",
    ] {
        let b_base = evaluate_interim_text_continuation(true, false, &history, blocker, 0);
        let b_over = apply_tracker_continuation_override(b_base, true, false, blocker);
        assert!(!b_over.should_continue, "must not continue on blocker: {blocker}");
    }

    // Budget-like "blocked by …" still continues when tracker work remains.
    let budget = "## Status\nBlocked by wall-clock budget. Next step: read design/diff.rs.";
    let budget_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, budget, 0),
        true,
        false,
        budget,
    );
    assert!(budget_over.should_continue);
    assert_eq!(budget_over.reason, "tracker_incomplete_continuation");

    // Tool-loop / wall-clock recaps continue even with "blocked by".
    for recap in [
        "## Status\nBlocked by tool loop budget. Next step: patch FileChangeItem.",
        "## Status\nTool loop budget exhausted this turn. Next: run cargo check.",
        "Task 3 mid-flight — hit the per-file read cap before editing. Next: one targeted read then patch.",
    ] {
        let over = apply_tracker_continuation_override(
            evaluate_interim_text_continuation(true, false, &history, recap, 0),
            true,
            false,
            recap,
        );
        assert!(over.should_continue, "must continue on recoverable recap: {recap}");
    }

    // Mid-text `?` in a status section is not a user handoff when tracker incomplete.
    let status_question = "## Status\nTask 2 done — is the next step clear? Continuing with task 3 now using tools.";
    let s_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, status_question, 0),
        true,
        false,
        status_question,
    );
    assert!(s_over.should_continue, "mid-text question in status recap must continue");

    // Broad mid-text "can you" / "need your" in status prose is not a handoff.
    let status_prose =
        "## Status\nNext we need your workspace-relative path in CONFIG; can you see it in the dump? Patching now.";
    let prose_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, status_prose, 0),
        true,
        false,
        status_prose,
    );
    assert!(prose_over.should_continue, "status prose mentioning need-your/can-you must continue");

    // Clause-start interview ask without a trailing `?` still ends the turn.
    let clause_ask = "Can you confirm which migration path to take before I edit the schema.";
    let c_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, clause_ask, 0),
        true,
        false,
        clause_ask,
    );
    assert!(!c_over.should_continue);

    // Trailing `?` stays terminal.
    let trailing_q = "Should I proceed with the remaining steps?";
    let t_over = apply_tracker_continuation_override(
        evaluate_interim_text_continuation(true, false, &history, trailing_q, 0),
        true,
        false,
        trailing_q,
    );
    assert!(!t_over.should_continue);

    // Verification-pending recaps are true handoffs — outer verification
    // recovery owns that path; in-turn tracker continuation must not race
    // past the anti-blind gate, even when full-auto already continued.
    for recap in [
        "Turn blocked after repeated unverified assistant responses; verification is still pending.",
        "## Status\nBlocked by wall-clock budget. Verification is still pending; run cargo check --locked.",
        "Anti-blind checkpoint: verification is still pending before further edits.",
    ] {
        let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
        let over = apply_tracker_continuation_override(base, true, false, recap);
        assert!(!over.should_continue, "must not continue past verification gate: {recap}");
    }

    // Session evidence: budget/status recaps without a gate still continue.
    // Full-auto may already continue some of these; the tracker override
    // must not reverse that. Assert `tracker_incomplete_continuation` only
    // when the base classifier would have stopped.
    for recap in [
        "## Status\nTask 7 blocked by the turn's preview budget before I could read docs.",
        "## Status\nWall-clock budget ran out after evidence gathering; next step is the patch.",
        "Task 3 mid-flight — hit the per-file read cap before editing.",
        "Fix 1 partially applied, needs verification; continuing with the remaining checks now.",
    ] {
        let base = evaluate_interim_text_continuation(true, false, &history, recap, 0);
        let over = apply_tracker_continuation_override(base, true, false, recap);
        assert!(over.should_continue, "must continue on session budget recap: {recap}");
        if !base.should_continue {
            assert_eq!(over.reason, "tracker_incomplete_continuation");
        }
    }

    // When the base classifier would end the turn, tracker override forces
    // non-relaxed continuation with the tracker reason.
    let status_only =
        "## Status\nBlocked by the turn's preview budget. Next step on resume: read docs/development/README.md.";
    let s_base = evaluate_interim_text_continuation(false, false, &history, status_only, 0);
    assert!(!s_base.should_continue, "interactive status recap must not auto-continue without tracker");
    let s_over = apply_tracker_continuation_override(s_base, true, false, status_only);
    assert!(s_over.should_continue);
    assert_eq!(s_over.reason, "tracker_incomplete_continuation");
    assert!(!s_over.is_relaxed_continuation);
}
#[test]
fn recoverable_status_recap_vocabulary_matches_outer_classifier() {
    use vtcode_core::core::agent::completion::recoverable_status_recap_phrasing;
    for phrase in [
        "blocked by the turn's preview budget",
        "wall-clock budget ran out",
        "per-file read cap",
        "preview budget exhausted",
        "tool loop budget exhausted",
        "recovery fallback",
        "reached the safety cap",
    ] {
        let lower = phrase.to_ascii_lowercase();
        assert!(recoverable_status_recap_phrasing(&lower), "shared vocab must cover {phrase}");
        assert!(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(
                Some(phrase)
            ),
            "outer classifier must cover {phrase}"
        );
    }
    for phrase in [
        "Turn blocked after repeated unverified assistant responses; verification is still pending.",
        "exec_command is denied by permission policy",
        "request_user_input is required",
    ] {
        assert!(
            !crate::agent::runloop::unified::turn::tool_outcomes::helpers::tracker_auto_continue_is_recoverable_block(
                Some(phrase)
            ),
            "outer classifier must deny {phrase}"
        );
    }
}
