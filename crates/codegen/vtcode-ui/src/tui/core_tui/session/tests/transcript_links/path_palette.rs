use super::*;

#[test]
fn path_with_line_col_suffix_resolves_correctly() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let path_with_loc = format!("{absolute_path}:42:10");
    session.push_line(InlineMessageKind::Agent, vec![make_segment(&format!("Error at {path_with_loc}"))]);

    let decorated = session.decorate_visible_transcript_links(
        vec![transcript_line(format!("Error at {path_with_loc}"))],
        Rect::new(0, 0, 200, 1),
    );

    assert!(!session.transcript_file_link_targets.is_empty());
    let target = session.transcript_file_link_targets[0].clone();
    assert!(matches!(
        &target.target,
        TranscriptLinkTarget::File(path)
            if path.path().display().to_string() == absolute_path
                && path.location_suffix() == Some(":42:10")
    ));
    let (tx, mut rx) = mpsc::unbounded_channel();
    left_click_session(&mut session, &tx, target.area.x, target.area.y, open_file_click_modifiers());
    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == path_with_loc
    ));
    assert!(
        decorated[0]
            .spans
            .iter()
            .any(|span| { span.style.add_modifier.contains(Modifier::UNDERLINED) })
    );
}
#[test]
fn path_with_paren_location_resolves_correctly() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let path_with_loc = format!("{absolute_path}(10,5)");

    let _ = session.decorate_visible_transcript_links(
        vec![transcript_line(format!("Error at {path_with_loc}"))],
        Rect::new(0, 0, 200, 1),
    );

    assert!(!session.transcript_file_link_targets.is_empty());
    assert!(matches!(
        &session.transcript_file_link_targets[0].target,
        TranscriptLinkTarget::File(path)
            if path.path().display().to_string() == absolute_path
                && path.location_suffix() == Some(":10:5")
    ));
}
#[test]
fn path_with_hash_location_resolves_and_opens_with_canonical_suffix() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let hash_path = format!("{absolute_path}#L12C4");

    let _ = session.decorate_visible_transcript_links(
        vec![transcript_line(format!("Error at {hash_path}"))],
        Rect::new(0, 0, 200, 1),
    );

    let target = session
        .transcript_file_link_targets
        .first()
        .expect("expected transcript file target")
        .clone();
    assert!(matches!(
        &target.target,
        TranscriptLinkTarget::File(path)
            if path.path().display().to_string() == absolute_path
                && path.location_suffix() == Some(":12:4")
    ));

    let (tx, mut rx) = mpsc::unbounded_channel();
    left_click_session(&mut session, &tx, target.area.x, target.area.y, open_file_click_modifiers());

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == format!("{absolute_path}:12:4")
    ));
}
#[test]
fn abbreviation_tokens_are_not_detected_as_paths() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let _ = session
        .decorate_visible_transcript_links(vec![transcript_line("e.g. this or i.e. that")], Rect::new(0, 0, 200, 1));

    assert!(session.transcript_file_link_targets.is_empty());
}
#[test]
fn collapsed_paste_review_reflow_includes_detected_raw_links() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://example.com/collapsed-review".to_string();

    let mut json = String::from("{\n");
    for idx in 0..ui::INLINE_JSON_COLLAPSE_LINE_THRESHOLD {
        json.push_str(&format!("  \"key{idx}\": \"value{idx}\",\n"));
    }
    json.push_str(&format!("  \"link\": \"{url}\",\n"));
    json.push_str("  \"end\": true\n}");

    session.append_pasted_message(InlineMessageKind::Tool, json.clone(), json.lines().count());

    let collapsed_index = session.collapsed_pastes[0].line_index;
    let review_lines = session.reflow_message_lines_for_review(collapsed_index, 80);

    assert!(review_lines.iter().any(|line| {
        line.explicit_links
            .iter()
            .any(|link| matches!(&link.target, InlineLinkTarget::Url(target) if target == &url))
    }));
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
fn mouse_wheel_navigates_slash_palette_instead_of_transcript() {
    let mut session = session_with_slash_palette_commands();
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    let _ = rendered_app_session_lines(&mut session, 20);
    let selected_before = session.slash_palette.selected_index();

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let panel_area = session.core.bottom_panel_area().expect("panel area");
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: panel_area.x,
            row: panel_area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert_ne!(session.slash_palette.selected_index(), selected_before);
    assert_eq!(session.core.transcript_view_top, 0);
}
#[test]
fn mouse_wheel_navigates_modal_list_instead_of_transcript() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    show_basic_list_overlay(&mut session);
    let _ = rendered_session_lines(&mut session, 20);

    let selected_before = session
        .modal_state()
        .and_then(|modal| modal.list.as_ref())
        .and_then(|list| list.current_selection());

    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let modal_area = session.modal_list_area().expect("modal list area");
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: modal_area.x,
            row: modal_area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    let selected_after = session
        .modal_state()
        .and_then(|modal| modal.list.as_ref())
        .and_then(|list| list.current_selection());

    assert_ne!(selected_after, selected_before);
    assert_eq!(session.transcript_view_top, 0);
}
#[test]
fn clicking_selected_slash_row_applies_command() {
    let mut session = session_with_slash_palette_commands();
    for key in [
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
    ] {
        let _ = session.process_key(key);
    }

    let _ = rendered_app_session_lines(&mut session, 20);
    let panel_area = session.core.bottom_panel_area().expect("panel area");
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: panel_area.x,
            // Info + search + divider = 3 fixed rows, so the first result row
            // (the selected /review entry) is at panel_area.y + 3.
            row: panel_area.y + 3,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert_eq!(session.core.input_manager.content(), "/review ");
    assert!(session.slash_palette.suggestions().is_empty());
}
#[test]
fn clicking_selected_file_palette_row_inserts_reference() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let workspace = vtcode_tui_workspace_root();
    load_app_file_palette(&mut session, vec![workspace.join("Cargo.toml").display().to_string()], workspace.clone());
    session.handle_command(app_types::InlineCommand::SetInput("@".to_string()));

    let lines = rendered_app_session_lines(&mut session, 20);
    let panel_area = session.core.bottom_panel_area().expect("panel area");

    // Find the row for the top-level file in the rendered list.
    let child_row = lines
        .iter()
        .position(|line| line.contains("Cargo.toml"))
        .expect("file should render");

    let (event_tx, _event_rx) = mpsc::unbounded_channel();

    // First click selects the child row.
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: panel_area.x,
            row: child_row as u16,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    // Second click on the selected child inserts the file reference.
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: panel_area.x,
            row: child_row as u16,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert_eq!(session.core.input_manager.content(), "@Cargo.toml ");
    assert!(!session.file_palette_active);
}
#[test]
fn clicking_selected_history_row_accepts_entry() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.task_panel_lines = vec!["Active task".to_string()];
    session.set_task_panel_visible(true);
    session.core.set_input("cargo test".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    session.core.set_input("git status".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let _ = rendered_app_session_lines(&mut session, 20);
    let expected = session
        .history_picker_state
        .selected_match()
        .map(|item| item.content.clone())
        .expect("selected history entry");
    let panel_area = session.core.bottom_panel_area().expect("panel area");
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: panel_area.x,
            // Header + info + search = 3 fixed rows, so the first result row
            // (the selected entry) is at panel_area.y + 3.
            row: panel_area.y + 3,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert!(!session.history_picker_state.active);
    assert_eq!(session.core.input_manager.content(), expected);
    let lines = rendered_app_session_lines(&mut session, 20);
    assert!(
        lines.iter().any(|line| line.contains("Active task")),
        "task panel should resume after the history picker closes"
    );
}
#[test]
fn clicking_input_moves_cursor_to_clicked_position() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello world".to_string());
    session.set_cursor(session.input_manager.content().len());

    let _ = rendered_session_lines(&mut session, 20);
    let input_area = session.input_area().expect("input area");
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: input_area.x + 5,
            row: input_area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert_eq!(session.input_manager.cursor(), 5);
}
#[test]
fn clicking_input_does_not_start_transcript_selection() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello world".to_string());
    let _ = rendered_session_lines(&mut session, 20);

    let input_area = session.input_area().expect("input area");
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: input_area.x + 3,
            row: input_area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert_eq!(session.mouse_drag_target, MouseDragTarget::Input);
    assert!(!session.mouse_selection.is_selecting);
    assert!(!session.mouse_selection.has_selection);
}
#[test]
fn dragging_in_input_creates_input_selection_without_transcript_selection() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello world".to_string());
    session.set_cursor(0);
    let _ = rendered_session_lines(&mut session, 20);

    let input_area = session.input_area().expect("input area");
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: input_area.x + 1,
            row: input_area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: input_area.x + 6,
            row: input_area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: input_area.x + 6,
            row: input_area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert_eq!(session.input_manager.cursor(), 6);
    assert_eq!(session.input_manager.selection_range(), Some((1, 6)));
    assert_eq!(session.mouse_drag_target, MouseDragTarget::None);
    assert!(!session.mouse_selection.is_selecting);
    assert!(!session.mouse_selection.has_selection);
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
        view.iter().any(|line| line.contains(&format!("{LABEL_PREFIX}-9"))),
        "expected most recent transcript line to remain visible even when viewport is small"
    );
}
#[test]
fn pty_busy_state_does_not_overlay_transcript_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    let mut visible = vec![TranscriptLine::default(); 2];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(|line| line_text(&line.line)).collect();

    assert!(
        !rendered.iter().any(|line| line.contains("Running...")),
        "busy PTY state should not inject transcript status overlay"
    );
}
#[test]
fn narrow_lines_skip_redundant_wrap_in_decorate_detected_link_lines() {
    let line = Line::from(Span::raw("short link text"));
    let area = Rect::new(0, 0, 80, 1);
    let (decorated, targets) = transcript_links::decorate_detected_link_lines(
        vec![line],
        area,
        None,
        None,
        Style::default(),
        Style::default(),
    );
    assert_eq!(decorated.len(), 1, "narrow line should not be split");
    assert_eq!(decorated[0].spans.len(), 1, "narrow line should keep single span");
    assert_eq!(decorated[0].spans[0].content.as_ref(), "short link text");
    assert!(targets.is_empty(), "no link targets expected for plain text without URL pattern");
}
