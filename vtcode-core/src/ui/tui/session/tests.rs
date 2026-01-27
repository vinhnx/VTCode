use super::prompt_palette;
use super::*;
use crate::ui::tui::style::ratatui_style_from_inline;
use crate::ui::tui::{InlineSegment, InlineTextStyle, InlineTheme};
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier},
    text::{Line, Span},
};
use tokio::sync::mpsc;

const VIEW_ROWS: u16 = 14;
const VIEW_WIDTH: u16 = 100;
const LINE_COUNT: usize = 10;
const LABEL_PREFIX: &str = "line";
const EXTRA_SEGMENT: &str = "\nextra-line";

fn make_segment(text: &str) -> InlineSegment {
    InlineSegment {
        text: text.to_string(),
        style: std::sync::Arc::new(InlineTextStyle::default()),
    }
}

fn themed_inline_colors() -> InlineTheme {
    let mut theme = InlineTheme::default();
    theme.foreground = Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE)));
    theme.tool_accent = Some(AnsiColorEnum::Rgb(RgbColor(0xBF, 0x45, 0x45)));
    theme.tool_body = Some(AnsiColorEnum::Rgb(RgbColor(0xAA, 0x88, 0x88)));
    theme.primary = Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x88, 0x88)));
    theme.secondary = Some(AnsiColorEnum::Rgb(RgbColor(0x77, 0x99, 0xAA)));
    theme
}

fn session_with_input(input: &str, cursor: usize) -> Session {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input(input.to_string());
    session.set_cursor(cursor);
    session
}

fn visible_transcript(session: &mut Session) -> Vec<String> {
    let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render test session");

    let width = session.transcript_width;
    let viewport = session.viewport_height();
    let offset = session.transcript_view_top;
    let lines = session.reflow_transcript_lines(width);

    let start = offset.min(lines.len());
    let mut visible: Vec<Line<'static>> = lines.into_iter().skip(start).take(viewport).collect();
    let filler = viewport.saturating_sub(visible.len());
    if filler > 0 {
        visible.extend((0..filler).map(|_| Line::default()));
    }
    session.overlay_queue_lines(&mut visible, width);

    visible
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect()
}

#[test]
fn move_left_word_from_end_moves_to_word_start() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    session.move_left_word();
    assert_eq!(session.input_manager.cursor(), 6);

    session.move_left_word();
    assert_eq!(session.input_manager.cursor(), 0);
}

#[test]
fn move_left_word_skips_trailing_whitespace() {
    let text = "hello  world";
    let mut session = session_with_input(text, text.len());

    session.move_left_word();
    assert_eq!(session.input_manager.cursor(), 7);
}

#[test]
fn arrow_keys_navigate_input_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("first message".to_string());
    let submit_first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(submit_first, Some(InlineEvent::Submit(value)) if value == "first message"));

    session.set_input("second".to_string());
    let submit_second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(submit_second, Some(InlineEvent::Submit(value)) if value == "second"));

    assert_eq!(session.input_manager.history().len(), 2);
    assert!(session.input_manager.content().is_empty());

    let up_latest = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert!(up_latest.is_none());
    assert_eq!(session.input_manager.content(), "second");

    let up_previous = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert!(up_previous.is_none());
    assert_eq!(session.input_manager.content(), "first message");

    let down_forward = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert!(down_forward.is_none());
    assert_eq!(session.input_manager.content(), "second");

    let down_restore = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert!(down_restore.is_none());
    assert!(session.input_manager.content().is_empty());
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn shift_enter_inserts_newline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.input_manager.set_content("queued".to_string());

    let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "queued\n");
    assert_eq!(
        session.input_manager.cursor(),
        session.input_manager.content().len()
    );
}

#[test]
fn paste_preserves_all_newlines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = (0..15)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let (tx, _rx) = mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::Paste(pasted.clone()), &tx, None);

    assert_eq!(session.input_manager.content(), pasted);
}

#[test]
fn selecting_prompt_via_palette_updates_input_with_slash_command() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let mut palette = PromptPalette::new();
    palette.append_entries(vec![prompt_palette::PromptEntry {
        name: "vtcode".to_string(),
        description: String::new(),
    }]);
    session.prompt_palette = Some(palette);
    session.prompt_palette_active = true;
    session.set_input("#vt".to_string());

    let handled =
        session.handle_prompt_palette_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(handled);

    assert_eq!(session.input_manager.content(), "/prompt:vtcode ");
    assert_eq!(
        session.input_manager.cursor(),
        session.input_manager.content().len()
    );
    assert!(!session.prompt_palette_active);
}

#[test]
fn control_enter_queues_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
}

#[test]
fn command_enter_queues_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SUPER));
    assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
}

#[test]
fn consecutive_duplicate_submissions_not_stored_twice() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("repeat".to_string());
    let first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Submit(value)) if value == "repeat"));

    session.set_input("repeat".to_string());
    let second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(second, Some(InlineEvent::Submit(value)) if value == "repeat"));

    assert_eq!(session.input_manager.history().len(), 1);
}

#[test]
fn alt_arrow_left_moves_cursor_by_word() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let event = KeyEvent::new(KeyCode::Left, KeyModifiers::ALT);
    session.process_key(event);

    assert_eq!(session.cursor(), 6);
}

#[test]
fn alt_b_moves_cursor_by_word() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let event = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT);
    session.process_key(event);

    assert_eq!(session.cursor(), 6);
}

#[test]
fn move_right_word_advances_to_word_boundaries() {
    let text = "hello  world";
    let mut session = session_with_input(text, 0);

    session.move_right_word();
    assert_eq!(session.cursor(), 5);

    session.move_right_word();
    assert_eq!(session.cursor(), 7);

    session.move_right_word();
    assert_eq!(session.cursor(), text.len());
}

#[test]
fn move_right_word_from_whitespace_moves_to_next_word_start() {
    let text = "hello  world";
    let mut session = session_with_input(text, 5);

    session.move_right_word();
    assert_eq!(session.cursor(), 7);
}

#[test]
fn super_arrow_right_moves_cursor_to_end() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let event = KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER);
    let result = session.process_key(event);

    assert_eq!(session.cursor(), text.len());
    // Ensure Command+Right does NOT launch editor
    assert!(!matches!(result, Some(InlineEvent::LaunchEditor)));
}

#[test]
fn super_a_moves_cursor_to_start() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SUPER);
    session.process_key(event);

    assert_eq!(session.cursor(), 0);
}

#[test]
fn super_e_moves_cursor_to_end() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::SUPER);
    let result = session.process_key(event);

    // Should move to end and return None (no event)
    assert!(result.is_none());
    assert_eq!(session.cursor(), text.len());
}

#[test]
fn control_e_does_not_launch_editor() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    // Control+E keybinding has been removed - use /edit command instead
    assert!(!matches!(result, Some(InlineEvent::LaunchEditor)));
}

#[test]
fn control_super_e_does_not_launch_editor() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let event = KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::SUPER,
    );
    let result = session.process_key(event);

    // Should not launch editor when both Control and Super (Cmd) are pressed
    assert!(!matches!(result, Some(InlineEvent::LaunchEditor)));
}

#[test]
fn arrow_keys_never_launch_editor() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    // Test Right arrow with all possible modifier combinations
    for modifiers in [
        KeyModifiers::empty(),
        KeyModifiers::CONTROL,
        KeyModifiers::SHIFT,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::META,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        KeyModifiers::CONTROL | KeyModifiers::SUPER,
    ] {
        let event = KeyEvent::new(KeyCode::Right, modifiers);
        let result = session.process_key(event);
        assert!(
            !matches!(result, Some(InlineEvent::LaunchEditor)),
            "Right arrow with modifiers {:?} should not launch editor",
            modifiers
        );
    }

    // Test other arrow keys for safety
    for key_code in [KeyCode::Left, KeyCode::Up, KeyCode::Down] {
        let event = KeyEvent::new(key_code, KeyModifiers::SUPER);
        let result = session.process_key(event);
        assert!(
            !matches!(result, Some(InlineEvent::LaunchEditor)),
            "{:?} with SUPER should not launch editor",
            key_code
        );
    }
}

#[test]
fn streaming_new_lines_preserves_scrolled_view() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 1..=LINE_COUNT {
        let label = format!("{LABEL_PREFIX}-{index}");
        session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
    }

    session.scroll_page_up();
    let before = visible_transcript(&mut session);

    session.append_inline(InlineMessageKind::Agent, make_segment(EXTRA_SEGMENT));

    let after = visible_transcript(&mut session);
    assert_eq!(before.len(), after.len());
    assert!(
        after.iter().all(|line| !line.contains("extra-line")),
        "appended lines should not appear when scrolled up"
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

    let mut transcripts = Vec::new();
    let mut iterations = 0;
    loop {
        transcripts.push(visible_transcript(&mut session));
        let previous_offset = session.scroll_offset();
        session.scroll_page_up();
        if session.scroll_offset() == previous_offset {
            break;
        }
        iterations += 1;
        assert!(
            iterations <= LINE_COUNT,
            "scroll_page_up did not converge within expected bounds"
        );
    }

    assert!(transcripts.len() > 1);

    for window in transcripts.windows(2) {
        assert_ne!(window[0], window[1]);
    }

    let top_view = transcripts
        .last()
        .expect("a top-of-buffer page should exist after scrolling");
    let first_label = format!("{LABEL_PREFIX}-1");
    let last_label = format!("{LABEL_PREFIX}-{LINE_COUNT}");

    assert!(top_view.iter().any(|line| line.contains(&first_label)));
    assert!(top_view.iter().all(|line| !line.contains(&last_label)));
    let scroll_offset = session.scroll_offset();
    let max_offset = session.current_max_scroll_offset();
    assert_eq!(scroll_offset, max_offset);
}

#[test]
fn resizing_viewport_clamps_scroll_offset() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 1..=LINE_COUNT {
        let label = format!("{LABEL_PREFIX}-{index}");
        session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
    }

    session.scroll_page_up();
    assert!(session.scroll_offset() > 0);

    session.force_view_rows(
        (LINE_COUNT as u16)
            + ui::INLINE_HEADER_HEIGHT
            + Session::input_block_height_for_lines(1)
            + 2,
    );

    assert_eq!(session.scroll_offset(), 0);
    let max_offset = session.current_max_scroll_offset();
    assert_eq!(max_offset, 0);
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
    assert!(
        view.last().map_or(false, |line| !line.is_empty()),
        "expected transcript to end with content, got {view:?}"
    );
}

#[test]
fn user_messages_render_with_dividers() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::User, vec![make_segment("Hi")]);

    let width = 10;
    let lines = session.reflow_transcript_lines(width);
    assert!(
        lines.len() >= 3,
        "expected dividers around the user message"
    );

    let top = match lines.first() {
        Some(line) => line_text(line),
        None => {
            tracing::error!("lines is empty despite assertion");
            return;
        }
    };
    let bottom = line_text(
        lines
            .last()
            .expect("user message should have closing divider"),
    );
    let expected = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(width as usize);

    assert_eq!(top, expected);
    assert_eq!(bottom, expected);
}

#[test]
fn header_lines_include_provider_model_and_metadata() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.provider = format!("{}xAI", ui::HEADER_PROVIDER_PREFIX);
    session.header_context.model = format!("{}grok-4-fast", ui::HEADER_MODEL_PREFIX);
    session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);
    session.header_context.mode = ui::HEADER_MODE_AUTO.to_string();
    session.header_context.workspace_trust = format!("{}full auto", ui::HEADER_TRUST_PREFIX);
    session.header_context.tools =
        format!("{}allow 11 · prompt 7 · deny 0", ui::HEADER_TOOLS_PREFIX);
    session.header_context.mcp = format!("{}enabled", ui::HEADER_MCP_PREFIX);
    session.input_manager.set_content("notes".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

    let lines = session.header_lines();
    assert_eq!(lines.len(), 1);

    let line_text: String = lines[0]
        .spans
        .iter()
        .map(|span| span.content.clone().into_owned())
        .collect();
    assert!(line_text.contains(&session.header_provider_short_value()));
    assert!(line_text.contains(&session.header_model_short_value()));
    let reasoning_label = format!("{}", session.header_reasoning_short_value());
    assert!(line_text.contains(&reasoning_label));
    let mode_label = session.header_mode_short_label();
    assert!(line_text.contains(&mode_label));
    for value in session.header_chain_values() {
        assert!(line_text.contains(&value));
    }
    // Removed assertion for HEADER_MCP_PREFIX since we're no longer showing MCP info in header
    assert!(!line_text.contains("Languages"));
    assert!(!line_text.contains(ui::HEADER_STATUS_LABEL));
    assert!(!line_text.contains(ui::HEADER_MESSAGES_LABEL));
    assert!(!line_text.contains(ui::HEADER_INPUT_LABEL));
}

#[test]
fn header_highlights_collapse_to_single_line() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.highlights = vec![
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
    ];
    session.input_manager.set_content("notes".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

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
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let limit = ui::HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS;
    let long_entry = "A".repeat(limit + 5);
    session.header_context.highlights = vec![InlineHeaderHighlight {
        title: "Details".to_string(),
        lines: vec![long_entry.clone()],
    }];
    session.input_manager.set_content("notes".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

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
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.highlights = vec![InlineHeaderHighlight {
        title: String::new(),
        lines: vec![
            "  - /{command}".to_string(),
            "  - /help Show slash command help".to_string(),
            "  - Enter Submit message".to_string(),
            "  - Escape Cancel input".to_string(),
        ],
    }];
    session.input_manager.set_content("notes".to_string());
    session
        .input_manager
        .set_cursor(session.input_manager.content().len());

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
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
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
fn agent_messages_include_left_padding() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("Response")]);

    let lines = session.reflow_transcript_lines(VIEW_WIDTH);
    let message_line = lines
        .iter()
        .map(line_text)
        .find(|text| text.contains("Response"))
        .expect("agent message should be visible");

    let expected_prefix = format!(
        "{}{}",
        ui::INLINE_AGENT_QUOTE_PREFIX,
        ui::INLINE_AGENT_MESSAGE_LEFT_PADDING
    );

    assert!(
        message_line.starts_with(&expected_prefix),
        "agent message should include left padding",
    );
    assert!(
        !message_line.contains('│'),
        "agent message should not render a left border",
    );
}

#[test]
fn wrap_line_splits_double_width_graphemes() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let style = session.default_style();
    let line = Line::from(vec![Span::styled(
        "你好世界".to_string(),
        ratatui_style_from_inline(&style, None),
    )]);

    let wrapped = session.wrap_line(line, 4);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["你好".to_string(), "世界".to_string()]);
}

#[test]
fn wrap_line_keeps_explicit_blank_rows() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let style = session.default_style();
    let line = Line::from(vec![Span::styled(
        "top\n\nbottom".to_string(),
        ratatui_style_from_inline(&style, None),
    )]);

    let wrapped = session.wrap_line(line, 40);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(
        rendered,
        vec!["top".to_string(), String::new(), "bottom".to_string()]
    );
}

#[test]
fn wrap_line_preserves_characters_wider_than_viewport() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let style = session.default_style();
    let line = Line::from(vec![Span::styled(
        "hi".to_string(),
        ratatui_style_from_inline(&style, None),
    )]);

    let wrapped = session.wrap_line(line, 1);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    assert_eq!(rendered, vec!["你".to_string()]);
}

#[test]
fn wrap_line_discards_carriage_return_before_newline() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let style = session.default_style();
    let line = Line::from(vec![Span::styled(
        "foo\r\nbar".to_string(),
        ratatui_style_from_inline(&style, None),
    )]);

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
            style: std::sync::Arc::new(InlineTextStyle::default()),
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
    assert_eq!(first_segment.text.as_str(), "fn demo() {}");
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

    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "first output".to_string(),
            style: std::sync::Arc::new(InlineTextStyle::default()),
        }],
    );

    let rendered = session.reflow_pty_lines(0, 80);
    assert!(rendered.iter().any(|line| !line.spans.is_empty()));
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
fn transcript_shows_content_when_viewport_smaller_than_padding() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 0..10 {
        let label = format!("{LABEL_PREFIX}-{index}");
        session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
    }

    let minimal_view_rows = ui::INLINE_HEADER_HEIGHT + Session::input_block_height_for_lines(1) + 1;
    session.force_view_rows(minimal_view_rows);

    let view = visible_transcript(&mut session);
    assert!(
        view.iter()
            .any(|line| line.contains(&format!("{LABEL_PREFIX}-9"))),
        "expected most recent transcript line to remain visible even when viewport is small"
    );
}

#[test]
fn pty_scroll_preserves_order() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    for index in 0..200 {
        let label = format!("{LABEL_PREFIX}-{index}");
        session.push_line(
            InlineMessageKind::Pty,
            vec![InlineSegment {
                text: label,
                style: std::sync::Arc::new(InlineTextStyle::default()),
            }],
        );
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
        top_view
            .iter()
            .any(|line| line.contains(&format!("{LABEL_PREFIX}-0"))),
        "top view should include earliest PTY line"
    );
    assert!(
        top_view
            .iter()
            .all(|line| !line.contains(&format!("{LABEL_PREFIX}-199"))),
        "top view should not include latest PTY line"
    );
}

#[test]
fn agent_label_uses_accent_color_without_border() {
    let accent = AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56));
    let mut theme = InlineTheme::default();
    theme.primary = Some(accent);

    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.labels.agent = Some("Agent".to_string());
    session.push_line(InlineMessageKind::Agent, vec![make_segment("Response")]);

    let index = session
        .lines
        .len()
        .checked_sub(1)
        .expect("agent message should be available");
    let spans = session.render_message_spans(index);

    assert!(spans.len() >= 3);

    let label_span = &spans[0];
    assert_eq!(label_span.content.clone().into_owned(), "Agent");
    assert_eq!(label_span.style.fg, Some(Color::Rgb(0x12, 0x34, 0x56)));

    let padding_span = &spans[1];
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

#[test]
fn timeline_hidden_keeps_navigation_unselected() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("Response")]);

    let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session with hidden timeline");

    assert!(session.navigation_state.selected().is_none());
}

#[test]
fn queued_inputs_overlay_bottom_rows() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Agent,
        vec![make_segment("Latest response from agent")],
    );

    session.handle_command(InlineCommand::SetQueuedInputs {
        entries: vec![
            "first queued message".to_string(),
            "second queued message".to_string(),
            "third queued message".to_string(),
        ],
    });

    let view = visible_transcript(&mut session);
    let footer: Vec<String> = view.iter().rev().take(10).cloned().collect();

    assert!(
        footer.iter().any(|line| line.contains("Follow-ups (3)")),
        "follow-up header should be visible at the bottom of the transcript"
    );
    assert!(
        footer.iter().any(|line| line.contains("1.")),
        "first queued message label should be rendered"
    );
    assert!(
        footer.iter().any(|line| line.contains("2.")),
        "second queued message label should be rendered"
    );
    assert!(
        footer.iter().any(|line| line.contains("3.")),
        "third queued message label should be rendered"
    );
    assert!(
        footer
            .iter()
            .any(|line| line.contains("Ctrl+↑ to edit queue")),
        "hint line should show how to edit queue"
    );
}

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

    assert_eq!(session.navigation_state.selected(), Some(1));
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
            style: std::sync::Arc::new(detail_style),
        }],
    );

    let index = session
        .lines
        .len()
        .checked_sub(1)
        .expect("tool detail line should exist");
    let spans = session.render_message_spans(index);

    assert_eq!(spans.len(), 1);
    let body_span = &spans[0];
    assert!(body_span.style.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(body_span.content.clone().into_owned(), "result line");
}
