use super::*;

#[test]
fn header_highlights_collapse_to_single_line() {
    let mut session = session_with_highlights(vec![
        InlineHeaderHighlight {
            title: "Keyboard Shortcuts".to_string(),
            lines: vec!["/help Show help".to_string(), "Enter Submit message".to_string()],
        },
        InlineHeaderHighlight {
            title: "Usage Tips".to_string(),
            lines: vec!["- Keep tasks focused".to_string()],
        },
    ]);

    let lines = session.header_lines();
    assert_eq!(lines.len(), 1);

    let summary: String = lines[0].spans.iter().map(|span| span.content.clone().into_owned()).collect();

    assert!(summary.contains("Keyboard Shortcuts"));
    assert!(summary.contains("/help Show help"));
    assert!(summary.contains("(+1 more)"));
    assert!(!summary.contains("Enter Submit message"));
    assert!(summary.contains("Usage Tips"));
    assert!(summary.contains("Keep tasks focused"));
}
#[test]
fn header_highlight_summary_truncates_long_entries() {
    let limit = ui::HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS;
    let long_entry = "A".repeat(limit + 5);
    let mut session = session_with_highlights(vec![InlineHeaderHighlight {
        title: "Details".to_string(),
        lines: vec![long_entry.clone()],
    }]);

    let lines = session.header_lines();
    assert_eq!(lines.len(), 1);

    let summary: String = lines[0].spans.iter().map(|span| span.content.clone().into_owned()).collect();

    let expected_preview = format!("{}{}", "A".repeat(limit.saturating_sub(1)), ui::INLINE_PREVIEW_ELLIPSIS);

    assert!(summary.contains("Details"));
    assert!(summary.contains(&expected_preview));
    assert!(!summary.contains(&long_entry));
}
#[test]
fn header_highlight_summary_hides_truncated_command_segments() {
    let mut session = session_with_highlights(vec![InlineHeaderHighlight {
        title: String::new(),
        lines: vec![
            "  - /{command}".to_string(),
            "  - /help Show slash command help".to_string(),
            "  - Enter Submit message".to_string(),
            "  - Escape Cancel input".to_string(),
        ],
    }]);

    let lines = session.header_lines();
    assert_eq!(lines.len(), 1);

    let summary: String = lines[0].spans.iter().map(|span| span.content.clone().into_owned()).collect();

    assert!(summary.contains("/{command}"));
    assert!(summary.contains("(+3 more)"));
    assert!(!summary.contains("Escape"));
    assert!(!summary.contains(ui::INLINE_PREVIEW_ELLIPSIS));
}
#[test]
fn header_height_expands_when_wrapping_required() {
    let mut session = fresh_session();
    session.header_context.provider = format!("{}Example Provider With Extended Label", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}ExampleModelIdentifierWithDetail", ui::HEADER_MODEL_PREFIX);
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);
    session.header_context.workspace_trust = format!("{}full auto", ui::HEADER_TRUST_PREFIX);
    session.header_context.tools =
        format!("{}allow 11 · prompt 7 · deny 0 · extras extras extras", ui::HEADER_TOOLS_PREFIX);
    session.header_context.mcp = format!("{}enabled", ui::HEADER_MCP_PREFIX);
    session.header_context.highlights = vec![InlineHeaderHighlight {
        title: "Tips".to_string(),
        lines: vec![
            "- Use /prompt:quick-start for boilerplate".to_string(),
            "- Keep responses focused".to_string(),
        ],
    }];
    session.input_manager.set_content("notes".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    let wide = session.header_height_for_width(120);
    let narrow = session.header_height_for_width(40);

    assert!(narrow >= wide, "expected narrower width to require at least as many header rows");
    assert!(
        wide >= ui::INLINE_HEADER_HEIGHT && narrow >= ui::INLINE_HEADER_HEIGHT,
        "expected header rows to meet minimum height"
    );
}
#[test]
fn agent_label_uses_accent_color_without_border() {
    let accent = AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56));
    let theme = InlineTheme { primary: Some(accent), ..Default::default() };

    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.labels.agent = Some("Agent".to_string());
    let mut segment = make_segment("Response");
    segment.style = Arc::new(InlineTextStyle { color: Some(accent), ..InlineTextStyle::default() });
    session.push_line(InlineMessageKind::Agent, vec![segment]);

    let index = session.lines.len().checked_sub(1).expect("agent message should be available");
    let spans = session.render_message_spans(index);

    assert!(!spans.is_empty());

    // Agent prose has no bullet gap; the optional label leads when configured.
    assert!(
        !spans.iter().any(|span| span.content.as_ref().contains('•')),
        "agent prose must not render a bullet gap"
    );

    let label_index = spans
        .iter()
        .position(|span| span.content.clone().into_owned() == "Agent")
        .expect("agent label span should be present");
    let label_span = &spans[label_index];
    assert_eq!(label_span.style.fg, Some(Color::Rgb(0x12, 0x34, 0x56)));

    let padding_span = spans.get(label_index + 1).expect("agent label should be followed by padding");
    assert_eq!(padding_span.content.clone().into_owned(), ui::INLINE_AGENT_MESSAGE_LEFT_PADDING);

    assert!(
        !spans.iter().any(|span| span.content.clone().into_owned().contains('│')),
        "agent prefix should not render a left border",
    );
    assert!(
        !spans.iter().any(|span| span.content.clone().into_owned().contains('✦')),
        "agent prefix should not include decorative symbols",
    );
}
#[test]
fn header_blocked_badge_uses_theme_error_style() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Blocked));

    let error_inline = vtcode_commons::ui_protocol::convert_style(crate::theme::active_styles().error);
    let expected = ratatui_style_from_inline(&error_inline, None);
    let lines = session.header_lines();
    let badge = lines[0]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "Blocked")
        .expect("blocked badge span should be present");

    assert_eq!(badge.style, expected, "badge must use the theme alert token, not a hardcoded color");
    assert!(matches!(badge.style.fg, Some(Color::Rgb(_, _, _))), "badge color must come from the theme palette");
}
#[test]
fn cached_header_tracks_activity_and_status_transitions() {
    let mut session = fresh_session();
    let initial = header_line_text(&mut session);
    assert!(!initial.contains("Blocked"));

    session.handle_command(InlineCommand::SetActivityState(ActivityState::Blocked));
    assert!(header_line_text(&mut session).contains("Blocked"));

    session.handle_command(InlineCommand::SetActivityState(ActivityState::Idle));
    assert!(!header_line_text(&mut session).contains("Blocked"));

    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: Some("[BLOCKED] waiting for approval".to_string()),
    });
    assert!(header_line_text(&mut session).contains("Blocked"));

    session.handle_command(InlineCommand::SetInputStatus { left: None, right: None });
    assert!(!header_line_text(&mut session).contains("Blocked"));
}
#[test]
fn header_shows_blocked_badge_from_blocked_status_needle() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: Some("[BLOCKED] 84% context left".to_string()),
    });

    assert_header_contains_badge(&mut session, "Blocked");
}
#[test]
fn header_hides_blocked_badge_without_blocked_signals() {
    let mut session = fresh_session();
    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: Some("84% context left".to_string()),
    });

    let text = header_line_text(&mut session);
    assert!(!text.contains("Blocked"), "header should not show blocked badge, got: {text}");
}
