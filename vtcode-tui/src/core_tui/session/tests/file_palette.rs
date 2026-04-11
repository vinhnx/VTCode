use super::super::*;
use super::helpers::*;

#[test]
fn file_palette_insertion_uses_at_alias_in_input() {
    let mut session = app_session_with_input("check @mai", "check @mai".len());

    session.insert_file_reference("src/main.rs");

    assert_eq!(session.core.input_manager.content(), "check @src/main.rs ");
    assert_eq!(session.core.cursor(), "check @src/main.rs ".len());
}

#[test]
fn set_input_command_activates_file_palette_for_at_query() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    load_app_file_palette(
        &mut session,
        vec!["src/main.rs".to_string()],
        PathBuf::from("."),
    );

    assert!(!session.file_palette_active);
    session.handle_command(app_types::InlineCommand::SetInput("@src".to_string()));
    assert!(session.file_palette_active);
}

#[test]
fn file_palette_renders_search_field_above_results() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    load_app_file_palette(
        &mut session,
        vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
        PathBuf::from("."),
    );
    session.handle_command(app_types::InlineCommand::SetInput("@src".to_string()));

    let lines = rendered_app_session_lines(&mut session, 20);
    let search_index = lines
        .iter()
        .position(|line| line.contains("Search files: [src"))
        .expect("search files field should render");
    let item_index = lines
        .iter()
        .position(|line| line.contains("src/main.rs"))
        .expect("file result should render");

    assert!(search_index < item_index);
}

#[test]
fn file_palette_uses_full_width_header_background_and_divider() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        background: Some(AnsiColorEnum::Rgb(RgbColor(0x2B, 0x2D, 0x33))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x99, 0xFF))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, VIEW_ROWS);
    load_app_file_palette(
        &mut session,
        vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
        PathBuf::from("."),
    );
    session.handle_command(app_types::InlineCommand::SetInput("@src".to_string()));

    let backend = TestBackend::new(VIEW_WIDTH, 20);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render file palette");

    let lines = rendered_app_session_lines(&mut session, 20);
    let title_row = lines
        .iter()
        .position(|line| line.contains("Files"))
        .expect("file title row");
    let divider_row_index = lines
        .iter()
        .position(|line| is_horizontal_rule(line))
        .expect("file divider row");
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
fn file_palette_trigger_auto_shows_inline_lists() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    load_app_file_palette(
        &mut session,
        vec!["src/main.rs".to_string()],
        PathBuf::from("."),
    );
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    session.handle_command(app_types::InlineCommand::SetInput("@src".to_string()));
    assert!(session.inline_lists_visible());
    assert!(session.file_palette_active);
}

#[test]
fn file_palette_keeps_base_input_and_cursor_active() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    load_app_file_palette(
        &mut session,
        vec!["src/main.rs".to_string()],
        PathBuf::from("."),
    );

    session.handle_command(app_types::InlineCommand::SetInput("@src".to_string()));

    assert!(session.file_palette_visible());
    assert!(session.core.input_enabled());
    assert!(
        session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );
}
