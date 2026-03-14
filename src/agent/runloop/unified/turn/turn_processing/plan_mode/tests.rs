use super::*;
use crate::agent::runloop::unified::state::SessionStats;
use vtcode_core::config::constants::tools;

fn tool_calls_result(
    tool_calls: Vec<uni::ToolCall>,
    assistant_text: impl Into<String>,
) -> TurnProcessingResult {
    TurnProcessingResult::ToolCalls {
        tool_calls: prepare_tool_calls(tool_calls),
        assistant_text: assistant_text.into(),
        reasoning: Vec::new(),
        reasoning_details: None,
    }
}

#[test]
fn maybe_force_plan_mode_interview_inserts_tool_call() {
    let mut stats = SessionStats::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "Proceeding without explicit questions.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Proceeding without explicit questions."),
        &mut stats,
        1,
        None,
    );

    match result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            ..
        } => {
            assert_eq!(assistant_text, "Proceeding without explicit questions.");
            assert!(!tool_calls.is_empty());
            let name = tool_calls.last().map(|call| call.tool_name()).unwrap_or("");
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
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Proceeding without explicit questions."),
        &mut stats,
        1,
        None,
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
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("What should I do next?"),
        &mut stats,
        2,
        None,
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
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        1,
        None,
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
fn maybe_force_plan_mode_interview_does_not_duplicate_existing_interview_when_plan_present() {
    let mut stats = SessionStats::default();
    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let processing_result = tool_calls_result(
        vec![uni::ToolCall::function(
            "call_interview_existing".to_string(),
            tools::REQUEST_USER_INPUT.to_string(),
            "{}".to_string(),
        )],
        "<proposed_plan>\nPlan content\n</proposed_plan>",
    );

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        7,
        None,
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
fn maybe_force_plan_mode_interview_appends_reminder_when_plan_ready() {
    let mut stats = SessionStats::default();
    let processing_result = TurnProcessingResult::TextResponse {
        text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();
    stats.record_plan_mode_interview_result(2, false);

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
        &mut stats,
        2,
        None,
    );

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert!(text.contains(PLAN_MODE_REMINDER));
        }
        _ => panic!("Expected text response with plan reminder"),
    }
}

#[test]
fn plan_mode_reminder_includes_manual_switch_fallback() {
    assert!(PLAN_MODE_REMINDER.contains("/plan off"));
    assert!(PLAN_MODE_REMINDER.contains("/mode"));
    assert!(PLAN_MODE_REMINDER.contains("Shift+Tab"));
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
        reasoning_details: None,
        proposed_plan: None,
    };

    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();
    stats.record_plan_mode_interview_result(2, false);

    let result =
        maybe_force_plan_mode_interview(processing_result, Some(&text), &mut stats, 3, None);

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

    let processing_result = tool_calls_result(
        vec![uni::ToolCall::function(
            "call_read".to_string(),
            tools::READ_FILE.to_string(),
            "{}".to_string(),
        )],
        String::new(),
    );

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Going to read files."),
        &mut stats,
        3,
        None,
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
fn inject_plan_mode_interview_preserves_existing_tool_call_when_appending() {
    let mut stats = SessionStats::default();
    let processing_result = tool_calls_result(
        vec![uni::ToolCall::function(
            "call_read".to_string(),
            tools::READ_FILE.to_string(),
            serde_json::json!({"path":"src/main.rs"}).to_string(),
        )],
        "Reading first.",
    );

    let result = inject_plan_mode_interview(
        processing_result,
        &mut stats,
        2,
        Some("Reading first."),
        None,
    );

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 2);
            assert_eq!(tool_calls[0].call_id(), "call_read");
            assert_eq!(tool_calls[0].tool_name(), tools::READ_FILE);
            assert_eq!(tool_calls[1].tool_name(), tools::REQUEST_USER_INPUT);
            assert!(stats.plan_mode_interview_shown());
        }
        _ => panic!("Expected tool calls with appended interview"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_strips_interview_from_mixed_tool_calls() {
    let mut stats = SessionStats::default();
    stats.increment_plan_mode_turns();
    stats.increment_plan_mode_turns();
    stats.increment_plan_mode_turns();

    let processing_result = tool_calls_result(
        vec![
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
        String::new(),
    );

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Going to read files."),
        &mut stats,
        3,
        None,
    );

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            let name = tool_calls
                .first()
                .map(|call| call.tool_name())
                .unwrap_or("");
            assert_eq!(name, tools::READ_FILE);
            assert!(stats.plan_mode_interview_pending());
        }
        _ => panic!("Expected tool calls with interview stripped"),
    }
}

#[test]
fn maybe_force_plan_mode_interview_reopens_when_open_decision_markers_exist() {
    let mut stats = SessionStats::default();
    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();
    stats.record_plan_mode_interview_result(1, false);

    let processing_result = TurnProcessingResult::TextResponse {
        text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("<proposed_plan>\nPlan content\n</proposed_plan>\n\n- Next open decision: TBD"),
        &mut stats,
        4,
        None,
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
fn maybe_force_plan_mode_interview_clears_stale_pending_when_decisions_closed() {
    let mut stats = SessionStats::default();
    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();
    stats.record_plan_mode_interview_result(2, false);
    stats.mark_plan_mode_interview_pending();

    let processing_result = TurnProcessingResult::TextResponse {
        text: "Decisions are closed and we can proceed.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Decisions are closed and we can proceed."),
        &mut stats,
        5,
        None,
    );

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "Decisions are closed and we can proceed.");
            assert!(!stats.plan_mode_interview_pending());
        }
        _ => panic!("Expected text response without extra interview"),
    }
}

#[test]
fn should_attempt_dynamic_interview_generation_skips_when_interview_already_called() {
    let mut stats = SessionStats::default();
    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();

    let processing_result = tool_calls_result(
        vec![uni::ToolCall::function(
            "call_interview".to_string(),
            tools::REQUEST_USER_INPUT.to_string(),
            "{}".to_string(),
        )],
        String::new(),
    );

    assert!(!should_attempt_dynamic_interview_generation(
        &processing_result,
        Some("Need clarification."),
        &stats,
    ));
}

#[test]
fn collect_interview_research_context_includes_custom_note_policy() {
    let stats = SessionStats::default();
    let context = collect_interview_research_context(
        &[uni::Message::assistant(
            "Decision needed: choose validation scope".to_string(),
        )],
        Some("- Next open decision: TBD"),
        &stats,
    );

    assert!(
        context
            .custom_note_policy
            .to_ascii_lowercase()
            .contains("custom")
    );
    assert!(
        context
            .custom_note_policy
            .to_ascii_lowercase()
            .contains("free-form")
    );
}

#[test]
fn line_has_open_decision_marker_only_tracks_next_open_decision() {
    assert!(line_has_open_decision_marker(
        "Next open decision: validate migration order"
    ));
    assert!(!line_has_open_decision_marker(
        "Decision needed: choose validation scope"
    ));
    assert!(!line_has_open_decision_marker("Next open decision: none"));
    assert!(!line_has_open_decision_marker(
        "Next open decision: No remaining scope decisions."
    ));
}

#[test]
fn collect_interview_research_context_extracts_recent_paths_and_symbols() {
    let stats = SessionStats::default();
    let context = collect_interview_research_context(
        &[uni::Message::assistant(
            "Focus files:\n- src/agent/runloop/unified/turn/turn_loop.rs\n- SessionStats::record_plan_mode_interview_result".to_string(),
        )],
        None,
        &stats,
    );

    assert!(
        context
            .recent_targets
            .iter()
            .any(|line| line.contains("turn_loop.rs"))
    );
    assert!(
        context
            .recent_targets
            .iter()
            .any(|line| line.contains("SessionStats::record_plan_mode_interview_result"))
    );
}

#[test]
fn maybe_force_plan_mode_interview_reprompts_after_cancelled_cycle() {
    let mut stats = SessionStats::default();
    stats.record_tool(tools::READ_FILE);
    stats.increment_plan_mode_turns();
    stats.record_plan_mode_interview_result(0, true);

    let processing_result = TurnProcessingResult::TextResponse {
        text: "Continuing planning.".to_string(),
        reasoning: Vec::new(),
        reasoning_details: None,
        proposed_plan: None,
    };

    let result = maybe_force_plan_mode_interview(
        processing_result,
        Some("Continuing planning."),
        &mut stats,
        6,
        None,
    );

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            let name = tool_calls.last().map(|call| call.tool_name()).unwrap_or("");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected interview to re-open after cancellation"),
    }
}
