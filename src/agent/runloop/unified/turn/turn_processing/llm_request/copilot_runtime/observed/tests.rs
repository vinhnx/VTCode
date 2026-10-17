use std::time::Duration;

use anstyle::{AnsiColor, Color};
use serde_json::json;
use tokio::sync::mpsc::unbounded_channel;
use vtcode_ui::tui::app::{InlineCommand, InlineMessageKind};

use super::{
    CopilotObservedToolCall, CopilotObservedToolCallStatus, InlineHandle, ObservedToolCallState, PtyConfig,
    observed_tool_command_display, observed_tool_output_delta,
};

fn update(status: CopilotObservedToolCallStatus, output: Option<&str>) -> CopilotObservedToolCall {
    CopilotObservedToolCall {
        tool_call_id: "observed-7".to_string(),
        tool_name: "Run printf observed".to_string(),
        status,
        arguments: Some(json!({"cmd": "printf observed"})),
        output: output.map(str::to_string),
        terminal_id: None,
    }
}

#[test]
fn cumulative_output_deltas_preserve_unicode_and_replacements() {
    for (previous, current, expected) in [
        (None, "A界", Some("A界")),
        (Some("A界"), "A界B尾", Some("B尾")),
        (Some("A界B"), "A界C", Some("C")),
        (Some("éA"), "êB", Some("êB")),
        (Some("A界B"), "A界", Some("A界")),
        (Some("A界"), "A界", None),
        (Some("A界"), "", None),
        (Some("left"), "right", Some("right")),
    ] {
        assert_eq!(observed_tool_output_delta(previous, current), expected);
    }
}

#[test]
fn command_display_uses_canonical_aliases_before_tool_name_fallback() {
    for key in ["command", "cmd", "raw_command", "bash_command"] {
        let mut call = update(CopilotObservedToolCallStatus::Pending, None);
        call.arguments = Some(json!({key: "printf argument"}));
        assert_eq!(observed_tool_command_display(&call).as_deref(), Some("printf argument"));
    }
    let mut call = update(CopilotObservedToolCallStatus::Pending, None);
    call.arguments = None;
    call.tool_name = "Run   printf fallback  ".to_string();
    assert_eq!(observed_tool_command_display(&call).as_deref(), Some("printf fallback"));
    call.tool_name = "Run   ".to_string();
    assert!(observed_tool_command_display(&call).is_none());
    call.tool_name = "Read a file".to_string();
    assert!(observed_tool_command_display(&call).is_none());
}

#[tokio::test]
async fn placeholder_name_enrichment_keeps_one_start_and_ignores_blank_output() {
    let (sender, _receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let config = PtyConfig::default();
    let mut state = ObservedToolCallState::new("copilot_tool".to_string());
    let mut call = update(CopilotObservedToolCallStatus::Pending, Some(" \n\t"));
    call.tool_name = "copilot_tool".to_string();
    call.arguments = None;
    let first = state.apply(&call, 8, &handle, &config);
    assert!(first.started);
    assert!(!first.finished);
    assert!(first.output_snapshot.is_none());
    assert!(state.pty_stream.is_none());

    call.tool_name = "Read a file".to_string();
    call.output = Some("first snapshot".to_string());
    let enriched = state.apply(&call, 8, &handle, &config);
    assert_eq!(state.tool_name(), "Read a file");
    assert!(!enriched.started);
    assert_eq!(enriched.output_snapshot.as_deref(), Some("first snapshot"));
    call.tool_name = "copilot_tool".to_string();
    let repeated = state.apply(&call, 8, &handle, &config);
    assert_eq!(state.tool_name(), "Read a file");
    assert!(repeated.output_snapshot.is_none());
    assert!(!repeated.started);
}

#[tokio::test]
async fn state_publishes_snapshots_once_and_streams_only_appended_output_before_finish() {
    for (status, expected_color) in [
        (CopilotObservedToolCallStatus::Completed, AnsiColor::Green),
        (CopilotObservedToolCallStatus::Failed, AnsiColor::Red),
    ] {
        let (sender, mut receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let config = PtyConfig::default();
        let mut state = ObservedToolCallState::new("Run printf observed".to_string());
        let first_call = update(CopilotObservedToolCallStatus::InProgress, Some("A界\n"));
        let first = state.apply(&first_call, 8, &handle, &config);
        assert!(first.started);
        assert!(!first.finished);
        assert_eq!(first.output_snapshot.as_deref(), Some("A界\n"));
        let repeated = state.apply(&first_call, 8, &handle, &config);
        assert!(!repeated.started);
        assert!(repeated.output_snapshot.is_none());

        let final_call = update(status, Some("A界\nB尾\n"));
        let final_update = state.apply(&final_call, 8, &handle, &config);
        assert!(!final_update.started);
        assert!(final_update.finished);
        assert_eq!(final_update.output_snapshot.as_deref(), Some("A界\nB尾\n"));
        assert!(state.pty_stream.is_none());
        let repeated = state.apply(&final_call, 8, &handle, &config);
        assert!(!repeated.finished);
        assert!(repeated.output_snapshot.is_none());
        drop(state);
        drop(handle);

        let mut final_block = None;
        tokio::time::timeout(Duration::from_secs(3), async {
            while let Some(command) = receiver.recv().await {
                if let InlineCommand::ReplaceLast { kind: InlineMessageKind::Pty, lines, .. } = command {
                    final_block = Some(lines);
                }
            }
        })
        .await
        .expect("completed observed presentation must release handles");
        let block = final_block.expect("final preview");
        let text = block
            .iter()
            .map(|line| line.iter().map(|segment| segment.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.starts_with("• Ran printf observed\n"), "{text}");
        assert!(text.find("A界").unwrap() < text.find("B尾").unwrap());
        assert_eq!(text.matches("A界").count(), 1);
        assert_eq!(text.matches("B尾").count(), 1);
        assert_eq!(block[0][0].style.color, Some(Color::Ansi(expected_color)));
    }
}
