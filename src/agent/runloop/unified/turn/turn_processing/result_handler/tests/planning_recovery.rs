use super::*;

#[tokio::test]
async fn planning_tokens_without_content_salvage_arms_one_final_content_retry() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;

    // The synthesis pass itself records completion tokens but no final
    // content (reasoning-interlaced end). Bounded salvage must arm instead of
    // parking the turn in a blocked handoff.
    let third = handle_planning_empty_with(
        &mut backing,
        NoOutputFacts { completion_tokens: 477, ..NoOutputFacts::default() },
        3,
        &mut repeated_tool_attempts,
        &mut turn_modified_files,
    )
    .await;
    assert!(matches!(third, TurnHandlerOutcome::Continue));
    assert!(
        backing
            .last_history_message_contains("recorded 477 completion tokens but returned no extractable final content"),
        "salvage directive must carry the captured token count, not an unverified emptiness claim"
    );
}
#[tokio::test]
async fn planning_third_tokens_without_content_handoff_states_captured_facts() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    for request in 1..=3 {
        let outcome = handle_planning_empty_with(
            &mut backing,
            NoOutputFacts { completion_tokens: 477, ..NoOutputFacts::default() },
            request + 2,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
        )
        .await;
        if request < 3 {
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        } else {
            assert!(
                matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) }) if reason == "planning awaiting input")
            );
        }
    }
    let notices = diagnostics.lock().expect("diagnostics");
    let notice = notices
        .iter()
        .find(|notice| notice.code == "planning.awaiting_input")
        .expect("canonical awaiting-input diagnostic");
    let details: serde_json::Value = serde_json::from_str(&notice.agent_details).expect("diagnostic details");
    assert_eq!(details["requests_used"], 3);
    assert_eq!(details["retained_draft"], false, "an empty response cannot create a saved draft");
    assert!(
        details["failure"]
            .as_str()
            .unwrap()
            .contains("provider recorded 477 completion tokens")
    );
    assert_eq!(notices.iter().filter(|notice| notice.code == "planning.awaiting_input").count(), 1);
    assert!(backing.last_history_message_contains("Planning needs your input. Saved progress is available."));
}
#[tokio::test]
async fn planning_true_no_output_handoff_never_claims_returned_tokens() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    for request in 1..=3 {
        let outcome = handle_planning_empty_with(
            &mut backing,
            NoOutputFacts::default(),
            request + 2,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
        )
        .await;
        if request < 3 {
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        } else {
            assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
        }
    }
    let notices = diagnostics.lock().expect("diagnostics");
    let notice = notices
        .iter()
        .find(|notice| notice.code == "planning.awaiting_input")
        .expect("canonical awaiting-input diagnostic");
    let details: serde_json::Value = serde_json::from_str(&notice.agent_details).expect("diagnostic details");
    assert_eq!(details["category"], "empty_content");
    assert!(
        details["failure"]
            .as_str()
            .unwrap()
            .contains("the model returned an empty synthesis response")
    );
    assert!(!details["failure"].as_str().unwrap().contains("completion tokens"));
    assert!(!backing.last_history_message_contains("completion tokens"));
}
#[tokio::test]
async fn planning_dropped_tool_calls_report_the_actual_failure_shape() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    backing.mark_interview_denied_for_test();
    let diagnostics = capture_planning_recovery_diagnostics(&mut backing);
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    advance_planning_recovery_to_tool_free_pass(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files)
        .await;
    for request in 1..=3 {
        let facts = NoOutputFacts {
            dropped_tool_calls: true,
            reasoning_only: true,
            completion_tokens: 210,
        };
        let outcome = handle_planning_empty_with(
            &mut backing,
            facts,
            request + 2,
            &mut repeated_tool_attempts,
            &mut turn_modified_files,
        )
        .await;
        if request < 3 {
            assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        } else {
            assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
        }
    }
    let notices = diagnostics.lock().expect("diagnostics");
    let notice = notices
        .iter()
        .find(|notice| notice.code == "planning.awaiting_input")
        .expect("canonical awaiting-input diagnostic");
    let details: serde_json::Value = serde_json::from_str(&notice.agent_details).expect("diagnostic details");
    assert_eq!(details["category"], "native_tool_calls");
    assert!(
        details["failure"]
            .as_str()
            .unwrap()
            .contains("attempted native tool calls on a tool-free pass")
    );
    assert!(!details["failure"].as_str().unwrap().contains("empty synthesis response"));
    assert!(!notice.agent_details.contains("private planning reasoning"));
    assert!(turn_modified_files.is_empty());
}
