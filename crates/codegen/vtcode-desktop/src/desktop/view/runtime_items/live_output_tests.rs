use super::{ToolCategory, activity_capture_output, bounded_output_chunk, describe_activity};
use vtcode_exec_events::{
    CommandExecutionItem, CommandExecutionStatus, ThreadItemDetails, ToolCallStatus, ToolOutputItem, WebSearchItem,
};

#[test]
fn tracker_capture_hides_structured_field_list_but_keeps_message() {
    let raw = "Structured result with fields: status, message, checklist\nChecklist updated";
    assert_eq!(activity_capture_output(raw, Some(ToolCategory::Tracker)).as_deref(), Some("Checklist updated"));
    assert_eq!(activity_capture_output(raw, Some(ToolCategory::Run)).as_deref(), Some(raw));
    assert_eq!(activity_capture_output("Structured result with fields: status", Some(ToolCategory::Tracker)), None);
}

#[test]
fn live_output_chunks_keep_order_and_bound_utf8() {
    let first = bounded_output_chunk("", "first\n");
    assert_eq!(bounded_output_chunk(&first, "second"), "first\nsecond");
    assert_eq!(bounded_output_chunk("other", ""), "other");
    let large = format!("prefix{}end", "界".repeat(30_000));
    let tail = bounded_output_chunk("old", &large);
    assert!(tail.len() <= 64 * 1024);
    assert!(tail.ends_with("界end"));
    assert!(!tail.contains("prefix"));
}

#[test]
fn failed_tool_output_and_command_report_exit_code_consistently() {
    let output = ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
        call_id: "call".into(),
        tool_call_id: None,
        spool_path: None,
        output: String::new(),
        is_delta: None,
        exit_code: Some(17),
        status: ToolCallStatus::Failed,
        app_previews: Vec::new(),
    }));
    let command = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
        command: "false".into(),
        arguments: None,
        aggregated_output: String::new(),
        exit_code: Some(17),
        status: CommandExecutionStatus::Failed,
    }));
    assert_eq!(describe_activity(&output, None).1, "failed (exit 17)");
    assert_eq!(describe_activity(&command, None).1, "failed (exit 17)");
    let ThreadItemDetails::ToolOutput(mut output) = output else {
        panic!("expected tool output")
    };
    output.status = ToolCallStatus::Completed;
    output.exit_code = Some(0);
    assert_eq!(describe_activity(&ThreadItemDetails::ToolOutput(output), None).1, "completed");
}

#[test]
fn blank_commands_fall_back_to_cli_placeholder_title() {
    for blank in ["", "   "] {
        let command = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: blank.into(),
            arguments: None,
            aggregated_output: String::new(),
            exit_code: None,
            status: CommandExecutionStatus::Completed,
        }));
        assert_eq!(describe_activity(&command, None).0, "Ran command");
    }
}

#[test]
fn multi_line_queries_collapse_to_single_line_titles() {
    // Titles render in a single-line slot: a multi-line query collapses
    // to one line instead of overflowing its row with a bare middle line
    // and ghosted neighbors (the 2026-11-09 session failure). Commands
    // preview to one line upstream (`gist_previews_multiline_commands`);
    // this pins the collapse for sources that carry raw text.
    let search = ThreadItemDetails::WebSearch(Box::new(WebSearchItem {
        query: "first line\n\tsecond line".into(),
        provider: None,
        results: None,
    }));
    assert_eq!(describe_activity(&search, None).0, "first line second line");
}

#[test]
fn completed_commands_preview_like_invocation_gists() {
    // A pending row and its completed row title identically: multi-line
    // commands preview to the first line on both paths.
    let execution = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
        command: "sed -n '1p' README.md\necho done".into(),
        arguments: None,
        aggregated_output: String::new(),
        exit_code: Some(0),
        status: CommandExecutionStatus::Completed,
    }));
    assert_eq!(describe_activity(&execution, None).0, "Ran sed README.md");
}
