use super::*;

#[test]
fn agent_trailing_blank_lines_do_not_stack_with_turn_gap() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("the answer\n\n")]);
    session.push_line(InlineMessageKind::User, vec![make_segment("follow-up")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let answer = texts.iter().position(|text| text.contains("the answer")).expect("agent answer");
    let divider = texts
        .iter()
        .position(|text| !text.is_empty() && text.chars().all(|ch| ch == '─'))
        .expect("user divider");
    let gap = &texts[answer + 1..divider];
    assert_eq!(gap.len(), 1, "content trailing blanks must not stack with the turn gap, got {texts:?}");
    assert!(gap[0].trim().is_empty(), "gap row should be blank, got {texts:?}");
}
#[test]
fn agent_trailing_blanks_do_not_stack_with_tool_gap() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("lead-in prose\n\n")]);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let prose = texts
        .iter()
        .position(|text| text.contains("lead-in prose"))
        .expect("agent prose");
    let tool = texts
        .iter()
        .position(|text| text.contains("Ran cargo check"))
        .expect("tool header");
    let gap = &texts[prose + 1..tool];
    assert_eq!(gap.len(), 1, "content trailing blanks must not stack with the tool gap, got {texts:?}");
    assert!(gap[0].trim().is_empty(), "gap row should be blank, got {texts:?}");
}
#[test]
fn user_followed_by_tool_has_single_blank() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::User, vec![make_segment("run the check")]);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let user = texts.iter().position(|text| text.contains("run the check")).expect("user text");
    let tool = texts
        .iter()
        .position(|text| text.contains("Ran cargo check"))
        .expect("tool header");
    assert!(tool > user, "tool header should follow the user text, got {texts:?}");
    let gap = &texts[user + 1..tool];
    assert_eq!(gap.len(), 1, "expected exactly one blank row, got {texts:?}");
    assert!(gap[0].trim().is_empty(), "gap row should be blank, got {texts:?}");
}
#[test]
fn policy_run_before_tool_has_single_blank() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Policy, vec![make_segment("thinking aloud")]);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let policy = texts
        .iter()
        .position(|text| text.contains("thinking aloud"))
        .expect("policy text");
    let tool = texts
        .iter()
        .position(|text| text.contains("Ran cargo check"))
        .expect("tool header");
    assert!(tool > policy, "tool header should follow the policy text, got {texts:?}");
    let gap = &texts[policy + 1..tool];
    assert_eq!(gap.len(), 1, "expected exactly one blank row, got {texts:?}");
    assert!(gap[0].trim().is_empty(), "gap row should be blank, got {texts:?}");
}
#[test]
fn spacing_zero_keeps_minimum_gap_before_divider_and_tool_blocks() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.message_block_spacing = 0;
    session.push_line(InlineMessageKind::Agent, vec![make_segment("the answer")]);
    session.push_line(InlineMessageKind::User, vec![make_segment("follow-up")]);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let answer = texts.iter().position(|text| text.contains("the answer")).expect("agent answer");
    let divider = texts
        .iter()
        .position(|text| !text.is_empty() && text.chars().all(|ch| ch == '─'))
        .expect("user divider");
    let gap = &texts[answer + 1..divider];
    assert_eq!(gap.len(), 1, "agent gap must keep its min-1 floor, got {texts:?}");

    let user = texts.iter().position(|text| text.contains("follow-up")).expect("user text");
    let tool = texts
        .iter()
        .position(|text| text.contains("Ran cargo check"))
        .expect("tool header");
    let tool_gap = &texts[user + 1..tool];
    assert_eq!(tool_gap.len(), 1, "tool top must keep its min-1 floor, got {texts:?}");
}
#[test]
fn spacing_two_does_not_stack_beyond_config() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.message_block_spacing = 2;
    session.push_line(InlineMessageKind::Agent, vec![make_segment("the answer\n\n")]);
    session.push_line(InlineMessageKind::User, vec![make_segment("follow-up")]);
    session.push_line(InlineMessageKind::Tool, vec![make_segment("• Ran cargo check")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let answer = texts.iter().position(|text| text.contains("the answer")).expect("agent answer");
    let divider = texts
        .iter()
        .position(|text| !text.is_empty() && text.chars().all(|ch| ch == '─'))
        .expect("user divider");
    let gap = &texts[answer + 1..divider];
    assert_eq!(gap.len(), 2, "content blanks must not stack on top of spacing 2, got {texts:?}");

    let user = texts.iter().position(|text| text.contains("follow-up")).expect("user text");
    let tool = texts
        .iter()
        .position(|text| text.contains("Ran cargo check"))
        .expect("tool header");
    let tool_gap = &texts[user + 1..tool];
    assert_eq!(tool_gap.len(), 2, "tool top follows config without doubling, got {texts:?}");
}
#[test]
fn consecutive_agent_lines_keep_single_interior_blank_row() {
    // Markdown separators arrive as empty Agent message lines (the sink maps
    // blank MarkdownLines to empty segment vecs); reflow must render each as
    // exactly one blank row with no divider and no collapsing.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("• Alpha")]);
    session.push_line(InlineMessageKind::Agent, Vec::new());
    session.push_line(InlineMessageKind::Agent, vec![make_segment("• Beta")]);

    let rendered = session.reflow_transcript_lines(80);
    let texts: Vec<String> = rendered.iter().map(line_text).collect();
    let alpha = texts.iter().position(|text| text.contains("Alpha")).expect("first item");
    let beta = texts.iter().position(|text| text.contains("Beta")).expect("second item");
    assert_eq!(beta, alpha + 2, "expected exactly one blank row between items, got {texts:?}");
    assert!(texts[alpha + 1].trim().is_empty(), "gap row should be blank, got {texts:?}");
    assert!(
        texts.iter().all(|text| text.is_empty() || !text.chars().all(|ch| ch == '─')),
        "consecutive agent lines must not gain a divider, got {texts:?}"
    );
}

#[test]
fn pty_wrapped_lines_keep_hanging_left_padding() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_pty_line(&mut session, "  └ this PTY output line wraps on narrow widths");

    let rendered = session.reflow_pty_lines(0, 18);
    let content_lines: Vec<String> = rendered
        .iter()
        .map(|line| line_text(&line.line))
        .filter(|text| !text.is_empty())
        .collect();
    assert!(content_lines.len() >= 2, "expected wrapped PTY output, got {} content line(s)", content_lines.len());

    let first = &content_lines[0];
    let second = &content_lines[1];

    // No left gutter – PTY body is flush, tree marker provides its own indent.
    assert!(first.starts_with("  └ "), "first line was: {first:?}");
    assert!(second.starts_with("    "), "wrapped line should keep hanging indent, got: {second:?}");
}
