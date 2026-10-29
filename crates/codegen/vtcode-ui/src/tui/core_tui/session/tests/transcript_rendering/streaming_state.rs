use super::*;

#[test]
fn streaming_state_starts_false() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);
}
#[test]
fn streaming_state_set_on_agent_append_line() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);

    session.handle_command(agent_append_line_command("Hello"));

    assert!(session.is_streaming_final_answer);
}
#[test]
fn streaming_state_set_on_agent_inline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);

    session.handle_command(InlineCommand::Inline {
        kind: InlineMessageKind::Agent,
        segment: InlineSegment {
            text: "Hello".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        },
    });

    assert!(session.is_streaming_final_answer);
}
#[test]
fn streaming_state_cleared_on_turn_completion() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.handle_command(agent_append_line_command("Hello"));
    assert!(session.is_streaming_final_answer);

    session.handle_command(InlineCommand::SetInputStatus { left: None, right: None });

    assert!(!session.is_streaming_final_answer);
}
#[test]
fn streaming_state_not_cleared_on_status_update_with_content() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.handle_command(agent_append_line_command("Hello"));
    assert!(session.is_streaming_final_answer);

    session.handle_command(InlineCommand::SetInputStatus { left: Some("Working...".to_string()), right: None });

    assert!(session.is_streaming_final_answer);
}
#[test]
fn non_agent_messages_dont_trigger_streaming_state() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::User,
        segments: vec![InlineSegment {
            text: "Hello".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    });

    assert!(!session.is_streaming_final_answer);
}
#[test]
fn empty_agent_segments_dont_trigger_streaming_state() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.handle_command(InlineCommand::AppendLine { kind: InlineMessageKind::Agent, segments: vec![] });

    assert!(!session.is_streaming_final_answer);
}
/// Regression for tool-summary detail grouping: a header (`• Search code`)
/// with its `  └ ` details must render as a tight block – only one blank
/// line before the header and one after the last detail, no gaps between
/// header/details. This prevents the “too blank” double-gap reported in
/// the screenshot (header → details and detail → detail should be tight).
#[test]
fn tool_summary_details_are_tightly_grouped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Info, vec![make_segment("• Search code for core agent loop")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("  └ File types: rs")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("  └ Max results: 25")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("  └ Path: crates/codegen/vtcode-core")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("  └ Result types: definition, path")]);
    let width = 80u16;
    let lines = session.reflow_transcript_lines(width);
    // Header + 4 details should be tight: only top/bottom blanks, no gaps inside.
    assert_eq!(lines.len(), 7);
    let texts: Vec<String> = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert!(texts[0].trim().is_empty()); // top
    assert_eq!(texts[1], "• Search code for core agent loop");
    assert_eq!(texts[2], "  └ File types: rs");
    assert_eq!(texts[3], "  └ Max results: 25");
    assert_eq!(texts[4], "  └ Path: crates/codegen/vtcode-core");
    assert_eq!(texts[5], "  └ Result types: definition, path");
    assert!(texts[6].trim().is_empty()); // bottom
}
/// Agent pre-announcement → tool block keeps exactly one blank line so the
/// message stays visually distinct from its work log (single ownership: the
/// agent suppresses its trailing gap and the tool block owns the top gap).
#[test]
fn agent_to_tool_has_single_blank() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![make_segment(
            "Got the doc overview. Now let me see the actual loop implementation.",
        )],
    );
    session.push_line(InlineMessageKind::Info, vec![make_segment("• Ran 2 commands")]);
    let width = 80u16;
    let lines = session.reflow_transcript_lines(width);
    let texts: Vec<String> = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    // Find the Agent line and the following Ran line.
    let agent_idx = texts.iter().position(|t| t.contains("Got the doc")).expect("agent line");
    let ran_idx = texts.iter().position(|t| t.contains("Ran 2 commands")).expect("ran line");
    // One blank line between them so prose stays distinct from its work log.
    assert_eq!(ran_idx, agent_idx + 2, "expected one blank row, got texts: {texts:?}");
    assert!(texts[agent_idx + 1].trim().is_empty(), "gap row should be blank, got texts: {texts:?}");
    // Also verify the pure policy: Agent -> Tool adds the tool top gap.
    use crate::tui::core_tui::session::message::MessageLine;
    use crate::tui::core_tui::session::reflow::should_add_tool_block_top_spacing_for_kinds;
    use crate::tui::core_tui::types::InlineMessageKind as Kind;
    let agent_line = MessageLine {
        diagnostic: None,
        kind: Kind::Agent,
        segments: vec![make_segment("hello")],
        link_ranges: vec![],
        revision: 0,
    };
    let tool_line = MessageLine {
        diagnostic: None,
        kind: Kind::Info,
        segments: vec![make_segment("• Search code")],
        link_ranges: vec![],
        revision: 0,
    };
    assert!(should_add_tool_block_top_spacing_for_kinds(&agent_line, &tool_line));
}
