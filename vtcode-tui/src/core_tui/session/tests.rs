use super::*;
use crate::config::constants::ui;
use crate::ui::tui::style::ratatui_style_from_inline;
use crate::ui::tui::{
    DiffHunk, DiffOverlayRequest, DiffPreviewMode, DiffPreviewState, InlineListItem,
    InlineListSearchConfig, InlineListSelection, InlineSegment, InlineTextStyle, InlineTheme,
    ListOverlayRequest, OverlayEvent, OverlayHotkey, OverlayHotkeyAction, OverlayHotkeyKey,
    OverlayRequest, OverlaySubmission, SlashCommandItem, WizardModalMode, WizardOverlayRequest,
    WizardStep,
};
use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier},
    text::{Line, Span},
};
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const VIEW_ROWS: u16 = 14;
const VIEW_WIDTH: u16 = 100;
const LINE_COUNT: usize = 10;
const LABEL_PREFIX: &str = "line";
const EXTRA_SEGMENT: &str = "\nextra-line";

fn make_segment(text: &str) -> InlineSegment {
    InlineSegment {
        text: text.to_string(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn themed_inline_colors() -> InlineTheme {
    InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        tool_accent: Some(AnsiColorEnum::Rgb(RgbColor(0xBF, 0x45, 0x45))),
        tool_body: Some(AnsiColorEnum::Rgb(RgbColor(0xAA, 0x88, 0x88))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x88, 0x88))),
        secondary: Some(AnsiColorEnum::Rgb(RgbColor(0x77, 0x99, 0xAA))),
        ..Default::default()
    }
}

fn session_with_input(input: &str, cursor: usize) -> Session {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input(input.to_string());
    session.set_cursor(cursor);
    session
}

fn session_with_slash_palette_commands() -> Session {
    Session::new_with_logs(
        InlineTheme::default(),
        None,
        VIEW_ROWS,
        true,
        None,
        vec![
            SlashCommandItem::new("new", "Start a new session"),
            SlashCommandItem::new("review", "Review current diff"),
            SlashCommandItem::new("doctor", "Run diagnostics"),
            SlashCommandItem::new("command", "Run a terminal command"),
            SlashCommandItem::new("files", "Browse files"),
        ],
        "Agent TUI".to_string(),
    )
}

fn show_diff_overlay(session: &mut Session, mode: DiffPreviewMode) {
    session.show_overlay(OverlayRequest::Diff(DiffOverlayRequest {
        file_path: "src/main.rs".to_string(),
        before: "fn old() {}\n".to_string(),
        after: "fn new() {}\n".to_string(),
        hunks: vec![DiffHunk {
            old_start: 0,
            new_start: 0,
            old_lines: 1,
            new_lines: 1,
            display: "@@ -1 +1 @@".to_string(),
        }],
        current_hunk: 0,
        mode,
    }));
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
    if !session.queued_inputs.is_empty() {
        session.overlay_queue_lines(&mut visible, width);
    }
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

fn text_content(text: &Text<'static>) -> String {
    text.lines
        .iter()
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n")
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
fn file_palette_insertion_uses_at_alias_in_input() {
    let mut session = session_with_input("check @mai", "check @mai".len());

    session.insert_file_reference("src/main.rs");

    assert_eq!(session.input_manager.content(), "check @src/main.rs ");
    assert_eq!(session.cursor(), "check @src/main.rs ".len());
}

#[test]
fn set_input_command_activates_file_palette_for_at_query() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::LoadFilePalette {
        files: vec!["src/main.rs".to_string()],
        workspace: PathBuf::from("."),
    });

    assert!(!session.file_palette_active);
    session.handle_command(InlineCommand::SetInput("@src".to_string()));
    assert!(session.file_palette_active);
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
    assert!(matches!(up_latest, Some(InlineEvent::HistoryPrevious)));
    assert_eq!(session.input_manager.content(), "second");

    let up_previous = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert!(matches!(up_previous, Some(InlineEvent::HistoryPrevious)));
    assert_eq!(session.input_manager.content(), "first message");

    let down_forward = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert!(matches!(down_forward, Some(InlineEvent::HistoryNext)));
    assert!(session.input_manager.content().is_empty());
    assert!(session.input_manager.history_index().is_none());

    let down_restore = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert!(down_restore.is_none());
    assert!(session.input_manager.content().is_empty());
    assert!(session.input_manager.history_index().is_none());
}

#[test]
fn cursor_visible_while_scrolling() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let initial = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(initial.cursor_should_be_visible);

    session.scroll_line_down();
    let during_scroll = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(during_scroll.cursor_should_be_visible);
    assert!(session.use_steady_cursor());

    session.scroll_cursor_steady_until = Some(Instant::now() - Duration::from_millis(1));
    session.handle_tick();

    assert!(!session.use_steady_cursor());
    let after_scroll = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(after_scroll.cursor_should_be_visible);
}

#[test]
fn cursor_steady_during_shimmer() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let initial = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(initial.cursor_should_be_visible);
    assert!(!session.use_steady_cursor());

    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });
    let during_shimmer = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(during_shimmer.cursor_should_be_visible);
    assert!(session.use_steady_cursor());

    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: None,
    });
    assert!(!session.use_steady_cursor());
    let after_shimmer = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(after_shimmer.cursor_should_be_visible);
}

#[test]
fn cursor_fake_during_status_shimmer() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let initial = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(initial.cursor_should_be_visible);
    assert!(!initial.use_fake_cursor);

    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Loading (Press Ctrl+C to cancel)".to_string()),
        right: None,
    });
    let during_shimmer = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(during_shimmer.cursor_should_be_visible);
    assert!(during_shimmer.use_fake_cursor);
    assert!(session.use_steady_cursor());

    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: None,
    });
    let after_shimmer = session.build_input_widget_data(VIEW_WIDTH, 1);
    assert!(after_shimmer.cursor_should_be_visible);
    assert!(!after_shimmer.use_fake_cursor);
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
fn pasted_message_displays_full_content() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("paste-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.append_pasted_message(
        InlineMessageKind::User,
        pasted_text.clone(),
        pasted_lines.len(),
    );

    let user_line = session
        .lines
        .iter()
        .find(|line| line.kind == InlineMessageKind::User)
        .expect("user line should exist");
    let combined: String = user_line
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    assert!(combined.contains("paste-1"));
    assert!(session.collapsed_pastes.is_empty());
}

#[test]
fn input_compact_preview_for_large_paste() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.insert_paste_text(&pasted_text);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"));
}

#[test]
fn input_compact_preview_for_image_path() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let image_path = "/tmp/Screenshot 2026-02-06 at 3.39.48 PM.png";

    session.insert_paste_text(image_path);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image:"));
    assert!(rendered.contains("Screenshot 2026-02-06"));
}

#[test]
fn input_compact_preview_for_quoted_image_path() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let image_path = "\"/tmp/Screenshot 2026-02-06 at 3.39.48 PM.png\"";

    session.insert_paste_text(image_path);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image:"));
    assert!(rendered.contains("Screenshot 2026-02-06"));
}

#[test]
fn input_compact_preview_for_image_path_with_text() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let input = "/tmp/Screenshot 2026-02-06 at 3.39.48 PM.png can you see";

    session.insert_paste_text(input);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image:"));
    assert!(rendered.contains("Screenshot 2026-02-06"));
    assert!(rendered.contains("can you see"));
}

#[test]
fn bang_prefix_input_shows_shell_mode_status_hint() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
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
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("   !ls -la".to_string());

    assert_eq!(session.shell_mode_border_title(), Some(" ! Shell mode "));
}

#[test]
fn bang_prefix_input_uses_zero_padding_for_visible_input_area() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("!echo hello".to_string());

    assert_eq!(
        session.input_block_padding(),
        ratatui::widgets::Padding::new(0, 0, 0, 0)
    );
}

#[test]
fn non_bang_input_has_no_shell_mode_border_title() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("run ls -la".to_string());

    assert_eq!(session.shell_mode_border_title(), None);
}

#[test]
fn non_bang_input_uses_default_padding() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
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
fn control_enter_queues_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
}

#[test]
fn control_l_submits_clear_command() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let event = session.process_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "/clear"));
}

#[test]
fn control_slash_toggles_inline_list_visibility() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
}

#[test]
fn control_i_toggles_inline_list_visibility() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
}

#[test]
fn tab_queues_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let queued = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
}

#[test]
fn double_escape_interrupts_when_running_activity() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(
        matches!(
            first,
            Some(InlineEvent::Cancel) | Some(InlineEvent::ForceCancelPtySession)
        ) || first.is_none()
    );

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(second, Some(InlineEvent::Interrupt)));
}

#[test]
fn double_escape_submits_rewind_when_idle() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let _ = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(matches!(second, Some(InlineEvent::Submit(value)) if value == "/rewind"));
}

#[test]
fn slash_palette_enter_submits_immediate_command() {
    let mut session = session_with_slash_palette_commands();

    for key in [
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
    ] {
        let event = session.process_key(key);
        assert!(event.is_none());
    }

    let submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(submit, Some(InlineEvent::Submit(value)) if value.trim() == "/new"));
}

#[test]
fn slash_palette_enter_submits_review_immediately() {
    let mut session = session_with_slash_palette_commands();

    for key in [
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
    ] {
        let event = session.process_key(key);
        assert!(event.is_none());
    }

    let submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(submit, Some(InlineEvent::Submit(value)) if value.trim() == "/review"));
}

#[test]
fn slash_palette_hides_entries_for_unmatched_keyword() {
    let mut session = session_with_slash_palette_commands();

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    assert!(
        !session.slash_palette.suggestions().is_empty(),
        "slash palette should show entries after typing '/'"
    );

    for key in [
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
    ] {
        let event = session.process_key(key);
        assert!(event.is_none());
    }

    assert!(
        session.slash_palette.suggestions().is_empty(),
        "slash palette should hide entries for unmatched /zzzz"
    );
}

#[test]
fn slash_trigger_auto_shows_inline_lists() {
    let mut session = session_with_slash_palette_commands();
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    assert!(session.inline_lists_visible());
}

#[test]
fn history_picker_trigger_auto_shows_inline_lists() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
    assert!(session.history_picker_state.active);
}

#[test]
fn file_palette_trigger_auto_shows_inline_lists() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::LoadFilePalette {
        files: vec!["src/main.rs".to_string()],
        workspace: PathBuf::from("."),
    });
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    session.handle_command(InlineCommand::SetInput("@src".to_string()));
    assert!(session.inline_lists_visible());
    assert!(session.file_palette_active);
}

#[test]
fn alt_up_edits_latest_queued_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.handle_command(InlineCommand::SetQueuedInputs {
        entries: vec!["first".to_string(), "second".to_string()],
    });

    let event = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert!(matches!(event, Some(InlineEvent::EditQueue)));
    assert_eq!(session.input_manager.content(), "second");
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
fn control_e_launches_editor() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(result, Some(InlineEvent::LaunchEditor)));
}

#[test]
fn control_a_moves_cursor_to_start() {
    let text = "hello world";
    let mut session = session_with_input(text, text.len());

    let result = session.process_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.cursor(), 0);
}

#[test]
fn control_e_moves_cursor_to_end_when_input_has_content() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.cursor(), text.len());
}

#[test]
fn control_w_deletes_previous_word() {
    let mut session = session_with_input("hello world", "hello world".len());

    let result = session.process_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello ");
    assert_eq!(session.cursor(), "hello ".len());
}

#[test]
fn control_u_deletes_to_start_of_line() {
    let mut session = session_with_input("hello world", 5);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), " world");
    assert_eq!(session.cursor(), 0);
}

#[test]
fn control_k_deletes_to_end_of_line() {
    let mut session = session_with_input("hello world", 5);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello");
    assert_eq!(session.cursor(), 5);
}

#[test]
fn control_alt_e_does_not_launch_editor() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    let result = session.process_key(event);

    assert!(!matches!(result, Some(InlineEvent::LaunchEditor)));
}

#[test]
fn control_g_launches_editor_from_plan_confirmation_modal() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_status_right = Some("model | 25% context".to_string());
    let plan = crate::ui::tui::types::PlanContent::from_markdown(
        "Test Plan".to_string(),
        "## Plan of Work\n- Step 1",
        Some(".vtcode/plans/test-plan.md".to_string()),
    );
    show_plan_confirmation_overlay(&mut session, plan);

    let event = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(
        result,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Hotkey(OverlayHotkeyAction::LaunchEditor)
        )))
    ));
    assert!(session.modal_state().is_none());
}

#[test]
fn plan_confirmation_modal_matches_four_way_gate_copy() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let plan = crate::ui::tui::types::PlanContent::from_markdown(
        "Test Plan".to_string(),
        "## Implementation Plan\n1. Step",
        Some(".vtcode/plans/test-plan.md".to_string()),
    );
    show_plan_confirmation_overlay(&mut session, plan);

    let modal = session
        .modal_state()
        .expect("plan confirmation modal should be present");
    assert_eq!(modal.title, "Ready to code?");
    assert_eq!(
        modal.lines.first().map(String::as_str),
        Some("A plan is ready to execute. Would you like to proceed?")
    );

    let list = modal
        .list
        .as_ref()
        .expect("plan confirmation should include list options");
    assert_eq!(list.items.len(), 3);

    assert_eq!(list.items[0].title, "Yes, auto-accept edits");
    assert_eq!(
        list.items[0].subtitle.as_deref(),
        Some("Execute with auto-approval.")
    );
    assert_eq!(list.items[0].badge.as_deref(), Some("Recommended"));

    assert_eq!(list.items[1].title, "Yes, manually approve edits");
    assert_eq!(
        list.items[1].subtitle.as_deref(),
        Some("Keep context and confirm each edit before applying.")
    );

    assert_eq!(list.items[2].title, "Type feedback to revise the plan");
    assert_eq!(
        list.items[2].subtitle.as_deref(),
        Some("Return to plan mode and refine the plan.")
    );
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
fn diff_overlay_defaults_to_edit_approval_mode() {
    let preview = DiffPreviewState::new(
        "src/main.rs".to_string(),
        "before".to_string(),
        "after".to_string(),
        Vec::new(),
    );

    assert_eq!(preview.mode, DiffPreviewMode::EditApproval);
}

#[test]
fn diff_overlay_edit_approval_keys_remain_unchanged() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, DiffPreviewMode::EditApproval);
    let apply = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        apply,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::DiffApply
        )))
    ));

    show_diff_overlay(&mut session, DiffPreviewMode::EditApproval);
    let reload = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(reload.is_none());
    assert!(session.diff_preview_state().is_some());

    let reject = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(
        reject,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::DiffReject
        )))
    ));
}

#[test]
fn diff_overlay_conflict_mode_maps_enter_reload_and_escape() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, DiffPreviewMode::FileConflict);
    let proceed = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        proceed,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::DiffProceed
        )))
    ));

    show_diff_overlay(&mut session, DiffPreviewMode::FileConflict);
    let reload = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(matches!(
        reload,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::DiffReload
        )))
    ));

    show_diff_overlay(&mut session, DiffPreviewMode::FileConflict);
    let abort = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(
        abort,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::DiffAbort
        )))
    ));
}

#[test]
fn diff_overlay_conflict_mode_ignores_trust_shortcuts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, DiffPreviewMode::FileConflict);
    let event = session.process_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(matches!(
        session.diff_preview_state().map(|preview| preview.mode),
        Some(DiffPreviewMode::FileConflict)
    ));
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
fn question_mark_opens_help_overlay_when_input_is_empty() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

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

fn request_user_input_step(question_id: &str, label: &str) -> WizardStep {
    WizardStep {
        title: format!("Question {question_id}"),
        question: format!("Select {question_id}"),
        items: vec![InlineListItem {
            title: label.to_string(),
            subtitle: Some("Option".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: question_id.to_string(),
                selected: vec![label.to_string()],
                other: None,
            }),
            search_value: Some(label.to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: None,
        freeform_placeholder: None,
    }
}

fn show_plan_confirmation_overlay(session: &mut Session, plan: crate::ui::tui::types::PlanContent) {
    let mut lines: Vec<String> = plan
        .raw_content
        .lines()
        .map(|line| line.to_string())
        .collect();
    if lines.is_empty() && !plan.summary.is_empty() {
        lines.push(plan.summary.clone());
    }
    lines.insert(
        0,
        "A plan is ready to execute. Would you like to proceed?".to_string(),
    );

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: "Ready to code?".to_string(),
            lines,
            footer_hint: plan
                .file_path
                .as_ref()
                .map(|path| format!("ctrl-g to edit in VS Code · {path}")),
            items: vec![
                InlineListItem {
                    title: "Yes, auto-accept edits".to_string(),
                    subtitle: Some("Execute with auto-approval.".to_string()),
                    badge: Some("Recommended".to_string()),
                    indent: 0,
                    selection: Some(InlineListSelection::PlanApprovalAutoAccept),
                    search_value: None,
                },
                InlineListItem {
                    title: "Yes, manually approve edits".to_string(),
                    subtitle: Some(
                        "Keep context and confirm each edit before applying.".to_string(),
                    ),
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::PlanApprovalExecute),
                    search_value: None,
                },
                InlineListItem {
                    title: "Type feedback to revise the plan".to_string(),
                    subtitle: Some("Return to plan mode and refine the plan.".to_string()),
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::PlanApprovalEditPlan),
                    search_value: None,
                },
            ],
            selected: Some(InlineListSelection::PlanApprovalAutoAccept),
            search: None,
            hotkeys: vec![OverlayHotkey {
                key: OverlayHotkeyKey::CtrlChar('g'),
                action: OverlayHotkeyAction::LaunchEditor,
            }],
        })),
    });
}

#[test]
fn show_list_modal_uses_bottom_inline_panel_min_height() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let item = InlineListItem {
        title: "Option A".to_string(),
        subtitle: Some("Select this option".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::SlashCommand("a".to_string())),
        search_value: Some("Option A".to_string()),
    };
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: "Pick one".to_string(),
            lines: vec!["Choose an option".to_string()],
            footer_hint: None,
            items: vec![item],
            selected: None,
            search: None,
            hotkeys: Vec::new(),
        })),
    });

    let input_width = VIEW_WIDTH.saturating_sub(2);
    let base_input_height =
        Session::input_block_height_for_lines(session.desired_input_lines(input_width));

    let backend = TestBackend::new(VIEW_WIDTH, 30);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render list modal");

    assert!(
        session.input_height >= base_input_height + ui::INLINE_LIST_PANEL_MIN_HEIGHT,
        "list modal should reserve min panel height below input"
    );
}

#[test]
fn render_always_reserves_input_status_row() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let input_width = VIEW_WIDTH.saturating_sub(2);
    let base_input_height =
        Session::input_block_height_for_lines(session.desired_input_lines(input_width));

    let backend = TestBackend::new(VIEW_WIDTH, 30);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session");

    assert!(
        session.input_height >= base_input_height + ui::INLINE_INPUT_STATUS_HEIGHT,
        "input should always reserve persistent status row"
    );
}

#[test]
fn wizard_multistep_submit_keeps_modal_open_until_last_step() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let steps = vec![
        request_user_input_step("q1", "Scope"),
        request_user_input_step("q2", "Priority"),
    ];

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Questions".to_string(),
            steps,
            current_step: 0,
            search: None,
            mode: WizardModalMode::MultiStep,
        })),
    });
    assert!(session.wizard_overlay().is_some());

    let first_submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(first_submit.is_none());
    assert!(
        session.wizard_overlay().is_some(),
        "wizard should remain open after intermediate step completion"
    );
    assert_eq!(
        session.wizard_overlay().map(|wizard| wizard.current_step),
        Some(1)
    );

    let final_submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        final_submit,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections)
        ))) if selections.len() == 2
    ));
    assert!(
        session.wizard_overlay().is_none(),
        "wizard should close after final submission"
    );
}

#[test]
fn wizard_search_paste_updates_filter_in_session_handle_event() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let (tx, _rx) = mpsc::unbounded_channel();

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Question".to_string(),
            steps: vec![WizardStep {
                title: "Choose".to_string(),
                question: "Pick one".to_string(),
                items: vec![
                    InlineListItem {
                        title: "Scope".to_string(),
                        subtitle: None,
                        badge: None,
                        indent: 0,
                        selection: Some(InlineListSelection::SlashCommand("scope".to_string())),
                        search_value: Some("scope".to_string()),
                    },
                    InlineListItem {
                        title: "Priority".to_string(),
                        subtitle: None,
                        badge: None,
                        indent: 0,
                        selection: Some(InlineListSelection::SlashCommand("priority".to_string())),
                        search_value: Some("priority".to_string()),
                    },
                ],
                completed: false,
                answer: None,
                allow_freeform: false,
                freeform_label: None,
                freeform_placeholder: None,
            }],
            current_step: 0,
            search: Some(InlineListSearchConfig {
                label: "Filter".to_string(),
                placeholder: None,
            }),
            mode: WizardModalMode::MultiStep,
        })),
    });

    session.handle_event(CrosstermEvent::Paste("prio".to_string()), &tx, None);

    let wizard = session.wizard_overlay().expect("wizard should stay open");
    assert_eq!(
        wizard.search.as_ref().map(|search| search.query.as_str()),
        Some("prio")
    );
    assert_eq!(wizard.steps[0].list.visible_indices, vec![1]);
}

#[test]
fn wizard_tabbed_submit_closes_modal_immediately() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let steps = vec![request_user_input_step("q1", "Single choice")];

    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::Wizard(WizardOverlayRequest {
            title: "Question".to_string(),
            steps,
            current_step: 0,
            search: None,
            mode: WizardModalMode::TabbedList,
        })),
    });
    assert!(session.wizard_overlay().is_some());

    let submit = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        submit,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Wizard(selections)
        ))) if selections.len() == 1
    ));
    assert!(session.wizard_overlay().is_none());
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
fn header_shows_safe_badge_for_tools_policy_trust() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.workspace_trust = format!("{}tools policy", ui::HEADER_TRUST_PREFIX);
    session.input_manager.set_content("test".to_string());
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
    assert!(line_text.contains("[SAFE]"));
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
    // Test with a wide character (Chinese character takes 2 columns)
    let line = Line::from(vec![Span::styled(
        "你".to_string(),
        ratatui_style_from_inline(&style, None),
    )]);

    let wrapped = session.wrap_line(line, 1);
    let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

    // Wide characters should be preserved even when wider than viewport
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

    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "first output".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    assert!(
        session.reflow_pty_lines(0, 80).is_empty(),
        "placeholder PTY line should remain hidden",
    );

    let rendered = session.reflow_pty_lines(1, 80);
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
fn pty_wrapped_lines_keep_hanging_left_padding() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "  └ this PTY output line wraps on narrow widths".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let rendered = session.reflow_pty_lines(0, 18);
    assert!(
        rendered.len() >= 2,
        "expected wrapped PTY output, got {} line(s)",
        rendered.len()
    );

    let first = line_text(&rendered[0]);
    let second = line_text(&rendered[1]);

    assert!(first.starts_with("    └ "), "first line was: {first:?}");
    assert!(
        second.starts_with("      "),
        "wrapped line should keep hanging indent, got: {second:?}"
    );
}

#[test]
fn pty_wrapped_lines_do_not_exceed_viewport_width() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "  └ this PTY output line wraps on narrow widths".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let width = 18usize;
    let rendered = session.reflow_pty_lines(0, width as u16);
    for line in rendered {
        let line_width: usize = line.spans.iter().map(|span| span.width()).sum();
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
fn pty_lines_use_subdued_foreground() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Pty,
        vec![InlineSegment {
            text: "plain pty output".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let rendered = session.reflow_pty_lines(0, 80);
    let body_span = rendered
        .iter()
        .flat_map(|line| line.spans.iter())
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
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.contains("pty output"))
        .expect("expected PTY body span");
    assert_eq!(pty_body.style.fg, Some(pty_fg));
    assert!(pty_body.style.add_modifier.contains(Modifier::DIM));
    assert_ne!(agent_body.style.fg, pty_body.style.fg);
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
                style: Arc::new(InlineTextStyle::default()),
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
        footer
            .iter()
            .any(|line| line.contains("↳ third queued message")),
        "latest queued message should render first"
    );
    assert!(
        footer
            .iter()
            .any(|line| line.contains("↳ second queued message")),
        "second-latest queued message should render second"
    );
    let hint = if cfg!(target_os = "macos") {
        "⌥ + ↑ edit"
    } else {
        "Alt + ↑ edit"
    };
    assert!(
        footer.iter().any(|line| line.contains(hint)),
        "hint line should show how to edit queue"
    );
}

#[test]
fn running_activity_not_overlaid_above_queue_lines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetQueuedInputs {
        entries: vec![
            "first queued message".to_string(),
            "second queued message".to_string(),
        ],
    });
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let mut visible = vec![Line::default(); 6];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(line_text).collect();

    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("Running command: test")),
        "running status should not be overlaid in transcript"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("↳ second queued message")),
        "latest queued message should remain visible"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("↳ first queued message")),
        "older queued message should remain visible"
    );
}

#[test]
fn running_activity_not_overlaid_without_queue() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: grep".to_string()),
        right: None,
    });

    let mut visible = vec![Line::default(); 3];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(line_text).collect();

    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("Running tool: grep")),
        "running status should render only in bottom input status row"
    );
}

#[test]
fn pty_busy_state_does_not_overlay_transcript_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    let mut visible = vec![Line::default(); 2];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(line_text).collect();

    assert!(
        !rendered.iter().any(|line| line.contains("Running...")),
        "busy PTY state should not inject transcript status overlay"
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

    let index = session
        .lines
        .len()
        .checked_sub(1)
        .expect("tool detail line should exist");
    let spans = session.render_message_spans(index);

    assert_eq!(spans.len(), 1);
    let body_span = &spans[0];
    assert!(body_span.style.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(body_span.content.clone().into_owned(), "    result line");
}

// Tests for streaming input queuing behavior (GitHub #12569)
// These tests verify that input is queued instead of submitted immediately
// when the assistant is streaming its final answer.

#[test]
fn streaming_state_starts_false() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);
}

#[test]
fn streaming_state_set_on_agent_append_line() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);

    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![InlineSegment {
            text: "Hello".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    });

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

    // Start streaming
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![InlineSegment {
            text: "Hello".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    });
    assert!(session.is_streaming_final_answer);

    // Simulate turn completion (status cleared)
    session.handle_command(InlineCommand::SetInputStatus {
        left: None,
        right: None,
    });

    assert!(!session.is_streaming_final_answer);
}

#[test]
fn streaming_state_not_cleared_on_status_update_with_content() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    // Start streaming
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![InlineSegment {
            text: "Hello".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    });
    assert!(session.is_streaming_final_answer);

    // Status update with content (not turn completion)
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

#[test]
fn streaming_state_set_on_agent_append_pasted_message() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);

    session.handle_command(InlineCommand::AppendPastedMessage {
        kind: InlineMessageKind::Agent,
        text: "Hello".to_string(),
        line_count: 1,
    });

    assert!(session.is_streaming_final_answer);
}
