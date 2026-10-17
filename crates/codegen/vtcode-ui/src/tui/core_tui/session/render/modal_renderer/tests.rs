use super::*;
use crate::tui::core_tui::types::{InlineCommand, InlineSegment, InlineTextStyle};
use crate::tui::ui::tui::InlineTheme;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use ratatui::{Terminal, backend::TestBackend};
use std::sync::Arc;
use tokio::sync::mpsc;

fn frame_text(terminal: &Terminal<TestBackend>) -> String {
    terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect()
}

fn base_session() -> Session {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![InlineSegment {
            text: "BASE α".to_owned(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    });
    session.handle_command(InlineCommand::SetInput("draft β".to_owned()));
    session.handle_command(InlineCommand::SetInputStatus { left: Some("STATUS Ω".to_owned()), right: None });
    session
}

fn assert_base_frame(terminal: &Terminal<TestBackend>) {
    let text = frame_text(terminal);
    for marker in ["BASE α", "draft β", "STATUS Ω"] {
        assert!(text.contains(marker), "missing {marker:?} in frame: {text:?}");
    }
}

#[test]
fn core_frame_without_overlay_preserves_transcript_input_and_status() {
    for (width, height) in [(120, 40), (44, 18)] {
        let mut session = base_session();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| session.render(frame)).unwrap();
        assert_base_frame(&terminal);
    }
}

#[test]
fn core_frame_restores_base_and_clears_hit_areas_after_escape() {
    for (width, height) in [(120, 40), (44, 18)] {
        let mut session = base_session();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        session.show_modal("MODAL γ".to_owned(), vec!["https://example.com/modal".to_owned()], None);
        terminal.draw(|frame| session.render(frame)).unwrap();
        assert!(frame_text(&terminal).contains("MODAL γ"));
        assert!(!session.modal_text_areas().is_empty());
        assert!(!session.modal_link_targets().is_empty());

        let (events, _receiver) = mpsc::unbounded_channel();
        session.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)), &events, None);
        assert!(!session.has_active_overlay());
        terminal.draw(|frame| session.render(frame)).unwrap();

        assert_base_frame(&terminal);
        assert!(!frame_text(&terminal).contains("MODAL γ"));
        assert!(session.modal_list_area().is_none());
        assert!(session.modal_text_areas().is_empty());
        assert!(session.modal_link_targets().is_empty());
    }
}

#[test]
fn modal_title_text_uses_modal_title_and_empty_default() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    assert_eq!(modal_title_text(&session), "");

    session.show_modal("Config".to_owned(), vec![], None);
    assert_eq!(modal_title_text(&session), "Config");
}

#[test]
fn modal_title_style_uses_explicit_chrome_color() {
    let session = Session::new(InlineTheme::default(), None, 20);
    let styles = modal_render_styles(&session);

    assert_eq!(styles.title.fg, Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN)));
    assert!(styles.title.bg.is_none());
    assert_eq!(styles.border.fg, Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN)));
    assert!(styles.title.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn modal_section_headers_use_chrome_color_on_base_background() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Ansi256(Ansi256Color(16))),
        background: Some(AnsiColorEnum::Ansi256(Ansi256Color(231))),
        primary: Some(AnsiColorEnum::Ansi256(Ansi256Color(117))),
        ..InlineTheme::default()
    };
    let session = Session::new(theme, None, 20);
    let styles = modal_render_styles(&session);

    assert_eq!(styles.header.fg, Some(Color::Indexed(117)));
    assert_eq!(styles.header.bg, Some(Color::Indexed(231)));
    assert_eq!(styles.instruction_title.fg, Some(Color::Indexed(117)));
    assert_eq!(styles.instruction_title.bg, Some(Color::Indexed(231)));
    assert!(styles.header.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn modal_render_styles_keep_popup_text_readable_without_dim() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Ansi256(Ansi256Color(252))),
        background: Some(AnsiColorEnum::Ansi256(Ansi256Color(235))),
        secondary: Some(AnsiColorEnum::Ansi256(Ansi256Color(245))),
        ..InlineTheme::default()
    };
    let session = Session::new(theme, None, 20);
    let styles = modal_render_styles(&session);
    let foreground = Some(Color::Indexed(252));
    let muted = Some(Color::Indexed(245));

    // The modal background must be modifier-free: ratatui's `Cell::set_style`
    // only *inserts* modifiers, so a DIM painted as the area background
    // sticks to every glyph later drawn inside the popup (the whole HITL
    // approval popup used to render dimmed).
    assert!(!styles.background.add_modifier.contains(Modifier::DIM));
    assert_eq!(styles.background.fg, foreground);

    // Body text and option titles stay at full foreground so every choice
    // reads; emphasis comes from the selected row's highlight instead.
    for (name, style) in [
        ("selectable", styles.selectable),
        ("instruction_body", styles.instruction_body),
        ("header", styles.header),
        ("title", styles.title),
    ] {
        assert!(!style.add_modifier.contains(Modifier::DIM), "{name} must not carry DIM: {style:?}");
    }
    assert_eq!(styles.selectable.fg, styles.background.fg);

    // Secondary text recedes by explicit muted color, never by intensity.
    for (name, style) in [
        ("detail", styles.detail),
        ("hint", styles.hint),
        ("divider", styles.divider),
        ("badge", styles.badge),
    ] {
        assert!(!style.add_modifier.contains(Modifier::DIM), "{name} must recede by color, got: {style:?}");
        assert_eq!(style.fg, muted, "{name} needs the muted foreground token");
    }
}

#[test]
fn floating_modal_area_uses_bottom_half_of_viewport() {
    let area = floating_modal_area(Rect::new(3, 5, 80, 31));

    assert_eq!(area, Rect::new(3, 21, 80, 15));
}

#[test]
fn floating_modal_area_uses_exact_half_for_even_height() {
    let area = floating_modal_area(Rect::new(0, 0, 80, 30));

    assert_eq!(area, Rect::new(0, 15, 80, 15));
}

#[test]
fn floating_modal_area_preserves_single_row_viewport() {
    let area = floating_modal_area(Rect::new(0, 0, 80, 1));

    assert_eq!(area, Rect::new(0, 0, 80, 1));
}

#[test]
fn clip_transcript_area_stops_at_overlapping_modal_top() {
    let transcript = Rect::new(2, 5, 76, 18);
    let modal = Rect::new(0, 14, 80, 10);

    assert_eq!(clip_transcript_area(transcript, modal), Rect::new(2, 5, 76, 9));
}

#[test]
fn clip_transcript_area_preserves_non_overlapping_transcript() {
    let transcript = Rect::new(2, 5, 76, 8);
    let modal = Rect::new(0, 14, 80, 10);

    assert_eq!(clip_transcript_area(transcript, modal), transcript);
}

#[test]
fn clip_transcript_area_handles_horizontal_non_overlap() {
    let transcript = Rect::new(2, 5, 20, 18);
    let modal = Rect::new(30, 14, 20, 10);

    assert_eq!(clip_transcript_area(transcript, modal), transcript);
}

#[test]
fn estimated_instruction_rows_counts_short_lines_verbatim() {
    let lines = vec!["Choose an option".to_string(), "Second line".to_string()];
    assert_eq!(estimated_modal_instruction_rows(&lines, 78), 2);
}

#[test]
fn estimated_instruction_rows_wraps_long_summary() {
    let long = format!("Summary: {}", "word ".repeat(30));
    let rows = estimated_modal_instruction_rows(&[long], 78);
    assert!(rows >= 2, "long summary must claim wrapped rows, got {rows}");
}

#[test]
fn estimated_instruction_rows_treats_empty_as_single_row() {
    assert_eq!(estimated_modal_instruction_rows(&[], 78), 1);
    assert_eq!(estimated_modal_instruction_rows(&["   ".to_string()], 78), 1);
}

#[test]
fn estimated_instruction_rows_clamps_to_viewport() {
    let lines = vec!["word ".repeat(60); 10];
    assert_eq!(estimated_modal_instruction_rows(&lines, 78), MAX_INLINE_INSTRUCTION_ROWS);
}

#[test]
fn clip_transcript_area_handles_zero_and_constrained_rectangles() {
    assert_eq!(clip_transcript_area(Rect::new(0, 0, 0, 10), Rect::new(0, 0, 10, 10)), Rect::new(0, 0, 0, 10));
    assert_eq!(clip_transcript_area(Rect::new(0, 0, 10, 0), Rect::new(0, 0, 10, 1)), Rect::new(0, 0, 10, 0));
    assert_eq!(clip_transcript_area(Rect::new(0, 0, 10, 1), Rect::new(0, 0, 10, 1)), Rect::new(0, 0, 10, 0));
    assert_eq!(clip_transcript_area(Rect::new(0, 0, 10, 1), Rect::new(0, 1, 10, 1)), Rect::new(0, 0, 10, 1));
}
