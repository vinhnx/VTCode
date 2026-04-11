use super::super::*;
use super::helpers::*;

fn make_list_item(title: &str, cmd: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::SlashCommand(cmd.to_string())),
        search_value: Some(title.to_string()),
    }
}

fn show_list_modal(
    session: &mut AppSession,
    title: &str,
    lines: Vec<&str>,
    items: Vec<InlineListItem>,
) {
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::List(
            app_types::ListOverlayRequest {
                title: title.to_string(),
                lines: lines.into_iter().map(|s| s.to_string()).collect(),
                footer_hint: None,
                items,
                selected: None,
                search: None,
                hotkeys: Vec::new(),
            },
        )),
    });
}

fn render_session_to_terminal(session: &mut AppSession, rows: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(VIEW_WIDTH, rows);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session");
    terminal
}

fn render_session_to_terminal_app(session: &mut Session, rows: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(VIEW_WIDTH, rows);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session");
    terminal
}

fn show_overlay(
    session: &mut Session,
    title: &str,
    lines: Vec<&str>,
    items: Vec<InlineListItem>,
    selected: Option<InlineListSelection>,
) {
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: title.to_string(),
            lines: lines.into_iter().map(|s| s.to_string()).collect(),
            footer_hint: None,
            items,
            selected,
            search: None,
            hotkeys: Vec::new(),
        })),
    });
}

#[test]
fn show_list_modal_renders_as_floating_transient_without_bottom_panel() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![make_list_item("Option A", "a")],
    );

    let _terminal = render_session_to_terminal(&mut session, 30);

    assert!(
        session.has_active_overlay(),
        "floating list modal should remain active after rendering"
    );
    assert!(
        session.core.bottom_panel_area().is_none(),
        "floating list modal should not render in the bottom panel"
    );
}

#[test]
fn show_list_modal_uses_bottom_half_of_terminal() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![make_list_item("Option A", "a")],
    );

    let lines = rendered_app_session_lines(&mut session, 30);
    assert!(
        lines.get(15).is_some_and(|line| line.contains("Pick one")),
        "floating modal title should start at the halfway row"
    );

    let modal_area = session.core.modal_list_area().expect("modal list area");
    assert!(
        modal_area.y >= 17,
        "floating modal list should render below the title chrome, got y={}",
        modal_area.y
    );
}

#[test]
fn titled_floating_modal_renders_matching_title_and_divider_chrome() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![make_list_item("Option A", "a")],
    );

    let terminal = render_session_to_terminal(&mut session, 30);

    let buffer = terminal.backend().buffer();
    let title_cell = buffer.cell((0, 15)).expect("title cell");
    let top_divider_cell = buffer.cell((0, 16)).expect("top divider cell");
    let bottom_divider_cell = buffer.cell((0, 29)).expect("bottom divider cell");

    assert_eq!(title_cell.symbol(), "P");
    assert_eq!(top_divider_cell.symbol(), ui::INLINE_BLOCK_HORIZONTAL);
    assert_eq!(bottom_divider_cell.symbol(), ui::INLINE_BLOCK_HORIZONTAL);
    assert_ne!(
        title_cell.style().bg,
        Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN))
    );
    assert_eq!(
        top_divider_cell.style().fg,
        Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN))
    );
    assert_eq!(
        bottom_divider_cell.style().fg,
        Some(Color::Indexed(ui::SAFE_ANSI_BRIGHT_CYAN))
    );
}

#[test]
fn floating_modal_clears_stale_buffer_content_before_painting() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0x22, 0x22, 0x22))),
        background: Some(AnsiColorEnum::Rgb(RgbColor(0xF5, 0xF5, 0xF0))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x8F, 0xFF))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, 30);
    show_list_modal(
        &mut session,
        "Theme",
        vec!["Choose a theme"],
        vec![InlineListItem {
            title: "Clapre".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::SlashCommand("theme".to_string())),
            search_value: Some("Clapre".to_string()),
        }],
    );

    let backend = TestBackend::new(VIEW_WIDTH, 30);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| {
            let filler = (0..30)
                .map(|_| Line::from("X".repeat(VIEW_WIDTH as usize)))
                .collect::<Vec<_>>();
            frame.render_widget(ratatui::widgets::Paragraph::new(filler), frame.area());
        })
        .expect("failed to prefill terminal buffer");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render modal over stale buffer");

    let buffer = terminal.backend().buffer();
    let title_tail_cell = buffer
        .cell((VIEW_WIDTH.saturating_sub(1), 15))
        .expect("title tail cell");
    let body_blank_cell = buffer.cell((10, 25)).expect("body blank cell");

    assert_eq!(title_tail_cell.symbol(), " ");
    assert_eq!(body_blank_cell.symbol(), " ");
    assert_eq!(
        title_tail_cell.style().bg,
        Some(Color::Rgb(0xF5, 0xF5, 0xF0))
    );
    assert_eq!(
        body_blank_cell.style().bg,
        Some(Color::Rgb(0xF5, 0xF5, 0xF0))
    );
}

#[test]
fn selected_modal_row_uses_full_width_accent_background() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, 30);
    let selection = InlineListSelection::SlashCommand("a".to_string());
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::List(
            app_types::ListOverlayRequest {
                title: "Pick one".to_string(),
                lines: vec!["Choose an option".to_string()],
                footer_hint: None,
                items: vec![InlineListItem {
                    title: "Option A".to_string(),
                    subtitle: None,
                    badge: Some("Active".to_string()),
                    indent: 0,
                    selection: Some(selection.clone()),
                    search_value: Some("Option A".to_string()),
                }],
                selected: Some(selection),
                search: None,
                hotkeys: Vec::new(),
            },
        )),
    });

    let terminal = render_session_to_terminal(&mut session, 30);

    let modal_area = session.core.modal_list_area().expect("modal list area");
    let far_right = terminal
        .backend()
        .buffer()
        .cell((
            modal_area.x + modal_area.width.saturating_sub(1),
            modal_area.y,
        ))
        .expect("selected row far-right cell");
    assert_eq!(far_right.style().bg, Some(Color::Rgb(0x12, 0x34, 0x56)));

    let badge_cell = terminal
        .backend()
        .buffer()
        .cell((modal_area.x, modal_area.y))
        .expect("selected row badge cell");
    assert_eq!(badge_cell.style().bg, Some(Color::Rgb(0x12, 0x34, 0x56)));

    let title_cell = terminal
        .backend()
        .buffer()
        .cell((modal_area.x + 10, modal_area.y))
        .expect("selected row title cell");
    assert_eq!(title_cell.style().bg, Some(Color::Rgb(0x12, 0x34, 0x56)));
}

#[test]
fn modal_section_header_uses_foreground_contrast_on_light_theme() {
    let theme = InlineTheme {
        foreground: Some(AnsiColorEnum::Rgb(RgbColor(0x22, 0x22, 0x22))),
        background: Some(AnsiColorEnum::Rgb(RgbColor(0xF5, 0xF5, 0xF0))),
        primary: Some(AnsiColorEnum::Rgb(RgbColor(0x7A, 0x8F, 0xFF))),
        ..InlineTheme::default()
    };
    let mut session = AppSession::new(theme, None, 30);
    show_list_modal(
        &mut session,
        "Theme",
        vec!["Choose a theme"],
        vec![
            InlineListItem {
                title: "Built-in themes".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: None,
                search_value: Some("Built-in themes".to_string()),
            },
            InlineListItem {
                title: "Clapre".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::SlashCommand("theme".to_string())),
                search_value: Some("Clapre".to_string()),
            },
        ],
    );

    let terminal = render_session_to_terminal(&mut session, 30);

    let lines = rendered_app_session_lines(&mut session, 30);
    let title_row = lines
        .iter()
        .position(|line| line.trim() == "Theme")
        .expect("title row");
    let modal_area = session.core.modal_list_area().expect("modal list area");
    let header_cell = terminal
        .backend()
        .buffer()
        .cell((modal_area.x + 1, modal_area.y))
        .expect("section header cell");
    let title_cell = terminal
        .backend()
        .buffer()
        .cell((modal_area.x, title_row as u16))
        .expect("title cell");

    assert_eq!(title_cell.symbol(), "T");
    assert_eq!(title_cell.style().bg, Some(Color::Rgb(0xF5, 0xF5, 0xF0)));
    assert_eq!(header_cell.symbol(), "B");
    assert_eq!(header_cell.style().fg, Some(Color::Rgb(0x7A, 0x8F, 0xFF)));
    assert_eq!(header_cell.style().bg, Some(Color::Rgb(0xF5, 0xF5, 0xF0)));
    assert!(header_cell.style().add_modifier.contains(Modifier::BOLD));
}

#[test]
fn untitled_floating_modal_skips_title_chrome_rows() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    show_list_modal(
        &mut session,
        "",
        vec!["Choose an option"],
        vec![make_list_item("Option A", "a")],
    );

    let lines = rendered_app_session_lines(&mut session, 30);
    assert!(
        lines
            .get(15)
            .is_some_and(|line| line.contains("Choose an option")),
        "untitled modal body should begin at the floating modal origin"
    );
    assert!(
        (15..30).all(|row| !lines.get(row).is_some_and(|line| is_horizontal_rule(line))),
        "untitled modal should not render title chrome divider rows"
    );

    let modal_area = session.core.modal_list_area().expect("modal list area");
    assert_eq!(modal_area.y, 16);
}

#[test]
fn closing_top_transient_restores_previous_bottom_panel() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    session.set_task_panel_visible(true);

    let mut terminal = render_session_to_terminal(&mut session, 30);
    assert!(
        session.core.bottom_panel_area().is_some(),
        "task panel should occupy the bottom panel when visible"
    );

    show_list_modal(
        &mut session,
        "Pick one",
        vec!["Choose an option"],
        vec![make_list_item("Option A", "a")],
    );

    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render stacked transients");
    assert!(
        session.core.bottom_panel_area().is_none(),
        "floating transient should hide the lower bottom panel while it is on top"
    );

    session.close_transient();
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render restored bottom panel");
    assert!(
        session.core.bottom_panel_area().is_some(),
        "closing the top transient should restore the previous bottom panel"
    );
}

#[test]
fn list_modal_keeps_last_selection_when_items_append() {
    let mut session = Session::new(InlineTheme::default(), None, 30);

    let selected = InlineListSelection::SlashCommand("second".to_string());
    show_overlay(
        &mut session,
        "Pick",
        vec!["Choose"],
        vec![
            make_list_item("First", "first"),
            make_list_item("Second", "second"),
        ],
        Some(selected.clone()),
    );
    session.handle_command(InlineCommand::CloseOverlay);

    show_overlay(
        &mut session,
        "Pick",
        vec!["Choose"],
        vec![
            make_list_item("First", "first"),
            make_list_item("Second", "second"),
            make_list_item("Third", "third"),
        ],
        Some(selected),
    );

    let selection = session
        .modal_state()
        .and_then(|modal| modal.list.as_ref())
        .and_then(|list| list.current_selection());
    assert_eq!(
        selection,
        Some(InlineListSelection::SlashCommand("third".to_string()))
    );
}

#[test]
fn render_always_reserves_input_status_row() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let input_width = VIEW_WIDTH.saturating_sub(2);
    let base_input_height =
        Session::input_block_height_for_lines(session.desired_input_lines(input_width));

    let _terminal = render_session_to_terminal_app(&mut session, 30);

    assert!(
        session.input_height >= base_input_height + ui::INLINE_INPUT_STATUS_HEIGHT,
        "input should always reserve persistent status row"
    );
}
