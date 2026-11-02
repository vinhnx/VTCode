use super::*;

#[test]
fn agent_messages_use_zero_indent_prose() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![make_segment(
            "Hello, here is the information you requested. This is an example of a standard agent message.",
        )],
    );

    let lines = session.reflow_transcript_lines(32);
    let content_lines: Vec<String> = lines.iter().map(line_text).filter(|text| !text.trim().is_empty()).collect();
    assert!(content_lines.len() >= 2, "expected wrapped agent lines to be visible");
    let first_line = &content_lines[0];
    let second_line = &content_lines[1];

    assert!(
        !first_line.starts_with(' ') && !first_line.starts_with('•'),
        "agent prose should start at column 0 with no bullet gap, got: {first_line:?}",
    );
    assert!(
        !second_line.starts_with(' '),
        "agent message continuation should start at column 0, got: {second_line:?}",
    );
    assert!(!first_line.contains('│'), "agent message should not render a left border",);
}
#[test]
fn labeled_agent_message_continuations_align_under_the_body() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.labels.agent = Some("Assistant: ".to_string());
    session.push_line(
        InlineMessageKind::Agent,
        vec![make_segment(
            "This response wraps so its continuation should start beneath the message body.",
        )],
    );

    let lines = session.reflow_transcript_lines(28);
    let content_lines: Vec<String> = lines.iter().map(line_text).filter(|text| !text.trim().is_empty()).collect();
    let first = content_lines.first().expect("first labeled message row");
    let second = content_lines.get(1).expect("wrapped continuation row");
    let prefix_width = "Assistant: ".chars().count();

    assert!(first.starts_with("Assistant: This"), "role label should remain on the first row: {first:?}");
    assert!(
        second.starts_with(&" ".repeat(prefix_width)),
        "continuation should align under the message body: {second:?}",
    );
}
#[test]
fn agent_prose_has_no_bullet_gap() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("Response")]);

    let index = session.lines.len().checked_sub(1).expect("agent message should be available");
    let spans = session.render_message_spans(index);
    assert!(
        !spans.iter().any(|span| span.content.as_ref().contains('•')),
        "agent prose must not render a bullet gap, got {spans:?}"
    );
}
#[test]
fn wrap_line_splits_double_width_graphemes() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "你好世界");

    let wrapped = session.wrap_line(line, 4);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["你好".to_string(), "世界".to_string()]);
}
