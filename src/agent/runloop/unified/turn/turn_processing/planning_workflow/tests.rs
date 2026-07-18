use super::interview_context::line_has_open_decision_marker;
use super::*;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::state::SessionStats;
use vtcode_core::config::constants::tools;

fn tool_calls_result(tool_calls: Vec<uni::ToolCall>, assistant_text: impl Into<String>) -> TurnProcessingResult {
    TurnProcessingResult::ToolCalls {
        tool_calls: prepare_tool_calls(tool_calls),
        assistant_text: assistant_text.into(),
        reasoning: Vec::new(),
        reasoning_details: None,
    }
}

#[test]
fn maybe_force_planning_workflow_interview_inserts_tool_call() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "Proceeding without explicit questions.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Proceeding without explicit questions."),
        &mut stats,
        &mut plan_session,
        1,
    );
    match result {
        TurnProcessingResult::ToolCalls { tool_calls, assistant_text, .. } => {
            assert_eq!(assistant_text, "Proceeding without explicit questions.");
            assert!(!tool_calls.is_empty());
            let name = tool_calls.last().map(|call| call.tool_name()).unwrap_or("");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected tool calls with forced interview"),
    }
}

#[test]
fn public_discovery_tools_make_planning_interview_ready() {
    for tool in [tools::EXEC_COMMAND, tools::CODE_SEARCH] {
        let mut stats = SessionStats::default();
        let mut plan_session = PlanningWorkflowSessionState::default();
        stats.record_tool(tool);
        plan_session.increment_turns();

        assert!(
            planning_workflow_interview_ready(&stats, &plan_session),
            "{tool} should count as planning discovery"
        );
    }
}

#[test]
fn maybe_force_planning_workflow_interview_includes_distinct_question_options() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "Proceeding without explicit questions.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Proceeding without explicit questions."),
        &mut stats,
        &mut plan_session,
        1,
    );
    let tool_calls = match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => tool_calls,
        _ => panic!("Expected tool calls with forced interview"),
    };

    let args = tool_calls
        .last()
        .and_then(|call| call.args())
        .expect("expected interview tool arguments");
    let payload = args.clone();
    let questions = payload["questions"].as_array().expect("questions array should exist");
    assert!(!questions.is_empty());

    let first_labels = questions
        .iter()
        .map(|question| {
            question["options"].as_array().expect("options array should exist")[0]["label"]
                .as_str()
                .expect("first option label should exist")
                .to_string()
        })
        .collect::<Vec<_>>();

    assert!(first_labels.iter().all(|label| label.contains("(Recommended)")));
    let unique_labels = first_labels.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(unique_labels.len(), first_labels.len());
}

#[test]
fn maybe_force_planning_workflow_interview_skips_when_questions_present() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "What should I do next?".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    plan_session.increment_turns();

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("What should I do next?"),
        &mut stats,
        &mut plan_session,
        2,
    );
    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "What should I do next?");
            assert!(!plan_session.interview_shown());
            assert!(!plan_session.interview_pending());
        }
        _ => panic!("Expected text response without forced interview"),
    }
}

#[test]
fn maybe_force_planning_workflow_interview_marks_shown_when_plan_present() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        &mut plan_session,
        1,
    );
    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            let name = tool_calls.last().map(|call| call.tool_name()).unwrap_or("");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected tool calls for plan interview"),
    }
}

#[test]
fn maybe_force_planning_workflow_interview_does_not_duplicate_existing_interview_when_plan_present() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();

    let processing_result = tool_calls_result(
        vec![uni::ToolCall::function(
            "call_interview_existing".to_string(),
            tools::REQUEST_USER_INPUT.to_string(),
            "{}".to_string(),
        )],
        "<proposed_plan>\nPlan content\n</proposed_plan>",
    );

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        &mut plan_session,
        7,
    );
    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            let interview_calls = tool_calls
                .iter()
                .filter(|call| call.tool_name() == tools::REQUEST_USER_INPUT)
                .count();
            assert_eq!(interview_calls, 1);
        }
        _ => panic!("Expected tool calls with single forced interview"),
    }
}

#[test]
fn maybe_force_planning_workflow_interview_appends_reminder_when_plan_ready() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();
    plan_session.record_interview_result(2, false);

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        &mut plan_session,
        2,
    );
    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert!(text.contains(PLANNING_WORKFLOW_REMINDER));
        }
        _ => panic!("Expected text response with plan reminder"),
    }
}

#[test]
fn planning_workflow_reminder_includes_manual_switch_fallback() {
    assert!(PLANNING_WORKFLOW_REMINDER.contains("finish_planning"));
    assert!(!PLANNING_WORKFLOW_REMINDER.contains(&format!("/{}", "mode")));
    assert!(PLANNING_WORKFLOW_REMINDER.contains("implement"));
}

#[test]
fn maybe_force_planning_workflow_interview_does_not_duplicate_reminder() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let text = format!("<proposed_plan>\nPlan content\n</proposed_plan>\n\n{PLANNING_WORKFLOW_REMINDER}");
    let processing_result = TurnProcessingResult::TextResponse {
        text: text.clone(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();
    plan_session.record_interview_result(2, false);

    let result =
        maybe_force_planning_workflow_interview(processing_result, Some(&text), &mut stats, &mut plan_session, 3);
    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text.matches(PLANNING_WORKFLOW_REMINDER).count(), 1);
        }
        _ => panic!("Expected text response with single reminder"),
    }
}

#[test]
fn maybe_force_planning_workflow_interview_defers_when_tool_calls_present() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.increment_turns();
    plan_session.increment_turns();

    let processing_result = tool_calls_result(
        vec![uni::ToolCall::function(
            "call_read".to_string(),
            tools::READ_FILE.to_string(),
            "{}".to_string(),
        )],
        String::new(),
    );

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Going to read files."),
        &mut stats,
        &mut plan_session,
        3,
    );
    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            assert!(plan_session.interview_pending());
        }
        _ => panic!("Expected tool calls to continue without interview"),
    }
}

#[test]
fn inject_planning_workflow_interview_preserves_existing_tool_call_when_appending() {
    let mut plan_session = PlanningWorkflowSessionState::default();
    let processing_result = tool_calls_result(
        vec![uni::ToolCall::function(
            "call_read".to_string(),
            tools::READ_FILE.to_string(),
            serde_json::json!({"path":"src/main.rs"}).to_string(),
        )],
        "Reading first.",
    );

    let result = inject_planning_workflow_interview(processing_result, &mut plan_session, 2);

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 2);
            assert_eq!(tool_calls[0].call_id(), "call_read");
            assert_eq!(tool_calls[0].tool_name(), tools::READ_FILE);
            assert_eq!(tool_calls[1].tool_name(), tools::REQUEST_USER_INPUT);
            assert!(plan_session.interview_shown());
        }
        _ => panic!("Expected tool calls with appended interview"),
    }
}

#[test]
fn maybe_force_planning_workflow_interview_strips_interview_from_mixed_tool_calls() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    plan_session.increment_turns();
    plan_session.increment_turns();
    plan_session.increment_turns();

    let processing_result = tool_calls_result(
        vec![
            uni::ToolCall::function("call_read".to_string(), tools::READ_FILE.to_string(), "{}".to_string()),
            uni::ToolCall::function(
                "call_interview".to_string(),
                tools::REQUEST_USER_INPUT.to_string(),
                "{}".to_string(),
            ),
        ],
        String::new(),
    );

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Going to read files."),
        &mut stats,
        &mut plan_session,
        3,
    );
    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            let name = tool_calls.first().map(|call| call.tool_name()).unwrap_or("");
            assert_eq!(name, tools::READ_FILE);
            assert!(plan_session.interview_pending());
        }
        _ => panic!("Expected tool calls with interview stripped"),
    }
}

#[test]
fn maybe_force_planning_workflow_interview_reopens_when_open_decision_markers_exist() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();
    plan_session.record_interview_result(1, false);

    let processing_result = TurnProcessingResult::TextResponse {
        text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>\n\n- Next open decision: TBD"),
        &mut stats,
        &mut plan_session,
        4,
    );
    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            let name = tool_calls.last().map(|call| call.tool_name()).unwrap_or("");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected follow-up interview tool call"),
    }
}

#[test]
fn maybe_force_planning_workflow_interview_clears_stale_pending_when_decisions_closed() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();
    plan_session.record_interview_result(2, false);
    plan_session.mark_interview_pending();

    let processing_result = TurnProcessingResult::TextResponse {
        text: "Decisions are closed and we can proceed.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Decisions are closed and we can proceed."),
        &mut stats,
        &mut plan_session,
        5,
    );
    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "Decisions are closed and we can proceed.");
            assert!(!plan_session.interview_pending());
        }
        _ => panic!("Expected text response without extra interview"),
    }
}

#[test]
fn line_has_open_decision_marker_only_tracks_next_open_decision() {
    assert!(line_has_open_decision_marker("Next open decision: validate migration order"));
    assert!(!line_has_open_decision_marker("Decision needed: choose validation scope"));
    assert!(!line_has_open_decision_marker("Next open decision: none"));
    assert!(!line_has_open_decision_marker("Next open decision: No remaining scope decisions."));
}

#[test]
fn maybe_force_planning_workflow_interview_reprompts_after_cancelled_cycle() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();
    plan_session.record_interview_result(0, true);

    let processing_result = TurnProcessingResult::TextResponse {
        text: "Continuing planning.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Continuing planning."),
        &mut stats,
        &mut plan_session,
        6,
    );
    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            let name = tool_calls.last().map(|call| call.tool_name()).unwrap_or("");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected interview to re-open after cancellation"),
    }
}

/// Regression test for checkpoint turn_655/turn_660: unlike a user cancelling
/// the interview modal (which re-opens it, see the test above), a permanent
/// policy/capability denial must never re-inject `request_user_input` — doing
/// so just repeats the same denial every turn until an unrelated circuit
/// breaker (tool wall-clock budget) trips.
#[test]
fn maybe_force_planning_workflow_interview_never_reinjects_after_denial() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();

    // Simulate the failure_path.rs handling of a policy-denied
    // `request_user_input` tool execution.
    plan_session.mark_interview_denied();

    assert!(!planning_workflow_interview_ready(&stats, &plan_session));

    let processing_result = TurnProcessingResult::TextResponse {
        text: "Continuing planning.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Continuing planning."),
        &mut stats,
        &mut plan_session,
        6,
    );
    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "Continuing planning.");
        }
        _ => panic!("Expected no interview tool call after denial"),
    }
}
