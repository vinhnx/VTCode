use super::super::*;
use super::helpers::*;
use crate::core_tui::session::input;
use std::time::Instant;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn fresh_session() -> Session {
    Session::new(InlineTheme::default(), None, VIEW_ROWS)
}

fn header_line_text(session: &mut Session) -> String {
    let lines = session.header_lines();
    assert_eq!(lines.len(), 1, "expected single-line header");
    lines[0]
        .spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect()
}

fn assert_header_contains_badge(session: &mut Session, badge_text: &str) {
    let text = header_line_text(session);
    assert!(
        text.contains(badge_text),
        "header should contain badge '{badge_text}', got: {text}"
    );
}

fn assert_badge_style(line: &Line<'_>, badge_text: &str, expected_fg: Color) {
    let badge_span = line
        .spans
        .iter()
        .find(|span| span.content.as_ref() == badge_text)
        .unwrap_or_else(|| panic!("badge span '{badge_text}' not found"));
    assert_eq!(badge_span.style.fg, Some(expected_fg));
    assert!(badge_span.style.add_modifier.contains(Modifier::BOLD));
}

fn setup_shimmer_session(session: &mut Session, left_status: &str) {
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some(left_status.to_string()),
        right: None,
    });
}

fn clear_shimmer_session(session: &mut Session) {
    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: None,
    });
}

fn input_data(session: &mut Session) -> input::InputWidgetData {
    session.build_input_widget_data(VIEW_WIDTH, 1)
}

fn session_with_highlights(highlights: Vec<InlineHeaderHighlight>) -> Session {
    let mut session = fresh_session();
    session.header_context.highlights = highlights;
    session.input_manager.set_content("notes".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());
    session
}

#[test]
fn copy_notification_renders_in_input_status_line() {
    let mut session = fresh_session();
    session.show_copy_notification();

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .expect("input status line")
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("Copied to clipboard"));
}

#[test]
fn copy_notification_expires_after_five_seconds() {
    let mut session = fresh_session();
    session.show_copy_notification();
    session.copy_notification_until = Some(Instant::now() - Duration::from_secs(1));
    session.handle_tick();

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .unwrap_or_default();

    assert!(!rendered.contains("Copied to clipboard"));
}

#[test]
fn cursor_visible_while_scrolling() {
    let mut session = fresh_session();
    let initial = input_data(&mut session);
    assert!(initial.cursor_should_be_visible);

    session.scroll_line_down();
    let during_scroll = input_data(&mut session);
    assert!(during_scroll.cursor_should_be_visible);
    assert!(session.use_steady_cursor());

    session.scroll_cursor_steady_until = Some(Instant::now() - Duration::from_millis(1));
    session.handle_tick();

    assert!(!session.use_steady_cursor());
    let after_scroll = input_data(&mut session);
    assert!(after_scroll.cursor_should_be_visible);
}

#[test]
fn cursor_steady_during_shimmer() {
    let mut session = fresh_session();
    let initial = input_data(&mut session);
    assert!(initial.cursor_should_be_visible);
    assert!(!session.use_steady_cursor());

    setup_shimmer_session(&mut session, "Running command: test");
    let during_shimmer = input_data(&mut session);
    assert!(during_shimmer.cursor_should_be_visible);
    assert!(session.use_steady_cursor());

    clear_shimmer_session(&mut session);
    assert!(!session.use_steady_cursor());
    let after_shimmer = input_data(&mut session);
    assert!(after_shimmer.cursor_should_be_visible);
}

#[test]
fn cursor_fake_during_status_shimmer() {
    let mut session = fresh_session();
    let initial = input_data(&mut session);
    assert!(initial.cursor_should_be_visible);
    assert!(!initial.use_fake_cursor);

    setup_shimmer_session(&mut session, "Loading (Esc, Ctrl+C, or /stop to stop)");
    let during_shimmer = input_data(&mut session);
    assert!(during_shimmer.cursor_should_be_visible);
    assert!(during_shimmer.use_fake_cursor);
    assert!(session.use_steady_cursor());

    clear_shimmer_session(&mut session);
    let after_shimmer = input_data(&mut session);
    assert!(after_shimmer.cursor_should_be_visible);
    assert!(!after_shimmer.use_fake_cursor);
}

#[test]
fn bang_prefix_input_shows_shell_mode_status_hint() {
    let mut session = fresh_session();
    session.set_input("!echo hello".to_string());

    let spans = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .expect("expected shell mode status hint");
    let rendered: String = spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect();

    assert!(rendered.contains("Shell mode (!):"));
}

#[test]
fn bang_prefix_input_enables_shell_mode_border_title() {
    let mut session = fresh_session();
    session.set_input("   !ls -la".to_string());

    assert_eq!(session.shell_mode_border_title(), Some(" ! Shell mode "));
}

#[test]
fn bang_prefix_input_uses_zero_padding_for_visible_input_area() {
    let mut session = fresh_session();
    session.set_input("!echo hello".to_string());

    assert_eq!(
        session.input_block_padding(),
        ratatui::widgets::Padding::new(0, 0, 0, 0)
    );
}

#[test]
fn non_bang_input_has_no_shell_mode_border_title() {
    let mut session = fresh_session();
    session.set_input("run ls -la".to_string());

    assert_eq!(session.shell_mode_border_title(), None);
}

#[test]
fn non_bang_input_uses_default_padding() {
    let mut session = fresh_session();
    session.set_input("run ls -la".to_string());

    assert_eq!(
        session.input_block_padding(),
        ratatui::widgets::Padding::new(
            ui::INLINE_INPUT_PADDING_HORIZONTAL,
            ui::INLINE_INPUT_PADDING_HORIZONTAL,
            ui::INLINE_INPUT_PADDING_VERTICAL,
            ui::INLINE_INPUT_PADDING_VERTICAL,
        )
    );
}

#[test]
fn question_mark_opens_help_overlay_when_input_is_empty() {
    let mut session = fresh_session();

    let result = session.process_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    assert!(result.is_none());
    let modal = session.modal_state().expect("help modal should open");
    assert_eq!(modal.title, "Keyboard Shortcuts");
    assert!(
        modal
            .lines
            .iter()
            .any(|line| line.contains("Ctrl+A / Ctrl+E"))
    );
}

#[test]
fn question_mark_inserts_character_when_input_has_content() {
    let mut session = session_with_input("why", 3);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "why?");
    assert_eq!(session.cursor(), 4);
    assert!(session.modal_state().is_none());
}

#[test]
fn header_shows_safe_badge_for_tools_policy_trust() {
    let mut session = fresh_session();
    session.header_context.workspace_trust = format!("{}tools policy", ui::HEADER_TRUST_PREFIX);
    session.input_manager.set_content("test".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

    assert_header_contains_badge(&mut session, "Safe");
}

#[test]
fn header_shows_full_auto_trust_badge_for_full_auto_trust() {
    let mut session = fresh_session();
    session.header_context.workspace_trust = format!("{}full auto", ui::HEADER_TRUST_PREFIX);
    session.input_manager.set_content("test".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

    assert_header_contains_badge(&mut session, "Full-auto");
}

#[test]
fn header_shows_auto_badge() {
    let mut session = fresh_session();
    session.header_context.autonomous_mode = true;
    session.header_context.workspace_trust = format!("{}tools policy", ui::HEADER_TRUST_PREFIX);
    session.input_manager.set_content("test".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

    let text = header_line_text(&mut session);
    assert!(text.contains("Auto"));
    assert!(text.contains("Safe"));
}

#[test]
fn header_shows_pr_review_status_badge() {
    let mut session = fresh_session();
    session.header_context.pr_review = Some(InlineHeaderStatusBadge {
        text: "PR: outdated".to_string(),
        tone: InlineHeaderStatusTone::Warning,
    });

    let line = session.header_meta_line();
    assert_badge_style(&line, "PR: outdated", Color::Yellow);
}

#[test]
fn header_shows_persistent_memory_status_badge() {
    let mut session = fresh_session();
    session.header_context.persistent_memory = Some(InlineHeaderStatusBadge {
        text: "Memory: cleanup".to_string(),
        tone: InlineHeaderStatusTone::Warning,
    });

    let line = session.header_meta_line();
    assert_badge_style(&line, "Memory: cleanup", Color::Yellow);
}

#[test]
fn header_meta_line_excludes_editor_context() {
    let mut session = fresh_session();
    session.header_context.editor_context =
        Some("File: src/main.rs · Rust · Sel 120-148".to_string());

    let line = session.header_meta_line();
    let summary = line_text(&line);

    assert!(!summary.contains("File: src/main.rs"));
}

#[test]
fn header_title_line_shows_model_context_window() {
    let mut session = fresh_session();
    session.header_context.provider = format!("{}Anthropic", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}claude-sonnet-4-6", ui::HEADER_MODEL_PREFIX);
    session.header_context.context_window_size = Some(1_000_000);

    let summary = line_text(&session.header_title_line());
    assert!(summary.contains("claude-sonnet-4-6 (1M)"));

    session.header_context.model = format!("{}claude-haiku-4-5", ui::HEADER_MODEL_PREFIX);
    session.header_context.context_window_size = Some(200_000);

    let summary = line_text(&session.header_title_line());
    assert!(summary.contains("claude-haiku-4-5 (200K)"));
}

#[test]
fn header_highlights_collapse_to_single_line() {
    let mut session = session_with_highlights(vec![
        InlineHeaderHighlight {
            title: "Keyboard Shortcuts".to_string(),
            lines: vec![
                "/help Show help".to_string(),
                "Enter Submit message".to_string(),
            ],
        },
        InlineHeaderHighlight {
            title: "Usage Tips".to_string(),
            lines: vec!["- Keep tasks focused".to_string()],
        },
    ]);

    let lines = session.header_lines();
    assert_eq!(lines.len(), 1);

    let summary: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect();

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

    let summary: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect();

    let expected_preview = format!(
        "{}{}",
        "A".repeat(limit.saturating_sub(1)),
        ui::INLINE_PREVIEW_ELLIPSIS
    );

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

    let summary: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect();

    assert!(summary.contains("/{command}"));
    assert!(summary.contains("(+3 more)"));
    assert!(!summary.contains("Escape"));
    assert!(!summary.contains(ui::INLINE_PREVIEW_ELLIPSIS));
}

#[test]
fn header_height_expands_when_wrapping_required() {
    let mut session = fresh_session();
    session.header_context.provider = format!(
        "{}Example Provider With Extended Label",
        ui::HEADER_PROVIDER_PREFIX
    );
    session.header_context.model = format!(
        "{}ExampleModelIdentifierWithDetail",
        ui::HEADER_MODEL_PREFIX
    );
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);
    session.header_context.mode = ui::HEADER_MODE_AUTO.to_string();
    session.header_context.workspace_trust = format!("{}full auto", ui::HEADER_TRUST_PREFIX);
    session.header_context.tools = format!(
        "{}allow 11 · prompt 7 · deny 0 · extras extras extras",
        ui::HEADER_TOOLS_PREFIX
    );
    session.header_context.mcp = format!("{}enabled", ui::HEADER_MCP_PREFIX);
    session.header_context.highlights = vec![InlineHeaderHighlight {
        title: "Tips".to_string(),
        lines: vec![
            "- Use /prompt:quick-start for boilerplate".to_string(),
            "- Keep responses focused".to_string(),
        ],
    }];
    session.input_manager.set_content("notes".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

    let wide = session.header_height_for_width(120);
    let narrow = session.header_height_for_width(40);

    assert!(
        narrow >= wide,
        "expected narrower width to require at least as many header rows"
    );
    assert!(
        wide >= ui::INLINE_HEADER_HEIGHT && narrow >= ui::INLINE_HEADER_HEIGHT,
        "expected header rows to meet minimum height"
    );
}

#[test]
fn agent_label_uses_accent_color_without_border() {
    let accent = AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56));
    let theme = InlineTheme {
        primary: Some(accent),
        ..Default::default()
    };

    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.labels.agent = Some("Agent".to_string());
    let mut segment = make_segment("Response");
    segment.style = Arc::new(InlineTextStyle {
        color: Some(accent),
        ..InlineTextStyle::default()
    });
    session.push_line(InlineMessageKind::Agent, vec![segment]);

    let index = session
        .lines
        .len()
        .checked_sub(1)
        .expect("agent message should be available");
    let spans = session.render_message_spans(index);

    assert!(!spans.is_empty());

    let prefix_span = &spans[0];
    assert_eq!(
        prefix_span.content.clone().into_owned(),
        ui::INLINE_AGENT_QUOTE_PREFIX
    );

    let label_index = spans
        .iter()
        .position(|span| span.content.clone().into_owned() == "Agent")
        .expect("agent label span should be present");
    let label_span = &spans[label_index];
    assert_eq!(label_span.style.fg, Some(Color::Rgb(0x12, 0x34, 0x56)));

    let padding_span = spans
        .get(label_index + 1)
        .expect("agent label should be followed by padding");
    assert_eq!(
        padding_span.content.clone().into_owned(),
        ui::INLINE_AGENT_MESSAGE_LEFT_PADDING
    );

    assert!(
        !spans
            .iter()
            .any(|span| span.content.clone().into_owned().contains('│')),
        "agent prefix should not render a left border",
    );
    assert!(
        !spans
            .iter()
            .any(|span| span.content.clone().into_owned().contains('✦')),
        "agent prefix should not include decorative symbols",
    );
}
