use super::super::*;
use super::helpers::*;

#[test]
fn diff_overlay_defaults_to_edit_approval_mode() {
    let preview = app_types::DiffPreviewState::new(
        "src/main.rs".to_string(),
        "before".to_string(),
        "after".to_string(),
        Vec::new(),
    );

    assert_eq!(preview.mode, app_types::DiffPreviewMode::EditApproval);
}

#[test]
fn diff_overlay_edit_approval_keys_remain_unchanged() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::EditApproval);
    let apply = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        apply,
        Some(app_types::InlineEvent::Transient(
            app_types::TransientEvent::Submitted(app_types::TransientSubmission::DiffApply)
        ))
    ));

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::EditApproval);
    let reload = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(reload.is_none());
    assert!(session.diff_preview_state().is_some());

    let reject = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(
        reject,
        Some(app_types::InlineEvent::Transient(
            app_types::TransientEvent::Submitted(app_types::TransientSubmission::DiffReject)
        ))
    ));
}

#[test]
fn diff_overlay_conflict_mode_maps_enter_reload_and_escape() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::FileConflict);
    let proceed = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        proceed,
        Some(app_types::InlineEvent::Transient(
            app_types::TransientEvent::Submitted(app_types::TransientSubmission::DiffProceed)
        ))
    ));

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::FileConflict);
    let reload = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(matches!(
        reload,
        Some(app_types::InlineEvent::Transient(
            app_types::TransientEvent::Submitted(app_types::TransientSubmission::DiffReload)
        ))
    ));

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::FileConflict);
    let abort = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(
        abort,
        Some(app_types::InlineEvent::Transient(
            app_types::TransientEvent::Submitted(app_types::TransientSubmission::DiffAbort)
        ))
    ));
}

#[test]
fn diff_overlay_conflict_mode_ignores_trust_shortcuts() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::FileConflict);
    let event = session.process_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(matches!(
        session.diff_preview_state().map(|preview| preview.mode),
        Some(app_types::DiffPreviewMode::FileConflict)
    ));
}

#[test]
fn diff_overlay_readonly_review_maps_enter_and_escape_to_back() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::ReadonlyReview);
    let enter = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        enter,
        Some(app_types::InlineEvent::Transient(
            app_types::TransientEvent::Submitted(app_types::TransientSubmission::DiffAbort)
        ))
    ));

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::ReadonlyReview);
    let escape = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(
        escape,
        Some(app_types::InlineEvent::Transient(
            app_types::TransientEvent::Submitted(app_types::TransientSubmission::DiffAbort)
        ))
    ));
}

#[test]
fn diff_overlay_readonly_review_ignores_reload_shortcut() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::ReadonlyReview);
    let reload = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));

    assert!(reload.is_none());
    assert!(matches!(
        session.diff_preview_state().map(|preview| preview.mode),
        Some(app_types::DiffPreviewMode::ReadonlyReview)
    ));
}

#[test]
fn diff_preview_suspends_task_panel_and_restores_it_on_close() {
    let mut session = AppSession::new(InlineTheme::default(), None, 30);
    session.task_panel_lines = vec!["Queued task".to_string()];
    session.set_task_panel_visible(true);

    let backend = TestBackend::new(VIEW_WIDTH, 30);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render task panel");
    assert!(session.core.bottom_panel_area().is_some());

    show_diff_overlay(&mut session, app_types::DiffPreviewMode::ReadonlyReview);
    assert!(!session.core.input_enabled());
    assert!(
        !session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );

    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render diff preview");
    assert!(
        session.core.bottom_panel_area().is_none(),
        "floating diff preview should hide the lower bottom panel"
    );

    session.close_diff_overlay();
    assert!(session.core.input_enabled());
    assert!(
        session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );

    let lines = rendered_app_session_lines(&mut session, 30);
    assert!(
        lines.iter().any(|line| line.contains("Queued task")),
        "task panel should resume after closing diff preview"
    );
}
