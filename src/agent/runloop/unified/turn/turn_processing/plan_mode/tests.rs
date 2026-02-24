use super::*;
use crate::agent::runloop::unified::state::SessionStats;
use vtcode_core::config::constants::tools;

#[test]
fn maybe_force_plan_mode_interview_inserts_tool_call() {
    let mut stats = SessionStats::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "Proceeding without explicit questions.".to_string(),
        reasoning: Vec::new(),
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Proceeding without explicit questions."),
        &mut stats,
        1,
    );

    match result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            ..
        } => {
            assert_eq!(assistant_text, "Proceeding without explicit questions.");
            assert!(!tool_calls.is_empty());
            let name = tool_calls
                .last()
                .and_then(|call| call.function.as_ref())
                .map(|func| func.name.as_str())
                .unwrap_or("");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected tool calls with forced interview"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_includes_distinct_question_options() {
    let mut stats = SessionStats::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "Proceeding without explicit questions.".to_string(),
        reasoning: Vec::new(),
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Proceeding without explicit questions."),
        &mut stats,
        1,
    );

    let tool_calls = match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => tool_calls,
        _ => panic!("Expected tool calls with forced interview"),
    };

    let args = tool_calls
        .last()
        .and_then(|call| call.function.as_ref())
        .map(|func| func.arguments.as_str())
        .expect("expected interview tool arguments");
    let payload: serde_json::Value =
        serde_json::from_str(args).expect("interview args should be valid JSON");
    let questions = payload["questions"]
        .as_array()
        .expect("questions array should exist");
    assert_eq!(questions.len(), 3);

    let first_labels = questions
        .iter()
        .map(|question| {
            question["options"]
                .as_array()
                .expect("options array should exist")[0]["label"]
                .as_str()
                .expect("first option label should exist")
                .to_string()
        })
        .collect::<Vec<_>>();

    assert!(
        first_labels
            .iter()
            .all(|label| label.contains("(Recommended)"))
    );
    assert_ne!(first_labels[0], first_labels[1]);
    assert_ne!(first_labels[1], first_labels[2]);
    assert_ne!(first_labels[0], first_labels[2]);
}

#[test]
fn maybe_force_plan_mode_interview_skips_when_questions_present() {
    let mut stats = SessionStats::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "What should I do next?".to_string(),
        reasoning: Vec::new(),
        proposed_plan: None,
    };

    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("What should I do next?"),
        &mut stats,
        2,
    );

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "What should I do next?");
            assert!(!stats.plan_mode_interview_shown());
            assert!(!stats.plan_mode_interview_pending());
        }
        _ => panic!("Expected text response without forced interview"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_marks_shown_when_plan_present() {
    let mut stats = SessionStats::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
        reasoning: Vec::new(),
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        1,
    );

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            let name = tool_calls
                .last()
                .and_then(|call| call.function.as_ref())
                .map(|func| func.name.as_str())
                .unwrap_or("");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected tool calls for plan interview"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_appends_reminder_when_plan_ready() {
    let mut stats = SessionStats::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
        reasoning: Vec::new(),
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();
    stats.mark_plan_mode_interview_shown();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        2,
    );

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert!(text.contains(PLAN_MODE_REMINDER));
        }
        _ => panic!("Expected text response with plan reminder"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_does_not_duplicate_reminder() {
    let mut stats = SessionStats::default();
    let text = format!(
        "<proposed_plan>\nPlan content\n</proposed_plan>\n\n{}",
        PLAN_MODE_REMINDER
    );
    let processing_result = TurnProcessingResult::TextResponse {
        text: text.clone(),
        reasoning: Vec::new(),
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();
    stats.mark_plan_mode_interview_shown();

    let result = maybe_force_plan_mode_interview(processing_result, Some(&text), &mut stats, 3);

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text.matches(PLAN_MODE_REMINDER).count(), 1);
        }
        _ => panic!("Expected text response with single reminder"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_defers_when_tool_calls_present() {
    let mut stats = SessionStats::default();
    stats.increment_plan_mode_turns();
    stats.increment_plan_mode_turns();

    let processing_result = TurnProcessingResult::ToolCalls {
        tool_calls: vec![uni::ToolCall::function(
            "call_read".to_string(),
            tools::READ_FILE.to_string(),
            "{}".to_string(),
        )],
        assistant_text: String::new(),
        reasoning: Vec::new(),
    };

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Going to read files."),
        &mut stats,
        3,
    );

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            assert!(stats.plan_mode_interview_pending());
        }
        _ => panic!("Expected tool calls to continue without interview"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_strips_interview_from_mixed_tool_calls() {
    let mut stats = SessionStats::default();
    stats.increment_plan_mode_turns();
    stats.increment_plan_mode_turns();
    stats.increment_plan_mode_turns();

    let processing_result = TurnProcessingResult::ToolCalls {
        tool_calls: vec![
            uni::ToolCall::function(
                "call_read".to_string(),
                tools::READ_FILE.to_string(),
                "{}".to_string(),
            ),
            uni::ToolCall::function(
                "call_interview".to_string(),
                tools::REQUEST_USER_INPUT.to_string(),
                "{}".to_string(),
            ),
        ],
        assistant_text: String::new(),
        reasoning: Vec::new(),
    };

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Going to read files."),
        &mut stats,
        3,
    );

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            let name = tool_calls
                .first()
                .and_then(|call| call.function.as_ref())
                .map(|func| func.name.as_str())
                .unwrap_or("");
            assert_eq!(name, tools::READ_FILE);
            assert!(stats.plan_mode_interview_pending());
        }
        _ => panic!("Expected tool calls with interview stripped"),
    }
}
