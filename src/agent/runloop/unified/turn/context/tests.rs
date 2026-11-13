use super::*;

#[test]
fn suppresses_redundant_diff_recap_after_git_diff_view_request() {
    let history = vec![
        uni::Message::user("show diff src/main.rs".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            r#"{"content_type":"git_diff","command":"git diff -- src/main.rs","output":"diff --git a/src/main.rs b/src/main.rs"}"#.to_string(),
        ),
    ];

    assert!(should_suppress_redundant_diff_recap(&history, "Diff for src/main.rs:\n```diff\n@@ -1 +1 @@\n```"));
}

#[test]
fn does_not_suppress_diff_recap_when_user_asked_for_analysis() {
    let history = vec![
        uni::Message::user("analyze this diff and explain".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            r#"{"content_type":"git_diff","command":"git diff -- src/main.rs"}"#.to_string(),
        ),
    ];

    assert!(!should_suppress_redundant_diff_recap(&history, "The diff shows one behavior change."));
}

#[test]
fn suppresses_heading_style_diff_recap_after_view_request() {
    let history = vec![
        uni::Message::user("show diff on vtcode-tui/src/ui/markdown.rs".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            r#"{"content_type":"git_diff","command":"git diff -- vtcode-tui/src/ui/markdown.rs","output":"diff --git a/vtcode-tui/src/ui/markdown.rs b/vtcode-tui/src/ui/markdown.rs\n@@ -1 +1 @@\n- old\n+ new"}"#.to_string(),
        ),
    ];

    assert!(should_suppress_redundant_diff_recap(
        &history,
        "Implemented updated syntax highlighting for diff previews.\n\n**Diff preview changes**\n\n```\n@@\n- old\n+ new\n```\n"
    ));
}

#[test]
fn parse_reasoning_detail_value_decodes_stringified_json_object() {
    let parsed = parse_reasoning_detail_value(r#"{"type":"reasoning.text","id":"r1","text":"hello"}"#);
    assert!(parsed.is_object());
    assert_eq!(parsed["type"], "reasoning.text");
}

#[test]
fn build_combined_reasoning_falls_back_to_detail_text() {
    let combined = build_combined_reasoning(&[], Some("detail trace"));
    assert_eq!(combined.as_deref(), Some("detail trace"));
}

#[test]
fn build_combined_reasoning_preserves_whitespace_only_segments_without_detail() {
    let combined = build_combined_reasoning(&[ReasoningSegment::new("  ", None)], None);
    assert_eq!(combined.as_deref(), Some("  "));
}

#[test]
fn push_assistant_message_preserves_reasoning_details_when_merging() {
    let mut history = vec![uni::Message::assistant("old".to_string())];
    let new_msg = uni::Message::assistant("new".to_string())
        .with_reasoning_details(Some(vec![serde_json::json!({"type":"reasoning.text","text":"trace"})]));

    push_assistant_message(&mut history, new_msg);

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_text(), "new");
    assert_eq!(
        history[0].reasoning_details,
        Some(vec![serde_json::json!({"type":"reasoning.text","text":"trace"})])
    );
}

#[test]
fn push_assistant_message_keeps_different_phases_separate() {
    let mut history =
        vec![uni::Message::assistant("working".to_string()).with_phase(Some(uni::AssistantPhase::Commentary))];
    let new_msg = uni::Message::assistant("done".to_string()).with_phase(Some(uni::AssistantPhase::FinalAnswer));

    push_assistant_message(&mut history, new_msg);

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].phase, Some(uni::AssistantPhase::Commentary));
    assert_eq!(history[1].phase, Some(uni::AssistantPhase::FinalAnswer));
}

#[test]
fn interim_continuation_handles_multibyte_clause_boundaries() {
    let text = "hello! what are we ducking through today—debugging, design, scope clarification, or planning a change?";

    let decision = evaluate_interim_text_continuation(false, false, &[], text, 0);

    assert!(!decision.should_continue);
    assert_eq!(decision.reason, "non_interim_text");
}

#[test]
fn clarifying_question_detected_when_last_line_ends_with_question_mark() {
    assert!(looks_like_clarifying_question(
        "I've reviewed the codebase. Should I implement this in a single unified runloop branch?"
    ));
}

#[test]
fn clarifying_question_detected_with_trailing_whitespace() {
    assert!(looks_like_clarifying_question("Which approach do you prefer?   \n\n"));
}

#[test]
fn clarifying_question_detected_with_preamble_and_blank_lines() {
    assert!(looks_like_clarifying_question(
        "I analyzed the module.\n\nNext open decision: Do you want a unified branch or a reviewable exec-plan document?\n"
    ));
}

#[test]
fn clarifying_question_not_detected_for_plan_ending_with_prose() {
    assert!(!looks_like_clarifying_question(
        "Summary: fix the parser\n1. Action -> file.rs -> verify: tests pass\n\nAssumptions: none"
    ));
}

#[test]
fn clarifying_question_not_detected_for_empty_text() {
    assert!(!looks_like_clarifying_question(""));
    assert!(!looks_like_clarifying_question("   \n\n  "));
}

#[test]
fn clarifying_question_not_detected_for_rhetorical_question_in_middle() {
    assert!(!looks_like_clarifying_question("Why is this needed? Because X. Step 1: Do Y. Step 2: Do Z."));
}

#[test]
fn clarifying_question_detected_for_exact_checkpoint_turn_856_phrase() {
    assert!(looks_like_clarifying_question(
        "Next open decision: Do you want me to implement this in a single unified runloop branch, or first create a reviewable exec-plan document under docs/harness/exec-plans/?"
    ));
}

#[test]
fn prepared_tool_call_maps_shell_aliases_to_exec_command() {
    let call = uni::ToolCall::function(
        "call_test".to_string(),
        "bash".to_string(),
        r#"{"command": ["echo", "hi"], "action": "run"}"#.to_string(),
    );
    let prepared = PreparedAssistantToolCall::new(call);
    assert_eq!(prepared.tool_name(), "exec_command");
    assert!(prepared.args_error().is_none() || prepared.args().is_some());
}

#[test]
fn prepared_tool_call_rejects_prose_blob_names() {
    let call =
        uni::ToolCall::function("call_test".to_string(), "` in content — could a skill".to_string(), "{}".to_string());
    let prepared = PreparedAssistantToolCall::new(call);
    assert!(prepared.args().is_none() || prepared.args_error().is_some());
}

#[test]
fn builtin_tool_names_all_pass_dispatchability_gate() {
    // Every builtin tool name reachable through native tool calls must pass
    // `is_dispatchable_tool_name`; otherwise the gate in
    // `PreparedAssistantToolCall::new` would reject legitimate calls with
    // "tool name is not a clean identifier". The constants module
    // compile-time-validates [a-z0-9_] shape, so this guards the gate against
    // future gate tightening that outlaws a shipped name.
    use vtcode_config::constants::tools;

    let names = [
        tools::EXEC_COMMAND,
        tools::WRITE_STDIN,
        tools::APPLY_PATCH,
        tools::CODE_SEARCH,
        tools::UNIFIED_SEARCH,
        tools::UNIFIED_EXEC,
        tools::UNIFIED_FILE,
        tools::THINK,
        tools::SEARCH_TOOLS,
        tools::MCP,
        tools::MCP_SEARCH_TOOLS,
        tools::MCP_GET_TOOL_DETAILS,
        tools::MCP_LIST_SERVERS,
        tools::MCP_CONNECT_SERVER,
        tools::MCP_DISCONNECT_SERVER,
        tools::WEB_SEARCH,
        tools::WEB_FETCH,
        tools::FETCH_URL,
        tools::LIST,
        tools::GREP,
        tools::FETCH,
        tools::EXEC_PTY_CMD,
        tools::SHELL,
        tools::GREP_FILE,
        tools::LIST_FILES,
        tools::LIST_SKILLS,
        tools::LOAD_SKILL,
        tools::LOAD_SKILL_RESOURCE,
        tools::RUN_PTY_CMD,
        tools::CREATE_PTY_SESSION,
        tools::LIST_PTY_SESSIONS,
        tools::CLOSE_PTY_SESSION,
        tools::SEND_PTY_INPUT,
        tools::READ_PTY_SESSION,
        tools::RESIZE_PTY_SESSION,
        tools::EXECUTE_CODE,
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::EDIT_FILE,
        tools::DELETE_FILE,
        tools::CREATE_FILE,
        tools::SEARCH_REPLACE,
        tools::FILE_OP,
        tools::MOVE_FILE,
        tools::COPY_FILE,
        tools::GET_ERRORS,
        tools::REQUEST_USER_INPUT,
        tools::MEMORY,
        tools::ASK_QUESTIONS,
        tools::ASK_USER_QUESTION,
        tools::AGENT,
        tools::CRON,
        tools::CRON_CREATE,
        tools::CRON_LIST,
        tools::CRON_DELETE,
        tools::START_PLANNING,
        tools::TASK_TRACKER,
        tools::SPAWN_AGENT,
        tools::SPAWN_BACKGROUND_SUBPROCESS,
        tools::SEND_INPUT,
        tools::WAIT_AGENT,
        tools::RESUME_AGENT,
        tools::CLOSE_AGENT,
    ];
    for name in names {
        assert!(
            crate::agent::runloop::text_tools::is_dispatchable_tool_name(name),
            "builtin tool name must pass the dispatchability gate: {name}"
        );
    }
}
