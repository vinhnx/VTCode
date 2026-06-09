use super::super::*;
use super::helpers::*;

#[test]
fn busy_slash_palette_stop_interrupts_immediately() {
    let mut session = session_with_slash_palette_commands();
    session.handle_command(app_types::InlineCommand::SetInputStatus {
        left: Some("Running command: cargo test".to_string()),
        right: None,
    });

    for key in [
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
    ] {
        let event = session.process_key(key);
        assert!(event.is_none());
    }

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(app_types::InlineEvent::Interrupt)));
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
    assert!(
        matches!(submit, Some(app_types::InlineEvent::Submit(value)) if value.trim() == "/new")
    );
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
    assert!(
        matches!(submit, Some(app_types::InlineEvent::Submit(value)) if value.trim() == "/review")
    );
}

#[test]
fn control_m_submits_model_command_without_clearing_draft() {
    let mut session = app_session_with_input("draft prompt", "draft prompt".len());

    let event = session.process_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL));

    assert!(
        matches!(event, Some(app_types::InlineEvent::Submit(value)) if value == "/model"),
        "ctrl+m should open the model picker via /model"
    );
    assert_eq!(session.core.input_manager.content(), "draft prompt");
}

#[test]
fn app_question_mark_opens_visible_help_overlay() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(session.has_active_overlay());
    let modal = session.modal_state().expect("help modal should be visible");
    assert_eq!(modal.title, "Keyboard Shortcuts");
    assert!(modal.is_help_modal, "should be a help modal");

    let lines = rendered_app_session_lines(&mut session, VIEW_ROWS);
    assert!(
        lines.iter().any(|line| line.contains("Keyboard Shortcuts")),
        "help overlay should render in the app session"
    );
}

#[test]
fn app_help_command_opens_same_visible_help_overlay() {
    let mut session = app_session_with_input("/help", "/help".len());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(session.core.input_manager.content().is_empty());
    assert!(session.has_active_overlay());
    let modal = session.modal_state().expect("help modal should be visible");
    assert_eq!(modal.title, "Keyboard Shortcuts");
    assert!(modal.is_help_modal, "should be a help modal");

    let lines = rendered_app_session_lines(&mut session, 32);
    assert!(
        lines.iter().any(|line| line.contains("Keyboard Shortcuts")),
        "help overlay should render the title"
    );
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
fn slash_palette_keeps_base_input_and_cursor_active() {
    let mut session = session_with_slash_palette_commands();

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(session.core.input_enabled());
    assert!(
        session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );
}

#[test]
fn slash_panel_renders_search_field_above_results() {
    let mut session = session_with_slash_palette_commands();
    for key in [
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
    ] {
        let _ = session.process_key(key);
    }

    let lines = rendered_app_session_lines(&mut session, 20);
    let search_index = lines
        .iter()
        .position(|line| line.contains("Search commands"))
        .expect("search commands field should render");
    let item_index = lines
        .iter()
        .position(|line| line.contains("/review"))
        .expect("slash result should render");

    assert!(search_index < item_index);
}

#[test]
fn slash_palette_uses_full_width_header_background_and_divider() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        background: Some(AnsiColorEnum::Rgb(RgbColor(0x2B, 0x2D, 0x33))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x99, 0xFF))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new_with_logs(
        theme,
        None,
        20,
        true,
        None,
        vec![
            app_types::SlashCommandItem::new("new", "Start a new session"),
            app_types::SlashCommandItem::new("review", "Review current diff"),
        ],
        "Agent TUI".to_string(),
    );

    for key in [
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
    ] {
        let _ = session.process_key(key);
    }

    let backend = TestBackend::new(VIEW_WIDTH, 20);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render slash palette");

    let lines = rendered_app_session_lines(&mut session, 20);
    let title_row = lines
        .iter()
        .position(|line| line.contains("Slash Commands"))
        .expect("slash title row");
    let divider_row_index = lines
        .iter()
        .position(|line| is_horizontal_rule(line))
        .expect("slash divider row");
    let panel_area = session.core.bottom_panel_area().expect("panel area");
    let buffer = terminal.backend().buffer();
    let title_left = buffer
        .cell((panel_area.x, title_row as u16))
        .expect("title left cell");
    let title_right = buffer
        .cell((
            panel_area.x + panel_area.width.saturating_sub(1),
            title_row as u16,
        ))
        .expect("title right cell");
    let divider_row = (0..panel_area.width)
        .filter_map(|x| buffer.cell((panel_area.x + x, divider_row_index as u16)))
        .map(|cell| cell.symbol().to_string())
        .collect::<String>()
        .trim_end()
        .to_string();

    assert_eq!(title_left.style().bg, Some(Color::Rgb(0x2B, 0x2D, 0x33)));
    assert_eq!(title_right.style().bg, Some(Color::Rgb(0x2B, 0x2D, 0x33)));
    assert_eq!(
        divider_row,
        ui::INLINE_BLOCK_HORIZONTAL.repeat(panel_area.width as usize)
    );
}

#[test]
fn slash_panel_height_stays_fixed_for_short_results() {
    let mut short_session = session_with_slash_palette_commands();
    for key in [
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
    ] {
        let _ = short_session.process_key(key);
    }

    let _ = rendered_app_session_lines(&mut short_session, 20);
    let short_height = short_session
        .core
        .bottom_panel_area()
        .expect("short slash panel area")
        .height;

    let mut full_session = session_with_slash_palette_commands();
    let _ = full_session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    let _ = rendered_app_session_lines(&mut full_session, 20);
    let full_height = full_session
        .core
        .bottom_panel_area()
        .expect("full slash panel area")
        .height;

    assert_eq!(
        short_height, full_height,
        "slash panel height should stay fixed regardless of result count"
    );
}
