use super::*;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn streaming_new_lines_preserves_scrolled_view() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 1..=LINE_COUNT {
        let label = format!("{LABEL_PREFIX}-{index}");
        session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
    }

    session.scroll_page_up();
    let before = visible_transcript(&mut session);
    let before_offset = session.scroll_offset();

    session.append_inline(InlineMessageKind::Agent, make_segment(EXTRA_SEGMENT));

    let after = visible_transcript(&mut session);
    assert_eq!(before.len(), after.len());
    assert_eq!(session.scroll_offset(), before_offset, "streaming should preserve manual scroll offset");
}
#[test]
fn streaming_segments_render_incrementally() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.push_line(InlineMessageKind::Agent, vec![make_segment("")]);

    session.append_inline(InlineMessageKind::Agent, make_segment("Hello"));
    let first = visible_transcript(&mut session);
    assert!(first.iter().any(|line| line.contains("Hello")));

    session.append_inline(InlineMessageKind::Agent, make_segment(" world"));
    let second = visible_transcript(&mut session);
    assert!(second.iter().any(|line| line.contains("Hello world")));
}
#[test]
fn appended_info_lines_refresh_the_cached_info_block() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.push_line(InlineMessageKind::Info, vec![make_segment("Active WebMCP bridge started.")]);
    let _ = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    session.push_line(InlineMessageKind::Info, vec![make_segment("WebSocket: ws://127.0.0.1:57759/webmcp")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("Pairing code: 1AF23A43F9C4")]);

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("WebSocket: ws://127.0.0.1:57759/webmcp")),
        "appended WebSocket line should be visible in the refreshed info block: {rendered:?}"
    );
    assert!(
        rendered.iter().any(|line| line.contains("Pairing code: 1AF23A43F9C4")),
        "appended pairing line should be visible in the refreshed info block: {rendered:?}"
    );
}
#[test]
fn info_box_after_tool_summary_invalidates_its_own_cached_head() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.push_line(InlineMessageKind::Info, vec![make_segment("• Ran cargo check")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("First info line")]);
    let _ = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    session.push_line(InlineMessageKind::Info, vec![make_segment("Second info line")]);

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);
    assert!(
        rendered.iter().any(|line| line.contains("Second info line")),
        "the info box after a tool summary should reflow from its own head: {rendered:?}"
    );
}
#[test]
fn page_up_reveals_prior_lines_until_buffer_start() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 1..=LINE_COUNT {
        let label = format!("{LABEL_PREFIX}-{index}");
        session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
    }

    let bottom_view = visible_transcript(&mut session);
    let start_offset = session.scroll_offset();
    for _ in 0..(LINE_COUNT * 2) {
        session.scroll_page_up();
        if session.scroll_offset() > start_offset {
            break;
        }
    }
    let scrolled_view = visible_transcript(&mut session);

    assert!(session.scroll_offset() > start_offset);
    assert_ne!(bottom_view, scrolled_view);
}
#[test]
fn resizing_viewport_clamps_scroll_offset() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 1..=(LINE_COUNT * 5) {
        let label = format!("{LABEL_PREFIX}-{index}");
        session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
    }

    visible_transcript(&mut session);
    for _ in 0..(LINE_COUNT * 2) {
        session.scroll_page_up();
        if session.scroll_offset() > 0 {
            break;
        }
    }
    assert!(session.scroll_offset() > 0);
    let scrolled_offset = session.scroll_offset();

    session
        .force_view_rows((LINE_COUNT as u16) + ui::INLINE_HEADER_HEIGHT + Session::input_block_height_for_lines(1) + 2);

    let max_offset = session.current_max_scroll_offset();
    assert!(session.scroll_offset() <= scrolled_offset);
    assert!(session.scroll_offset() <= max_offset);
}
#[test]
fn scroll_end_displays_full_final_paragraph() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let total = LINE_COUNT * 5;

    for index in 1..=total {
        let label = format!("{LABEL_PREFIX}-{index}");
        let text = format!("{label}\n{label}-continued");
        session.push_line(InlineMessageKind::Agent, vec![make_segment(text.as_str())]);
    }

    // Prime layout to ensure transcript dimensions are measured.
    visible_transcript(&mut session);

    for _ in 0..total {
        session.scroll_page_up();
        if session.scroll_offset() == session.current_max_scroll_offset() {
            break;
        }
    }
    assert!(session.scroll_offset() > 0);

    for _ in 0..total {
        session.scroll_page_down();
        if session.scroll_offset() == 0 {
            break;
        }
    }

    assert_eq!(session.scroll_offset(), 0);

    let view = visible_transcript(&mut session);
    let expected_tail = format!("{LABEL_PREFIX}-{total}-continued");
    assert!(
        view.iter().any(|line| line.contains(&expected_tail)),
        "expected final paragraph tail `{expected_tail}` to appear, got {view:?}"
    );
}
#[test]
fn user_messages_render_with_dividers() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::User, vec![make_segment("Hi")]);

    let width = 10;
    let lines = session.reflow_transcript_lines(width);
    assert!(
        lines.iter().any(|line| line_text(line).contains("Hi")),
        "expected user message to remain visible in transcript"
    );
}
