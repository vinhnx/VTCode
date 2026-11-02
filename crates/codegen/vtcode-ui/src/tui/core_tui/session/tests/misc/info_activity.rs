use super::*;

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
fn info_warning_error_lines_render_without_borders() {
    let cases = [
        InlineMessageKind::Info,
        InlineMessageKind::Warning,
        InlineMessageKind::Error,
    ];

    for kind in cases {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        session.push_line(kind, vec![make_segment("Theme switched to Ciapre")]);

        let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

        // No bordered block: no centered labels and no horizontal rules.
        for label in [" Info ", " Warning ", " Error "] {
            assert!(
                rendered.iter().all(|line| !line.contains(label)),
                "{kind:?} group must not render a '{label}' label, got: {rendered:?}"
            );
        }
        assert!(
            rendered.iter().all(|line| !is_horizontal_rule(line)),
            "{kind:?} group must not render horizontal rules, got: {rendered:?}"
        );
        assert!(
            rendered.iter().all(|line| {
                let trimmed = line.trim();
                trimmed.is_empty() || trimmed.chars().any(|ch| !matches!(ch, '─' | '━' | '/' | '=' | '-'))
            }),
            "{kind:?} group must not render rule fills, got: {rendered:?}"
        );
        assert!(
            rendered.iter().all(|line| !line.contains('│')),
            "{kind:?} group must not draw vertical sides, got: {rendered:?}"
        );
        // The content itself is preserved.
        assert!(
            rendered.iter().any(|line| line.contains("Theme switched to Ciapre")),
            "{kind:?} group content should be preserved, got: {rendered:?}"
        );
    }
}
#[test]
fn info_warning_error_lines_use_semantic_colors() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Error, vec![make_segment("error content")]);
    session.push_line(InlineMessageKind::Warning, vec![make_segment("warning content")]);
    session.push_line(InlineMessageKind::Info, vec![make_segment("info content")]);

    let lines = session.reflow_transcript_lines(80);
    let find_span = |needle: &str| {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.content.contains(needle))
            .unwrap_or_else(|| panic!("expected span containing '{needle}'"))
    };

    let error_span = find_span("error content");
    assert_eq!(error_span.style.fg, Some(Color::Red), "error should render red");
    assert!(
        !error_span.style.add_modifier.contains(Modifier::DIM),
        "error should stay saturated, got {:?}",
        error_span.style.add_modifier
    );

    let warning_span = find_span("warning content");
    assert_eq!(warning_span.style.fg, Some(Color::Yellow), "warning should render amber/yellow");
    assert!(
        !warning_span.style.add_modifier.contains(Modifier::DIM),
        "warning should stay saturated, got {:?}",
        warning_span.style.add_modifier
    );

    let info_span = find_span("info content");
    assert!(
        info_span.style.add_modifier.contains(Modifier::DIM),
        "info should render dimmed, got {:?}",
        info_span.style.add_modifier
    );
}
#[test]
fn active_file_operation_indicator_shimmers_without_losing_text() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Info, vec![make_segment("❋ Editing vtcode.toml...")]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: edit_file".to_string()),
        right: None,
    });
    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered.iter().any(|line| line.contains("❋ Editing vtcode.toml...")),
        "shimmer must restyle the indicator row, never eat its text, got: {rendered:?}"
    );
}
#[test]
fn reduced_motion_keeps_file_operation_marker_static() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.reduce_motion_mode = true;
    session.push_line(InlineMessageKind::Info, vec![make_segment("❋ Editing vtcode.toml...")]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: edit_file".to_string()),
        right: None,
    });

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);
    let animated = format!("{} Editing vtcode.toml...", pulse_spinner_frame_for_phase(0.0));

    assert!(
        rendered.iter().any(|line| line.contains("❋ Editing vtcode.toml...")),
        "reduced motion should keep the file operation label visible"
    );
    assert!(
        !rendered.iter().any(|line| line.contains(&animated)),
        "reduced motion should not replace the static file marker with a spinner"
    );
}
#[test]
fn non_file_tool_status_keeps_static_file_operation_indicator() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Info, vec![make_segment("❋ Editing vtcode.toml...")]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: code_search".to_string()),
        right: None,
    });

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered.iter().any(|line| line.contains("❋ Editing vtcode.toml...")),
        "non-file tool activity should not animate stale file operation indicators"
    );
}
#[test]
fn planning_indicator_shimmers_without_losing_text() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Info, vec![make_segment("❋ Drafting plan — researching codebase...")]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Drafting plan... (42 chars)".to_string()),
        right: None,
    });
    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("❋ Drafting plan — researching codebase...")),
        "planning shimmer must restyle the row, never eat its text, got: {rendered:?}"
    );
}
#[test]
fn planning_indicator_shimmers_while_validating() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Info, vec![make_segment("❋ Validating plan...")]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Validating plan...".to_string()),
        right: None,
    });
    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered.iter().any(|line| line.contains("❋ Validating plan...")),
        "planning shimmer must restyle the row, never eat its text, got: {rendered:?}"
    );
}
#[test]
fn unrelated_status_keeps_planning_indicator_static() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Info, vec![make_segment("❋ Drafting plan — researching codebase...")]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: code_search".to_string()),
        right: None,
    });

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered.iter().any(|line| line.contains("❋ Drafting plan")),
        "unrelated tool activity should not animate a stale planning indicator"
    );
}
#[test]
fn reduced_motion_keeps_planning_marker_static() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.reduce_motion_mode = true;
    session.push_line(InlineMessageKind::Info, vec![make_segment("❋ Drafting plan — researching codebase...")]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Drafting plan... (42 chars)".to_string()),
        right: None,
    });

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);
    let animated = format!("{} Drafting plan — researching codebase...", pulse_spinner_frame_for_phase(0.0));

    assert!(
        rendered.iter().any(|line| line.contains("❋ Drafting plan")),
        "reduced motion should keep the planning label visible"
    );
    assert!(
        !rendered.iter().any(|line| line.contains(&animated)),
        "reduced motion should not replace the static planning marker with a spinner"
    );
}
#[test]
fn empty_enter_with_active_pty_opens_jobs() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(matches!(event, Some(InlineEvent::Submit(ref value)) if value == "/jobs"));
}
#[test]
fn active_pty_observer_drives_compact_loading_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let active_pty_sessions = Arc::new(AtomicUsize::new(1));
    session.active_pty_sessions = Some(Arc::clone(&active_pty_sessions));
    session.handle_command(InlineCommand::SetInputStatus { left: Some("main*".to_string()), right: None });

    assert_eq!(session.status_left_text(), Some("Running PTY command..."));
    assert!(session.has_status_spinner());
    assert!(!session.is_running_activity(), "foreground command status must not mark an idle turn busy");

    active_pty_sessions.store(0, Ordering::Relaxed);

    assert_eq!(session.status_left_text(), Some("main*"));
    assert!(!session.has_status_spinner());
    assert!(!session.is_running_activity());
}
