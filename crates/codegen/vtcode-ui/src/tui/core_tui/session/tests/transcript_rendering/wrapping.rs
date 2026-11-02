use super::*;

#[test]
fn wrap_line_keeps_explicit_blank_rows() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "top\n\nbottom");

    let wrapped = session.wrap_line(line, 40);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["top".to_string(), String::new(), "bottom".to_string()]);
}
#[test]
fn wrap_line_prefers_word_boundaries_for_plain_text() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "alpha beta gamma");

    let wrapped = session.wrap_line(line, 7);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]);
}
#[test]
fn wrap_line_keeps_words_intact_across_same_style_stream_chunks() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let style = ratatui_style_from_inline(&session.default_style(), None);
    let line = Line::from(vec![
        Span::styled("alpha be".to_string(), style),
        Span::styled("ta gamma".to_string(), style),
    ]);

    let wrapped = session.wrap_line(line, 7);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]);
}
#[test]
fn wrap_line_keeps_list_continuation_aligned() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "• alpha beta gamma");

    let wrapped = session.wrap_line(line, 8);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["• alpha".to_string(), "  beta".to_string(), "  gamma".to_string()]);
}
#[test]
fn wrap_line_preserves_characters_wider_than_viewport() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "你");

    let wrapped = session.wrap_line(line, 1);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["你".to_string()]);
}
#[test]
fn wrap_line_discards_carriage_return_before_newline() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "foo\r\nbar");

    let wrapped = session.wrap_line(line, 80);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["foo".to_string(), "bar".to_string()]);
}
#[test]
fn tool_code_fence_markers_are_skipped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.append_inline(
        InlineMessageKind::Tool,
        InlineSegment {
            text: "```rust\nfn demo() {}\n```".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        },
    );

    let tool_lines: Vec<&MessageLine> = session
        .lines
        .iter()
        .filter(|line| line.kind == InlineMessageKind::Tool)
        .collect();

    assert_eq!(tool_lines.len(), 1);
    let Some(first_line) = tool_lines.first() else {
        panic!("Expected at least one tool line");
    };
    assert_eq!(first_line.segments.len(), 1);
    let Some(first_segment) = first_line.segments.first() else {
        panic!("Expected at least one segment");
    };
    assert_eq!(first_segment.text.as_str(), "```rust\nfn demo() {}\n```");
    assert!(!session.in_tool_code_fence);
}
