use super::*;

#[test]
fn timeline_visible_selects_latest_item() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("First")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("Second")]);

    let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session with timeline");

    assert!(session.navigation_state.selected().is_none());
}
#[test]
fn tool_detail_renders_with_border_and_body_style() {
    let theme = themed_inline_colors();
    let mut session = Session::new(theme, None, VIEW_ROWS);
    let detail_style = InlineTextStyle::default().italic();
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "    result line".to_string(),
            style: Arc::new(detail_style),
        }],
    );

    let index = session.lines.len().checked_sub(1).expect("tool detail line should exist");
    let spans = session.render_message_spans(index);

    assert_eq!(spans.len(), 1);
    let body_span = &spans[0];
    assert!(body_span.style.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(body_span.content.clone().into_owned(), "    result line");
}
#[test]
fn top_level_task_tree_tail_line_is_dimmed_in_tool_blocks() {
    let theme = themed_inline_colors();
    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "└ Report actions taken, blockers, and required user input".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let index = session.lines.len().checked_sub(1).expect("tool detail line should exist");
    let transcript_lines = session.reflow_message_lines(index, 100, false);
    let task_span = transcript_lines
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("Report actions taken"))
        .expect("expected task span");

    assert!(task_span.style.add_modifier.contains(Modifier::DIM), "top-level task rows should render dimmed");
}
#[test]
fn tool_block_with_cjk_prefix_does_not_overflow_viewport() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "日本語のツール詳細情報".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    let index = session.lines.len() - 1;
    let transcript = session.reflow_message_lines(index, VIEW_WIDTH, false);
    for line in &transcript {
        let width: usize = line.line.spans.iter().map(|span| span.content.width()).sum();
        assert!(
            width <= VIEW_WIDTH as usize,
            "tool block line with CJK prefix overflowed viewport: {width} > {VIEW_WIDTH}"
        );
    }
}
#[test]
fn error_block_with_cjk_label_reserves_correct_content_width() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Error,
        vec![InlineSegment {
            text: "エラーが発生しました".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    let index = session.lines.len() - 1;
    let transcript = session.reflow_message_lines(index, VIEW_WIDTH, false);
    for line in &transcript {
        let width: usize = line.line.spans.iter().map(|span| span.content.width()).sum();
        assert!(
            width <= VIEW_WIDTH as usize,
            "error block line with CJK label overflowed viewport: {width} > {VIEW_WIDTH}"
        );
    }
}
