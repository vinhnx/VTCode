use super::super::*;
use super::helpers::*;
use crate::ui::tui::style::ratatui_style_from_inline;

// ---------------------------------------------------------------------------
// Common test helpers extracted from repeated patterns
// ---------------------------------------------------------------------------

fn make_pty_segment(text: &str) -> InlineSegment {
    InlineSegment {
        text: text.to_string(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn push_pty_line(session: &mut Session, text: &str) {
    session.push_line(InlineMessageKind::Pty, vec![make_pty_segment(text)]);
}

fn make_styled_line(session: &Session, text: &str) -> Line<'static> {
    Line::from(vec![Span::styled(
        text.to_string(),
        ratatui_style_from_inline(&session.default_style(), None),
    )])
}

fn agent_append_line_command(text: &str) -> InlineCommand {
    InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![InlineSegment {
            text: text.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    }
}

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
    assert_eq!(
        session.scroll_offset(),
        before_offset,
        "streaming should preserve manual scroll offset"
    );
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

    session.force_view_rows(
        (LINE_COUNT as u16)
            + ui::INLINE_HEADER_HEIGHT
            + Session::input_block_height_for_lines(1)
            + 2,
    );

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

#[test]
fn agent_messages_include_left_padding() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![make_segment(
            "Hello, here is the information you requested. This is an example of a standard agent message.",
        )],
    );

    let lines = session.reflow_transcript_lines(32);
    let content_lines: Vec<String> = lines
        .iter()
        .map(line_text)
        .filter(|text| !text.trim().is_empty())
        .collect();
    assert!(
        content_lines.len() >= 2,
        "expected wrapped agent lines to be visible"
    );
    let first_line = &content_lines[0];
    let second_line = &content_lines[1];

    let expected_prefix = format!(
        "{}{}",
        ui::INLINE_AGENT_QUOTE_PREFIX,
        ui::INLINE_AGENT_MESSAGE_LEFT_PADDING
    );
    let continuation_prefix = " ".repeat(expected_prefix.chars().count());

    assert!(
        first_line.starts_with(&expected_prefix),
        "agent message should include left padding",
    );
    assert!(
        second_line.starts_with(&continuation_prefix),
        "agent message continuation should align with content padding",
    );
    assert!(
        !second_line.starts_with(&expected_prefix),
        "agent message continuation should not repeat bullet prefix",
    );
    assert!(
        !first_line.contains('│'),
        "agent message should not render a left border",
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

#[test]
fn wrap_line_keeps_explicit_blank_rows() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "top\n\nbottom");

    let wrapped = session.wrap_line(line, 40);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(
        rendered,
        vec!["top".to_string(), String::new(), "bottom".to_string()]
    );
}

#[test]
fn wrap_line_prefers_word_boundaries_for_plain_text() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "alpha beta gamma");

    let wrapped = session.wrap_line(line, 7);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(
        rendered,
        vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
    );
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

    assert_eq!(
        rendered,
        vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
    );
}

#[test]
fn wrap_line_keeps_list_continuation_aligned() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line = make_styled_line(&session, "• alpha beta gamma");

    let wrapped = session.wrap_line(line, 8);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(
        rendered,
        vec![
            "• alpha".to_string(),
            "  beta".to_string(),
            "  gamma".to_string()
        ]
    );
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

    assert!(
        session.reflow_pty_lines(0, 80).is_empty(),
        "placeholder PTY line should remain hidden",
    );

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
fn pty_wrapped_lines_keep_hanging_left_padding() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_pty_line(
        &mut session,
        "  └ this PTY output line wraps on narrow widths",
    );

    let rendered = session.reflow_pty_lines(0, 18);
    assert!(
        rendered.len() >= 2,
        "expected wrapped PTY output, got {} line(s)",
        rendered.len()
    );

    let first = line_text(&rendered[0].line);
    let second = line_text(&rendered[1].line);

    assert!(first.starts_with("    └ "), "first line was: {first:?}");
    assert!(
        second.starts_with("      "),
        "wrapped line should keep hanging indent, got: {second:?}"
    );
}

#[test]
fn pty_wrapped_lines_do_not_exceed_viewport_width() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_pty_line(
        &mut session,
        "  └ this PTY output line wraps on narrow widths",
    );

    let width = 18usize;
    let rendered = session.reflow_pty_lines(0, width as u16);
    for line in rendered {
        let line_width: usize = line.line.spans.iter().map(|span| span.width()).sum();
        assert!(
            line_width <= width,
            "wrapped PTY line exceeded viewport width: {line_width} > {width}",
        );
    }
}

#[test]
fn tool_diff_numbered_lines_keep_hanging_indent_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text:
                "459 + let digits_len = digits.chars().take_while(|c| c.is_ascii_digit()).count();"
                    .to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let rendered = session.reflow_transcript_lines(40);
    assert!(
        rendered.len() >= 2,
        "expected wrapped tool diff output, got {} line(s)",
        rendered.len()
    );

    let first = line_text(&rendered[0]);
    let second = line_text(&rendered[1]);

    assert!(
        first.contains("459 + "),
        "first line should include diff gutter: {first:?}"
    );
    assert!(
        second.starts_with("          "),
        "wrapped line should keep hanging indent after tool prefix, got: {second:?}"
    );
}

#[test]
fn agent_numbered_code_lines_keep_hanging_indent_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![
            InlineSegment {
                text: " 12  ".to_string(),
                style: Arc::new(InlineTextStyle {
                    effects: anstyle::Effects::DIMMED,
                    ..InlineTextStyle::default()
                }),
            },
            make_segment(
                "fn wrapped_diff_continuation_prefix(line_text: &str) -> Option<String> {",
            ),
        ],
    );

    let rendered = session.reflow_transcript_lines(36);
    let content_lines: Vec<String> = rendered
        .iter()
        .map(line_text)
        .filter(|text| !text.trim().is_empty())
        .collect();
    assert!(
        content_lines.len() >= 2,
        "expected wrapped code line, got: {content_lines:?}"
    );

    let first = &content_lines[0];
    let second = &content_lines[1];
    let agent_indent = " ".repeat(
        format!(
            "{}{}",
            ui::INLINE_AGENT_QUOTE_PREFIX,
            ui::INLINE_AGENT_MESSAGE_LEFT_PADDING
        )
        .chars()
        .count(),
    );
    let expected_prefix = format!("{agent_indent}{}", " ".repeat(" 12  ".chars().count()));

    assert!(
        first.contains("12  fn wrapped_diff"),
        "first line was: {first:?}"
    );
    assert!(
        second.starts_with(&expected_prefix),
        "wrapped code continuation should keep gutter indent, got: {second:?}"
    );
}

#[test]
fn agent_omitted_code_lines_keep_hanging_indent_when_wrapped() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "21-421  … [+400 lines omitted; use read_file with offset/limit (1-indexed line numbers) for full content]".to_string(),
            style: Arc::new(InlineTextStyle {
                effects: anstyle::Effects::DIMMED,
                ..InlineTextStyle::default()
            }),
        }],
    );

    let rendered = session.reflow_transcript_lines(52);
    let content_lines: Vec<String> = rendered
        .iter()
        .map(line_text)
        .filter(|text| !text.trim().is_empty())
        .collect();
    assert!(
        content_lines.len() >= 2,
        "expected wrapped omitted line, got: {content_lines:?}"
    );

    let first = &content_lines[0];
    let second = &content_lines[1];
    let agent_indent = " ".repeat(
        format!(
            "{}{}",
            ui::INLINE_AGENT_QUOTE_PREFIX,
            ui::INLINE_AGENT_MESSAGE_LEFT_PADDING
        )
        .chars()
        .count(),
    );
    let expected_prefix = format!("{agent_indent}{}", " ".repeat("21-421  ".chars().count()));

    assert!(
        first.contains("21-421  … [+400 lines omitted"),
        "first line was: {first:?}"
    );
    assert!(
        second.starts_with(&expected_prefix),
        "wrapped omitted-line continuation should keep gutter indent, got: {second:?}"
    );
}

#[test]
fn pty_lines_use_subdued_foreground() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    push_pty_line(&mut session, "plain pty output");

    let rendered = session.reflow_pty_lines(0, 80);
    let body_span = rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("plain pty output"))
        .expect("expected PTY body span");
    assert!(
        body_span.style.fg.is_some() || body_span.style.add_modifier.contains(Modifier::DIM),
        "PTY body span should apply non-default visual styling"
    );
}

#[test]
fn assistant_text_is_brighter_than_pty_output() {
    let agent_fg = Color::Rgb(0xEE, 0xEE, 0xEE);
    let pty_fg = Color::Rgb(0x7A, 0x7A, 0x7A);
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        pty_body: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x7A, 0x7A))),
        ..Default::default()
    };

    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "assistant reply".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "pty output".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let agent_spans = session.render_message_spans(0);
    let agent_body = agent_spans
        .iter()
        .find(|span| span.content.contains("assistant reply"))
        .expect("expected assistant body span");
    assert_eq!(agent_body.style.fg, Some(agent_fg));

    let pty_rendered = session.reflow_pty_lines(1, 80);
    let pty_body = pty_rendered
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("pty output"))
        .expect("expected PTY body span");
    assert_eq!(pty_body.style.fg, Some(pty_fg));
    assert!(pty_body.style.add_modifier.contains(Modifier::DIM));
    assert_ne!(agent_body.style.fg, pty_body.style.fg);
}

#[test]
fn pty_scroll_preserves_order() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 0..200 {
        let label = format!("{LABEL_PREFIX}-{index}");
        push_pty_line(&mut session, &label);
    }

    let bottom_view = visible_transcript(&mut session);
    assert!(
        bottom_view
            .iter()
            .any(|line| line.contains(&format!("{LABEL_PREFIX}-199"))),
        "bottom view should include latest PTY line"
    );

    for _ in 0..200 {
        session.scroll_page_up();
        if session.scroll_manager.offset() == session.current_max_scroll_offset() {
            break;
        }
    }

    let top_view = visible_transcript(&mut session);
    assert!(
        (0..=5).any(|index| top_view
            .iter()
            .any(|line| line.contains(&format!("{LABEL_PREFIX}-{index}")))),
        "top view should include earliest PTY lines"
    );
    assert!(
        top_view
            .iter()
            .all(|line| !line.contains(&format!("{LABEL_PREFIX}-199"))),
        "top view should not include latest PTY line"
    );
}

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

    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: None,
    });

    assert!(!session.is_streaming_final_answer);
}

#[test]
fn streaming_state_not_cleared_on_status_update_with_content() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.handle_command(agent_append_line_command("Hello"));
    assert!(session.is_streaming_final_answer);

    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Working...".to_string()),
        right: None,
    });

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

    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![],
    });

    assert!(!session.is_streaming_final_answer);
}
