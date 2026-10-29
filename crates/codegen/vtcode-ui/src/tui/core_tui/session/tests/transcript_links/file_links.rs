use super::*;

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
        .find(|span| span.content.contains(transcript_file_fixture_relative_path()))
        .expect("expected linked span");
    assert!(linked_span.style.add_modifier.contains(Modifier::UNDERLINED));
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
        .find(|span| span.content.contains(transcript_file_fixture_relative_path()))
        .expect("expected hovered linked span");
    assert!(linked_span.style.add_modifier.contains(Modifier::UNDERLINED));
    assert!(linked_span.style.add_modifier.contains(Modifier::BOLD));
}
#[test]
fn mixed_transcript_file_references_are_all_underlined() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_workspace_root(Some(vtcode_tui_workspace_root()));
    let temp_file = quoted_transcript_temp_file_path();
    fs::write(&temp_file, "mixed-transcript-link").expect("write mixed transcript temp file");

    let line =
        transcript_line(format!("Open {} and `{}`", transcript_file_fixture_relative_path(), temp_file.display()));
    let temp_file_display = temp_file.display().to_string();
    let decorated = session.decorate_visible_transcript_links(vec![line], Rect::new(0, 0, 200, 1));

    assert_eq!(session.transcript_file_link_targets.len(), 2);
    assert!(decorated[0].spans.iter().any(|span| {
        span.content.contains(transcript_file_fixture_relative_path())
            && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));
    assert!(decorated[0].spans.iter().any(|span| {
        span.content.contains(&temp_file_display) && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));

    let _ = fs::remove_file(&temp_file);
}
#[test]
fn plain_click_emits_open_file_event_for_absolute_transcript_path() {
    let mut fixture = SessionWithFileLink::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let area = fixture.target_area();

    left_click_session(&mut fixture.session, &tx, area.x, area.y, KeyModifiers::NONE);

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

    left_click_session(&mut fixture.session, &tx, click.column, click.row, click.modifiers);
    left_click_session(&mut fixture.session, &tx, click.column, click.row, click.modifiers);

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    rx.try_recv().unwrap_err();
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
        vec![make_segment(&format!("Open {absolute_path} and {quoted_temp_path}"))],
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

    left_click_session(&mut session, &tx, first_target.area.x, first_target.area.y, KeyModifiers::NONE);
    left_click_session(&mut session, &tx, second_target.area.x, second_target.area.y, KeyModifiers::NONE);

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
    left_click_session(&mut fixture.session, &tx, click.column, click.row, click.modifiers);
    left_click_session(&mut fixture.session, &tx, click.column, click.row, click.modifiers);

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenFileInEditor(path)) if path == fixture.path
    ));
    rx.try_recv().unwrap_err();
}
#[test]
fn modifier_click_emits_open_file_event_for_quoted_path_with_spaces() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let temp_file = quoted_transcript_temp_file_path();
    fs::write(&temp_file, "transcript-link").expect("write quoted transcript temp file");
    let quoted_path = format!("`{}`", temp_file.display());
    let _ = session.decorate_visible_transcript_links(
        vec![transcript_line(format!("Open {quoted_path}"))],
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

    left_click_session(&mut session, &tx, target.area.x, target.area.y, KeyModifiers::NONE);

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

    left_click_session(&mut session, &tx, target.area.x, target.area.y, KeyModifiers::NONE);

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

    left_click_session(&mut session, &tx, target.area.x, target.area.y, open_file_click_modifiers());

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

    let transcript_lines = session.reflow_message_lines(0, 60, false);
    let decorated = session.decorate_visible_cached_transcript_links(transcript_lines, Rect::new(0, 0, 60, 8));
    let targets = session
        .transcript_file_link_targets
        .iter()
        .filter(|target| matches!(&target.target, TranscriptLinkTarget::Url(clicked) if clicked == &url))
        .cloned()
        .collect::<Vec<_>>();
    assert!(targets.len() >= 2, "expected wrapped transcript url segments");

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
    left_click_session(&mut session, &tx, target.area.x, target.area.y, open_file_click_modifiers());

    assert!(matches!(
        rx.try_recv(),
        Ok(InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}
#[test]
fn reflowed_tool_lines_include_detected_raw_links() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = "https://example.com/tool-output".to_string();
    session.push_line(InlineMessageKind::Tool, vec![make_segment(&format!("open {url}"))]);

    let transcript_lines = session.reflow_message_lines(0, 80, false);

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

    left_click_app_session(&mut session, &tx, target.area.x, target.area.y, open_file_click_modifiers());

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
    assert!(!targets.is_empty(), "expected visible wizard modal url target");

    let target = targets
        .iter()
        .max_by_key(|target| (target.area.y, target.area.x))
        .expect("expected wrapped wizard modal url target")
        .clone();
    let (tx, mut rx) = mpsc::unbounded_channel();

    left_click_app_session(&mut session, &tx, target.area.x, target.area.y, KeyModifiers::NONE);

    assert!(matches!(
        rx.try_recv(),
        Ok(app_types::InlineEvent::OpenUrl(clicked)) if clicked == url
    ));
}
#[test]
fn modal_auth_text_in_app_session_is_selectable_and_copied() {
    let _guard = CLIPBOARD_TEST_LOCK.lock().expect("clipboard test lock should not be poisoned");

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
    terminal.draw(|frame| session.render(frame)).expect("render modal selection");

    let buffer = terminal.backend().buffer();
    assert_eq!(session.core.mouse_selection.extract_text(buffer, buffer.area), "https");
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
    session.handle_command(InlineCommand::ShowOverlay { request: Box::new(list_auth_overlay(&url)) });

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

    left_click_session(&mut session, &tx, target.area.x, target.area.y, KeyModifiers::NONE);

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
    session.handle_command(InlineCommand::ShowOverlay { request: Box::new(list_auth_overlay(&url)) });

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
    rx.try_recv().unwrap_err();
}
#[test]
fn double_click_emits_open_url_event_for_standard_modal_auth_link() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let url = format!(
        "https://auth.openai.com/oauth/authorize?client_id=test&state={}",
        "abcdefghijklmnopqrstuvwxyz".repeat(10)
    );
    session.handle_command(InlineCommand::ShowOverlay { request: Box::new(list_auth_overlay(&url)) });

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
    rx.try_recv().unwrap_err();
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
                ..Default::default()
            }],
            selected: Some(InlineListSelection::SlashCommand("continue".to_string())),
            search: None,
            hotkeys: Vec::new(),
            status: None,
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

    left_click_session(&mut session, &tx, target.area.x, target.area.y, open_file_click_modifiers());

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

    left_click_session(&mut session, &tx, target.area.x, target.area.y, open_file_click_modifiers());

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

    assert_eq!(linked_span.style.fg, themed_inline_colors().tool_accent.map(ratatui_color_from_ansi));
    assert!(linked_span.style.add_modifier.contains(Modifier::UNDERLINED));
}
