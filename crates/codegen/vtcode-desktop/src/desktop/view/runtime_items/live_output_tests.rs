use super::{bounded_output_chunk, describe_activity};
use vtcode_exec_events::{
    CommandExecutionItem, CommandExecutionStatus, HarnessEventItem, HarnessEventKind, ThreadItemDetails,
    ToolCallStatus, ToolOutputItem, WebSearchItem,
};

// The legacy "Structured result with fields:" strip lives in the shared
// human projection (`output_sections::human_output_text`, pinned by
// `human_output_drops_legacy_structured_result_first_line`), so every tool
// row — tracker or otherwise — hides the line without a category gate here.

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
fn harness_rows_title_quietly_like_command_rows() {
    // Harness verification commands title like execution rows (`Ran <program>
    // <file>`), never as the raw shell expression; artifact paths relativize
    // to the workspace; a bare event falls back to the kind label.
    let command = ThreadItemDetails::Harness(Box::new(HarnessEventItem {
        command: Some("sed -n '1,20p' src/main.rs | head -c 99".into()),
        ..HarnessEventItem::new(HarnessEventKind::TurnBlocked)
    }));
    assert_eq!(describe_activity(&command, None).0, "Ran sed main.rs");

    let workspace = std::path::Path::new("/work/vtcode");
    let path = ThreadItemDetails::Harness(Box::new(HarnessEventItem {
        path: Some("/work/vtcode/.vtcode/handoff.md".into()),
        ..HarnessEventItem::new(HarnessEventKind::BlockedHandoffWritten)
    }));
    assert_eq!(describe_activity(&path, Some(workspace)).0, ".vtcode/handoff.md");

    let bare = ThreadItemDetails::Harness(Box::new(HarnessEventItem::new(HarnessEventKind::TurnBlocked)));
    assert_eq!(describe_activity(&bare, None).0, "Harness");

    // Asymmetric: a command wins over a path, and a blank command falls
    // through to the path instead of titling the row empty.
    let both = ThreadItemDetails::Harness(Box::new(HarnessEventItem {
        command: Some("cargo check --locked".into()),
        path: Some("target/debug/vtcode".into()),
        ..HarnessEventItem::new(HarnessEventKind::TurnBlocked)
    }));
    assert_eq!(describe_activity(&both, None).0, "Ran cargo");
    let blank = ThreadItemDetails::Harness(Box::new(HarnessEventItem {
        command: Some("   ".into()),
        path: Some("target/debug/vtcode".into()),
        ..HarnessEventItem::new(HarnessEventKind::TurnBlocked)
    }));
    assert_eq!(describe_activity(&blank, None).0, "target/debug/vtcode");
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
