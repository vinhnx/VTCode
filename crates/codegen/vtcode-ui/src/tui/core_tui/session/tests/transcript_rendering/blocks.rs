use super::*;

#[test]
fn pty_block_omits_placeholder_when_empty() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Pty, Vec::new());

    let lines = session.reflow_pty_lines(0, 80);
    assert!(lines.is_empty());
}
#[test]
fn pty_block_hides_until_output_available() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Pty, Vec::new());

    assert!(session.reflow_pty_lines(0, 80).is_empty());

    push_pty_line(&mut session, "first output");

    assert!(session.reflow_pty_lines(0, 80).is_empty(), "placeholder PTY line should remain hidden",);

    let rendered = session.reflow_pty_lines(1, 80);
    assert!(rendered.iter().any(|line| !line.line.spans.is_empty()));
}
#[test]
fn pty_block_skips_status_only_sequence() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Pty, Vec::new());
    session.push_line(InlineMessageKind::Pty, Vec::new());

    assert!(session.reflow_pty_lines(0, 80).is_empty());
    assert!(session.reflow_pty_lines(1, 80).is_empty());
}
#[test]
fn pty_tool_block_has_top_and_bottom_spacing() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_pty_line(&mut session, "first output");
    push_pty_line(&mut session, "second output");

    let first = session.reflow_pty_lines(0, 80);
    let last = session.reflow_pty_lines(1, 80);

    assert!(first.first().is_some_and(|line| line.line.spans.is_empty()));
    assert!(last.last().is_some_and(|line| line.line.spans.is_empty()));
}
#[test]
fn tool_block_has_top_and_bottom_spacing() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("tool output")]);

    let rendered = session.reflow_message_lines(0, 80, false);

    assert!(rendered.first().is_some_and(|line| line.line.spans.is_empty()));
    assert!(rendered.last().is_some_and(|line| line.line.spans.is_empty()));
}
#[test]
fn cached_reflow_refreshes_tool_and_pty_block_boundaries() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("I will run the check.")]);
    let _ = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);
    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);
    let agent = rendered
        .iter()
        .position(|line| line.contains("I will run the check."))
        .expect("agent line");
    let tool = rendered
        .iter()
        .position(|line| line.contains("Ran cargo check"))
        .expect("tool header");
    assert_eq!(tool, agent + 2, "agent-to-tool boundary needs exactly one blank row: {rendered:?}");
    assert!(rendered[agent + 1].trim().is_empty(), "tool gap must be blank: {rendered:?}");

    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::User, vec![make_segment("Run the check.")]);
    let _ = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);
    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);
    let user = rendered
        .iter()
        .position(|line| line.contains("Run the check."))
        .expect("user line");
    let tool = rendered
        .iter()
        .position(|line| line.contains("Ran cargo check"))
        .expect("tool header");
    assert_eq!(tool, user + 2, "user-to-tool boundary needs exactly one blank row: {rendered:?}");
    assert!(rendered[user + 1].trim().is_empty(), "tool gap must be blank: {rendered:?}");

    for kind in [InlineMessageKind::Tool, InlineMessageKind::Pty] {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        session.push_line(kind, vec![make_segment("• Ran first-command")]);
        let _ = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

        session.push_line(kind, vec![make_segment("  └ second output")]);
        let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);
        let first = rendered
            .iter()
            .position(|line| line.contains("first-command"))
            .expect("first command");
        let second = rendered
            .iter()
            .position(|line| line.contains("second output"))
            .expect("second output");
        assert_eq!(
            second,
            first + 1,
            "{kind:?} block must not keep stale trailing spacing after an appended line: {rendered:?}"
        );
    }
}
#[test]
fn cached_reflow_drops_replaced_info_group_details() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Info, vec![make_segment("First info detail")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("Removed info detail")]);
    let _ = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    session.replace_last(1, InlineMessageKind::Agent, vec![vec![make_segment("Replacement answer")]], None);
    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered.iter().any(|line| line.contains("First info detail")),
        "remaining detail is missing: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.contains("Replacement answer")),
        "replacement is missing: {rendered:?}"
    );
    assert!(
        rendered.iter().all(|line| !line.contains("Removed info detail")),
        "cached info-box head retained a replaced detail: {rendered:?}"
    );
}
#[test]
fn agent_followed_by_user_has_single_blank_before_divider() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("the answer")]);
    session.push_line(InlineMessageKind::User, vec![make_segment("follow-up")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let answer = texts.iter().position(|text| text.contains("the answer")).expect("agent answer");
    let divider = texts
        .iter()
        .position(|text| !text.is_empty() && text.chars().all(|ch| ch == '─'))
        .expect("user divider");
    assert!(divider > answer, "divider should follow the answer, got {texts:?}");
    let gap = &texts[answer + 1..divider];
    assert_eq!(gap.len(), 1, "expected exactly one blank row before the divider, got {texts:?}");
    assert!(gap[0].trim().is_empty(), "gap row should be blank, got {texts:?}");
}
