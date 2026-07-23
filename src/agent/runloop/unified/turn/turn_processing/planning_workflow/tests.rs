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
fn maybe_force_planning_workflow_interview_passes_text_response() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "Proceeding with planning research.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    plan_session.increment_turns();

    let result = maybe_force_planning_workflow_interview(
        processing_result,
        Some("Proceeding with planning research."),
        &mut stats,
        &mut plan_session,
        1,
    );
    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "Proceeding with planning research.");
        }
        _ => panic!("Expected text response without forced interview tool call"),
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
fn maybe_force_planning_workflow_interview_strips_request_user_input_calls() {
    let mut stats = SessionStats::default();
    let mut plan_session = PlanningWorkflowSessionState::default();
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
        }
        _ => panic!("Expected tool calls with request_user_input stripped"),
    }
}

#[test]
fn line_has_open_decision_marker_only_tracks_next_open_decision() {
    assert!(line_has_open_decision_marker("Next open decision: validate migration order"));
    assert!(!line_has_open_decision_marker("Decision needed: choose validation scope"));
    assert!(!line_has_open_decision_marker("Next open decision: none"));
    assert!(!line_has_open_decision_marker("Next open decision: No remaining scope decisions."));
}
