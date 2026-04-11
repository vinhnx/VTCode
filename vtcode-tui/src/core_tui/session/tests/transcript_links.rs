use super::super::*;
use super::helpers::*;
use crate::core_tui::session::transcript_links::TranscriptLinkTarget;

#[test]
fn transcript_relative_file_reference_is_underlined() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS * 2);
    session.set_workspace_root(Some(vtcode_tui_workspace_root()));

    let decorated = session.decorate_visible_transcript_links(
        vec![transcript_line(format!(
            "See {}",
            transcript_file_fixture_relative_path()
        ))],
        Rect::new(0, 0, 120, 1),
    );

    assert_eq!(session.transcript_file_link_targets.len(), 1);
    let linked_span = decorated[0]
        .spans
        .iter()
        .find(|span| {
            span.content
                .contains(transcript_file_fixture_relative_path())
        })
        .expect("expected linked span");
    assert!(
        linked_span
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
}

#[test]
fn hovered_transcript_file_reference_adds_hover_style() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS * 2);
    session.set_workspace_root(Some(vtcode_tui_workspace_root()));

    let line = transcript_line(format!("Open {}", transcript_file_fixture_relative_path()));
    let area = Rect::new(0, 0, 120, 1);
    let _ = session.decorate_visible_transcript_links(vec![line.clone()], area);
    let target = session
        .transcript_file_link_targets
        .first()
        .expect("expected transcript file target")
        .clone();

    assert!(session.update_transcript_file_link_hover(target.area.x, target.area.y));

    let decorated = session.decorate_visible_transcript_links(vec![line], area);
    let linked_span = decorated[0]
        .spans
        .iter()
        .find(|span| {
            span.content
                .contains(transcript_file_fixture_relative_path())
        })
        .expect("expected hovered linked span");
    assert!(
        linked_span
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
    assert!(linked_span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn mixed_transcript_file_references_are_all_underlined() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_workspace_root(Some(vtcode_tui_workspace_root()));
    let temp_file = quoted_transcript_temp_file_path();
    fs::write(&temp_file, "mixed-transcript-link").expect("write mixed transcript temp file");

    let line = transcript_line(format!(
        "Open {} and `{}`",
        transcript_file_fixture_relative_path(),
        temp_file.display()
    ));
    let temp_file_display = temp_file.display().to_string();
    let decorated = session.decorate_visible_transcript_links(vec![line], Rect::new(0, 0, 200, 1));

    assert_eq!(session.transcript_file_link_targets.len(), 2);
    assert!(decorated[0].spans.iter().any(|span| {
        span.content
            .contains(transcript_file_fixture_relative_path())
            && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));
    assert!(decorated[0].spans.iter().any(|span| {
        span.content.contains(&temp_file_display)
            && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));

    let _ = fs::remove_file(&temp_file);
}

#[test]
fn plain_click_emits_open_file_event_for_absolute_transcript_path() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    left_click_session(
        &mut fixture.session,
        &tx,
        area.x,
        area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
}

#[test]
fn repeated_plain_click_on_same_transcript_file_link_is_throttled() {
    let mut fixture = SessionWithFileLink::new();
    let area = fixture.target_area();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    };
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut fixture.session,
        &tx,
        click.column,
        click.row,
        click.modifiers,
    );
    left_click_session(
        &mut fixture.session,
        &tx,
        click.column,
        click.row,
        click.modifiers,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn repeated_plain_click_on_different_transcript_file_links_is_not_throttled() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let temp_file = quoted_transcript_temp_file_path();
    fs::write(&temp_file, "transcript-link-throttle").expect("write throttle transcript temp file");
    let quoted_temp_path = format!("`{}`", temp_file.display());
    session.push_line(
        InlineMessageKind::Agent,
        vec![make_segment(&format!(
            "Open {} and {}",
            absolute_path, quoted_temp_path
        ))],
    );

    let _ = visible_transcript(&mut session);
    let first_target = session
        .transcript_file_link_targets
        .iter()
        .find(|target| {
            matches!(
                &target.target,
                TranscriptLinkTarget::File(path) if path.path().display().to_string() == absolute_path
            )
        })
        .expect("expected first transcript file target")
        .clone();
    let second_target = session
        .transcript_file_link_targets
        .iter()
        .find(|target| {
            matches!(
                &target.target,
                TranscriptLinkTarget::File(path) if path.path() == temp_file.as_path()
            )
        })
        .expect("expected second transcript file target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut session,
        &tx,
        first_target.area.x,
        first_target.area.y,
        KeyModifiers::NONE,
    );
    left_click_session(
        &mut session,
        &tx,
        second_target.area.x,
        second_target.area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == absolute_path
    ));
    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == temp_file.display().to_string()
    ));

    let _ = fs::remove_file(&temp_file);
}

#[test]
fn double_click_emits_open_file_event_for_absolute_transcript_path() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    };
    left_click_session(
        &mut fixture.session,
        &tx,
        click.column,
        click.row,
        click.modifiers,
    );
    left_click_session(
        &mut fixture.session,
        &tx,
        click.column,
        click.row,
        click.modifiers,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn modifier_click_emits_open_file_event_for_quoted_path_with_spaces() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let temp_file = quoted_transcript_temp_file_path();
    fs::write(&temp_file, "transcript-link").expect("write quoted transcript temp file");
    let quoted_path = format!("`{}`", temp_file.display());
    let _ = session.decorate_visible_transcript_links(
        vec![transcript_line(format!("Open {}", quoted_path))],
        Rect::new(0, 0, 200, 1),
    );
    let target = session
        .transcript_file_link_targets
        .iter()
        .find(|target| {
            matches!(
                &target.target,
                TranscriptLinkTarget::File(path) if path.path() == temp_file.as_path()
            )
        })
        .expect("expected quoted transcript file target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == temp_file.display().to_string()
    ));

    let _ = fs::remove_file(&temp_file);
}

#[test]
fn modifier_click_emits_open_url_event_for_explicit_transcript_link() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://example.com/docs".to_string();
    let _ = session.decorate_visible_transcript_links(
        vec![TranscriptLine {
            line: Line::from("Open docs"),
            explicit_links: vec![RenderedTranscriptLink {
                start: 5,
                end: 9,
                start_col: 5,
                width: 4,
                target: InlineLinkTarget::Url(url.clone()),
            }],
        }],
        Rect::new(0, 0, 200, 1),
    );
    let target = session
        .transcript_file_link_targets
        .first()
        .expect("expected transcript url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}

#[test]
fn modifier_click_emits_open_url_event_for_raw_transcript_url() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://auth.openai.com/oauth/authorize?client_id=test".to_string();
    session.push_line(InlineMessageKind::Agent, vec![make_segment(url.as_str())]);
    let _ = rendered_session_lines(&mut session, VIEW_ROWS);
    let target = session
        .transcript_file_link_targets
        .first()
        .expect("expected transcript url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        open_file_click_modifiers(),
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}

#[test]
fn wrapped_transcript_url_last_segment_is_underlined_and_clickable() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = format!(
        "https://auth.openai.com/oauth/authorize?response_type=code&client_id=test&scope=openid%20profile%20email&state={}",
        "abcdefghijklmnopqrstuvwxyz".repeat(12)
    );
    session.push_line(InlineMessageKind::Agent, vec![make_segment(url.as_str())]);

    let transcript_lines = session.reflow_message_lines(0, 60);
    let decorated =
        session.decorate_visible_cached_transcript_links(transcript_lines, Rect::new(0, 0, 60, 8));
    let targets = session
        .transcript_file_link_targets
        .iter()
        .filter(|target| matches!(&target.target, TranscriptLinkTarget::Url(clicked) if clicked == &url))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        targets.len() >= 2,
        "expected wrapped transcript url segments"
    );

    let target = targets
        .iter()
        .max_by_key(|target| (target.area.y, target.area.x))
        .expect("expected wrapped transcript url target")
        .clone();
    assert!(
        decorated[target.area.y as usize]
            .spans
            .iter()
            .any(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        open_file_click_modifiers(),
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}

#[test]
fn reflowed_tool_lines_include_detected_raw_links() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://example.com/tool-output".to_string();
    session.push_line(
        InlineMessageKind::Tool,
        vec![make_segment(&format!("open {url}"))],
    );

    let transcript_lines = session.reflow_message_lines(0, 80);

    assert!(transcript_lines.iter().any(|line| {
        line.explicit_links
            .iter()
            .any(|link| matches!(&link.target, InlineLinkTarget::Url(target) if target == &url))
    }));
}

#[test]
fn modifier_click_emits_open_url_event_for_modal_auth_link_in_app_session() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://auth.openai.com/oauth/authorize?client_id=test".to_string();
    session.show_transient(wizard_auth_transient(&url));

    let _ = rendered_app_session_lines(&mut session, 20);
    let target = session
        .core
        .modal_link_targets()
        .first()
        .expect("expected modal url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_app_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        open_file_click_modifiers(),
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}

#[test]
fn plain_click_emits_open_url_event_for_wrapped_wizard_modal_auth_link() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = format!(
        "https://auth.openai.com/oauth/authorize?response_type=code&client_id=test&scope=openid%20profile%20email&state={}",
        "abcdefghijklmnopqrstuvwxyz".repeat(10)
    );
    session.show_transient(wizard_auth_transient(&url));

    let _ = rendered_app_session_lines(&mut session, 20);
    let targets = session
        .core
        .modal_link_targets()
        .iter()
        .filter(|target| matches!(&target.target, TranscriptLinkTarget::Url(clicked) if clicked == &url))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        !targets.is_empty(),
        "expected visible wizard modal url target"
    );

    let target = targets
        .iter()
        .max_by_key(|target| (target.area.y, target.area.x))
        .expect("expected wrapped wizard modal url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_app_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}

#[test]
fn modal_auth_text_in_app_session_is_selectable_and_copied() {
    let _guard = CLIPBOARD_TEST_LOCK
        .lock()
        .expect("clipboard test lock should not be poisoned");

    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://auth.openai.com/oauth/authorize?client_id=test".to_string();
    session.show_transient(wizard_auth_transient(&url));

    let _ = rendered_app_session_lines(&mut session, 20);
    let target = session
        .core
        .modal_link_targets()
        .first()
        .expect("expected modal url target")
        .clone();
    let (tx, _rx) = mpsc::unbounded_channel::<app_types::InlineEvent>();

    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: target.area.x,
            row: target.area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &tx,
        None,
    );
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: target.area.x + 5,
            row: target.area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &tx,
        None,
    );
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: target.area.x + 5,
            row: target.area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &tx,
        None,
    );

    let backend = TestBackend::new(VIEW_WIDTH, 20);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("render modal selection");

    let buffer = terminal.backend().buffer();
    assert_eq!(
        session
            .core
            .mouse_selection
            .extract_text(buffer, buffer.area),
        "https"
    );
    assert!(session.core.mouse_selection.has_selection);
    assert!(!session.core.mouse_selection.needs_copy());
}

#[test]
fn plain_click_emits_open_url_event_for_standard_modal_auth_link() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = format!(
        "https://auth.openai.com/oauth/authorize?client_id=test&state={}",
        "abcdefghijklmnopqrstuvwxyz".repeat(10)
    );
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(list_auth_overlay(&url)),
    });

    let _ = rendered_session_lines(&mut session, VIEW_ROWS);
    let targets = session
        .modal_link_targets()
        .iter()
        .filter(|target| matches!(&target.target, TranscriptLinkTarget::Url(clicked) if clicked == &url))
        .cloned()
        .collect::<Vec<_>>();
    assert!(!targets.is_empty(), "expected visible modal url target");
    let target = targets.last().expect("expected modal url target").clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}

#[test]
fn repeated_plain_click_on_same_modal_url_link_is_throttled() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = format!(
        "https://auth.openai.com/oauth/authorize?client_id=test&state={}",
        "abcdefghijklmnopqrstuvwxyz".repeat(10)
    );
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(list_auth_overlay(&url)),
    });

    let _ = rendered_session_lines(&mut session, VIEW_ROWS);
    let target = session
        .modal_link_targets()
        .iter()
        .find(|target| matches!(&target.target, TranscriptLinkTarget::Url(clicked) if clicked == &url))
        .expect("expected modal url target")
        .clone();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: target.area.x,
        row: target.area.y,
        modifiers: KeyModifiers::NONE,
    };
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(&mut session, &tx, click.column, click.row, click.modifiers);
    left_click_session(&mut session, &tx, click.column, click.row, click.modifiers);

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn double_click_emits_open_url_event_for_standard_modal_auth_link() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = format!(
        "https://auth.openai.com/oauth/authorize?client_id=test&state={}",
        "abcdefghijklmnopqrstuvwxyz".repeat(10)
    );
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(list_auth_overlay(&url)),
    });

    let _ = rendered_session_lines(&mut session, VIEW_ROWS);
    let target = session
        .modal_link_targets()
        .iter()
        .find(|target| matches!(&target.target, TranscriptLinkTarget::Url(clicked) if clicked == &url))
        .expect("expected modal url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: target.area.x,
        row: target.area.y,
        modifiers: KeyModifiers::NONE,
    };
    left_click_session(&mut session, &tx, click.column, click.row, click.modifiers);
    left_click_session(&mut session, &tx, click.column, click.row, click.modifiers);

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn modifier_click_emits_open_file_event_for_standard_modal_file_link_with_location() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let file_target = format!("{absolute_path}#L12C4");
    let canonical_target = format!("{absolute_path}:12:4");
    session.handle_command(InlineCommand::ShowOverlay {
        request: Box::new(OverlayRequest::List(ListOverlayRequest {
            title: "Open source file".to_string(),
            lines: vec![format!("Review this file:\n{file_target}")],
            footer_hint: None,
            items: vec![InlineListItem {
                title: "Continue".to_string(),
                subtitle: None,
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::SlashCommand("continue".to_string())),
                search_value: None,
            }],
            selected: Some(InlineListSelection::SlashCommand("continue".to_string())),
            search: None,
            hotkeys: Vec::new(),
        })),
    });

    let _ = rendered_session_lines(&mut session, VIEW_ROWS);
    let target = session
        .modal_link_targets()
        .iter()
        .find(|target| {
            matches!(
                &target.target,
                TranscriptLinkTarget::File(path)
                    if path.path().display().to_string() == absolute_path
                        && path.location_suffix() == Some(":12:4")
            )
        })
        .expect("expected modal file target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        open_file_click_modifiers(),
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == canonical_target
    ));
}

#[test]
fn modifier_click_emits_open_file_event_for_explicit_transcript_file_link() {
    let mut session = Session::new(themed_inline_colors(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let _ = session.decorate_visible_transcript_links(
        vec![TranscriptLine {
            line: Line::from("Open file"),
            explicit_links: vec![RenderedTranscriptLink {
                start: 5,
                end: 9,
                start_col: 5,
                width: 4,
                target: InlineLinkTarget::Url(absolute_path.clone()),
            }],
        }],
        Rect::new(0, 0, 200, 1),
    );
    let target = session
        .transcript_file_link_targets
        .first()
        .expect("expected transcript file target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        open_file_click_modifiers(),
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == absolute_path
    ));
}

#[test]
fn explicit_transcript_file_link_uses_theme_accent_color() {
    let mut session = Session::new(themed_inline_colors(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let decorated = session.decorate_visible_transcript_links(
        vec![TranscriptLine {
            line: Line::from("Open file"),
            explicit_links: vec![RenderedTranscriptLink {
                start: 5,
                end: 9,
                start_col: 5,
                width: 4,
                target: InlineLinkTarget::Url(absolute_path),
            }],
        }],
        Rect::new(0, 0, 200, 1),
    );

    let linked_span = decorated[0]
        .spans
        .iter()
        .find(|span| span.content == "file")
        .expect("expected explicit linked span");

    assert_eq!(
        linked_span.style.fg,
        themed_inline_colors()
            .tool_accent
            .map(ratatui_color_from_ansi)
    );
    assert!(
        linked_span
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED)
    );
}

#[test]
fn meta_click_emits_open_file_event_for_transcript_path() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    // Some terminals (Ghostty, iTerm2) report Cmd as META instead of SUPER
    left_click_session(
        &mut fixture.session,
        &tx,
        area.x,
        area.y,
        KeyModifiers::META,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
}

#[test]
fn ctrl_click_does_not_emit_open_file_event_on_macos() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture.session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::CONTROL,
        }),
        &tx,
        None,
    );

    assert!(rx.try_recv().is_err());
    assert_eq!(fixture.session.mouse_drag_target, MouseDragTarget::None);
    assert!(!fixture.session.mouse_selection.is_selecting);
    assert!(!fixture.session.mouse_selection.has_selection);
}

#[test]
fn command_key_press_then_plain_click_emits_open_file_event_on_macos() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture.session.handle_event(
        CrosstermEvent::Key(command_modifier_press_event()),
        &tx,
        None,
    );
    left_click_session(
        &mut fixture.session,
        &tx,
        area.x,
        area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));

    fixture.session.handle_event(
        CrosstermEvent::Key(command_modifier_release_event()),
        &tx,
        None,
    );
    left_click_session(
        &mut fixture.session,
        &tx,
        area.x,
        area.y,
        KeyModifiers::NONE,
    );

    assert!(rx.try_recv().is_err());
}

#[test]
fn meta_key_press_then_plain_click_emits_open_file_event_on_macos() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture
        .session
        .handle_event(CrosstermEvent::Key(meta_modifier_press_event()), &tx, None);
    left_click_session(
        &mut fixture.session,
        &tx,
        area.x,
        area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
}

#[test]
fn app_session_modifier_click_emits_open_file_event_for_transcript_path() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    left_click_app_session(
        &mut fixture.session,
        &tx,
        area.x,
        area.y,
        open_file_click_modifiers(),
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    assert_eq!(
        fixture.session.core.mouse_drag_target,
        MouseDragTarget::None
    );
    assert!(!fixture.session.core.mouse_selection.is_selecting);
    assert!(!fixture.session.core.mouse_selection.has_selection);
}

#[test]
fn app_session_double_click_emits_open_file_event_for_transcript_path() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    };
    left_click_app_session(
        &mut fixture.session,
        &tx,
        click.column,
        click.row,
        click.modifiers,
    );
    left_click_app_session(
        &mut fixture.session,
        &tx,
        click.column,
        click.row,
        click.modifiers,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn app_session_double_click_emits_open_url_event_for_modal_auth_link() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://auth.openai.com/oauth/authorize?client_id=test".to_string();
    session.show_transient(wizard_auth_transient(&url));

    let _ = rendered_app_session_lines(&mut session, 20);
    let target = session
        .core
        .modal_link_targets()
        .first()
        .expect("expected modal url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: target.area.x,
        row: target.area.y,
        modifiers: KeyModifiers::NONE,
    };
    left_click_app_session(&mut session, &tx, click.column, click.row, click.modifiers);
    left_click_app_session(&mut session, &tx, click.column, click.row, click.modifiers);

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
    assert!(rx.try_recv().is_err());
}

#[test]
fn app_session_command_key_press_then_plain_click_emits_open_file_event_on_macos() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture.session.handle_event(
        CrosstermEvent::Key(command_modifier_press_event()),
        &tx,
        None,
    );
    left_click_app_session(
        &mut fixture.session,
        &tx,
        area.x,
        area.y,
        KeyModifiers::NONE,
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
}

#[test]
fn app_session_ctrl_click_on_link_is_consumed_without_selection() {
    let mut fixture = AppSessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    fixture.session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::CONTROL,
        }),
        &tx,
        None,
    );

    assert!(rx.try_recv().is_err());
    assert_eq!(
        fixture.session.core.mouse_drag_target,
        MouseDragTarget::None
    );
    assert!(!fixture.session.core.mouse_selection.is_selecting);
    assert!(!fixture.session.core.mouse_selection.has_selection);
}

#[test]
fn scroll_between_clicks_clears_double_click_history() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("hello world")]);

    let (transcript_area, rendered) = rendered_transcript_lines(&mut session, VIEW_ROWS * 2);
    let row = rendered
        .iter()
        .position(|line| line.contains("hello world"))
        .expect("expected hello world to be rendered");
    let column = rendered[row]
        .find("hello")
        .expect("expected hello word in rendered line") as u16
        + transcript_area.x
        + 1;
    let row = transcript_area.y + row as u16;

    let (tx, _rx) = mpsc::unbounded_channel();
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let release = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let scroll = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };

    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(scroll), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(click), &tx, None);
    session.handle_event(CrosstermEvent::Mouse(release), &tx, None);

    assert!(!session.mouse_selection.has_selection);
}

#[test]
fn path_with_line_col_suffix_resolves_correctly() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let absolute_path = transcript_file_fixture_absolute_path();
    let path_with_loc = format!("{}:42:10", absolute_path);
    session.push_line(
        InlineMessageKind::Agent,
        vec![make_segment(&format!("Error at {}", path_with_loc))],
    );

    let decorated = session.decorate_visible_transcript_links(
        vec![transcript_line(format!("Error at {}", path_with_loc))],
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
    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        open_file_click_modifiers(),
    );
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
    let path_with_loc = format!("{}(10,5)", absolute_path);

    let _ = session.decorate_visible_transcript_links(
        vec![transcript_line(format!("Error at {}", path_with_loc))],
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
        vec![transcript_line(format!("Error at {}", hash_path))],
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
    left_click_session(
        &mut session,
        &tx,
        target.area.x,
        target.area.y,
        open_file_click_modifiers(),
    );

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == format!("{absolute_path}:12:4")
    ));
}

#[test]
fn abbreviation_tokens_are_not_detected_as_paths() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let _ = session.decorate_visible_transcript_links(
        vec![transcript_line("e.g. this or i.e. that")],
        Rect::new(0, 0, 200, 1),
    );

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
    let modal_area = session.modal_list_area.expect("modal list area");
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
            row: panel_area.y + 4,
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
    load_app_file_palette(
        &mut session,
        vec![
            workspace
                .join("src/core_tui/session.rs")
                .display()
                .to_string(),
        ],
        workspace.clone(),
    );
    session.handle_command(app_types::InlineCommand::SetInput("@".to_string()));

    let _ = rendered_app_session_lines(&mut session, 20);
    let panel_area = session.core.bottom_panel_area().expect("panel area");
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: panel_area.x,
            row: panel_area.y + 5,
            modifiers: KeyModifiers::NONE,
        }),
        &event_tx,
        None,
    );

    assert_eq!(
        session.core.input_manager.content(),
        "@src/core_tui/session.rs "
    );
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
    let input_area = session.input_area.expect("input area");
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

    let input_area = session.input_area.expect("input area");
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

    let input_area = session.input_area.expect("input area");
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
        view.iter()
            .any(|line| line.contains(&format!("{LABEL_PREFIX}-9"))),
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
