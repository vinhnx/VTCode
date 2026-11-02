use super::*;

#[test]
fn alt_g_toggles_task_panel() {
    let mut session = build_session();
    assert!(!session.show_task_panel);

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    assert!(session.show_task_panel, "Alt+G should reveal the task panel");

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    assert!(!session.show_task_panel, "Second Alt+G should hide the task panel");
}
#[test]
fn task_panel_visibility_requests_retain_body_and_metadata_progress() {
    use crate::tui::core_tui::app::types::{TaskPanelMetadata, TaskPanelTransientRequest, TransientRequest};
    use vtcode_commons::ui_protocol::TaskItemStatus;

    let mut session = build_session();
    let tree = vec!["  └ Implement".to_string(), "  └ Verify".to_string()];
    let statuses = vec![TaskItemStatus::Pending, TaskItemStatus::Completed];
    let metadata = TaskPanelMetadata {
        title: "Release".to_string(),
        completed: 1,
        total: 2,
    };

    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: tree.clone(),
        statuses: statuses.clone(),
        current: Some(0),
        visible: None,
        metadata: Some(metadata),
    }));
    assert_eq!(session.task_panel_lines, tree);
    assert_eq!(session.task_panel_statuses, statuses);
    assert_eq!(session.task_panel_current, Some(0));
    assert_eq!(session.task_panel_metadata.as_ref().map(|m| m.title.as_str()), Some("Release"));

    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: Vec::new(),
        statuses: Vec::new(),
        current: None,
        visible: Some(true),
        metadata: None,
    }));
    // Appearance may suppress auto-show; visibility must never wipe content.
    assert_eq!(session.task_panel_lines, tree, "show_task_panel must not wipe the panel body");
    assert_eq!(session.task_panel_statuses, statuses, "show_task_panel must not wipe row statuses");
    assert_eq!(session.task_panel_current, Some(0));
    assert_eq!(session.task_panel_metadata.as_ref().map(|m| m.completed), Some(1));

    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: Vec::new(),
        statuses: Vec::new(),
        current: None,
        visible: Some(false),
        metadata: None,
    }));
    assert!(!session.show_task_panel);
    assert_eq!(session.task_panel_lines, tree, "hide_task_panel must not wipe the panel body");

    // Session clear is a content update without metadata: it must drop
    // stale panel metadata so the terminal title cannot keep old N/M.
    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: Vec::new(),
        statuses: Vec::new(),
        current: None,
        visible: None,
        metadata: None,
    }));
    assert!(session.task_panel_lines.is_empty());
    assert!(session.task_panel_statuses.is_empty(), "clear must drop row statuses");
    assert_eq!(session.task_panel_current, None);
    assert!(session.task_panel_metadata.is_none(), "clear must drop panel metadata");
}
#[test]
fn ctrl_home_and_end_jump_transcript_in_fullscreen() {
    let mut session = build_session();
    for index in 0..40 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("line {index}"))]);
    }

    session.core.scroll_page_up();
    assert!(session.core.scroll_offset() > 0);

    let _ = session.process_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    assert_eq!(session.core.scroll_offset(), 0);

    let _ = session.process_key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL));
    assert_eq!(session.core.scroll_offset(), session.core.current_max_scroll_offset());
}
#[test]
fn tool_output_viewer_search_accept_and_cancel_work() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec![
            "• Ran alpha".to_string(),
            "  └ beta alpha".to_string(),
            "  └ gamma alpha".to_string(),
        ],
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in ['a', 'l', 'p', 'h', 'a'] {
        let _ = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    let status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(status.contains("search 'alpha'"));
    assert!(status.contains("(2/3)"));

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    let _ = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    let status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(status.contains("search 'alpha'"));
}
#[test]
fn tool_output_viewer_exports_complete_tool_output() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec![
            "• Ran printf complete".to_string(),
            "  └ first complete line".to_string(),
            "    second complete line".to_string(),
        ],
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    match session.process_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)) {
        Some(InlineEvent::OpenToolOutputInEditor(text)) => {
            assert!(text.contains("first complete line"));
            assert!(text.contains("second complete line"));
        }
        other => panic!("unexpected viewer editor event: {other:?}"),
    }

    match session.process_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE)) {
        Some(InlineEvent::OpenToolOutputScrollback(text)) => {
            assert!(text.contains("first complete line"));
            assert!(text.contains("second complete line"));
        }
        other => panic!("unexpected viewer scrollback event: {other:?}"),
    }
}
#[test]
fn tool_output_viewer_scrolls_copies_selection_and_closes() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: (0..20).map(|index| format!("output line {index}")).collect(),
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    let initial_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    let top_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    let next_status = session.tool_output_viewer_state().expect("viewer open").status_label();

    assert!(initial_status.contains("line 13/20"));
    assert!(top_status.contains("line 1/20"));
    assert!(next_status.contains("line 2/20"));

    session.core.mouse_selection.set_selection((1, 1), (8, 1));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(session.core.mouse_selection.has_copy_request());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(session.tool_output_viewer_state().is_none());
}
#[test]
fn tool_output_viewer_mouse_scroll_does_not_move_transcript() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: (0..20).map(|index| format!("output line {index}")).collect(),
        ..Default::default()
    });
    session.tool_output_revision = 1;
    for index in 0..20 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("transcript line {index}"))]);
    }

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    let transcript_offset = session.core.scroll_offset();
    let viewer_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &tx,
        None,
    );

    assert_eq!(session.core.scroll_offset(), transcript_offset);
    assert_ne!(session.tool_output_viewer_state().expect("viewer open").status_label(), viewer_status);
}
#[test]
fn mouse_events_are_ignored_when_fullscreen_mouse_capture_is_disabled() {
    let mut session = build_session();
    session.core.fullscreen.interaction.mouse_capture = false;
    for index in 0..20 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("line {index}"))]);
    }
    session.core.scroll_page_up();
    let initial_offset = session.core.scroll_offset();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &tx,
        None,
    );

    assert_eq!(session.core.scroll_offset(), initial_offset);
}
#[test]
fn transcript_wheel_scrolls_through_floating_plan_overlay() {
    let mut session = build_session();
    for index in 0..40 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("plan line {index}"))]);
    }
    session.show_transient(TransientRequest::List(crate::tui::core_tui::app::types::ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines: vec!["A plan is ready to execute.".to_string()],
        footer_hint: None,
        items: vec![crate::tui::core_tui::types::InlineListItem {
            title: "Yes".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: None,
            search_value: None,
            ..Default::default()
        }],
        selected: None,
        search: None,
        hotkeys: Vec::new(),
        status: None,
    }));

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render overlay");
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );

    assert!(session.core.scroll_offset() > 0, "transcript should remain scrollable behind the overlay");
}
#[test]
fn control_g_from_plan_approval_overlay_emits_launch_editor_hotkey() {
    use crate::tui::core_tui::app::types::{
        ListOverlayRequest, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey,
    };
    use crate::tui::core_tui::types::{InlineListItem, InlineListSelection};

    let mut session = build_session();
    session.show_transient(TransientRequest::List(ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines: vec!["A plan is ready to execute. Would you like to proceed?".to_string()],
        footer_hint: Some("ctrl-g to edit in VS Code · .vtcode/plans/test-plan.md".to_string()),
        items: vec![InlineListItem {
            title: "Yes, implement this plan".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalExecute),
            search_value: None,
            ..Default::default()
        }],
        selected: Some(InlineListSelection::PlanApprovalExecute),
        search: None,
        hotkeys: vec![TransientHotkey {
            key: TransientHotkeyKey::CtrlChar('g'),
            action: TransientHotkeyAction::LaunchEditor,
        }],
        status: None,
    }));
    assert!(session.has_active_overlay(), "plan approval overlay should be open");
    assert!(
        !session.core.input_enabled(),
        "modal focus must disable the composer so Ctrl+G hits the overlay hotkey, not the draft editor"
    );

    let event = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert!(
        matches!(
            &event,
            Some(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Hotkey(
                TransientHotkeyAction::LaunchEditor
            ))))
        ),
        "Ctrl+G must emit the plan-file hotkey, got {event:?}"
    );
    // The TUI closes the overlay on hotkey; the plan-approval runloop owns
    // re-showing it after the external editor closes (`show_overlay_and_wait`).
    assert!(!session.has_active_overlay(), "overlay closes locally; runloop re-shows after editing");
}
#[test]
fn control_g_without_overlay_still_opens_composer_draft_editor() {
    let mut session = build_session();
    assert!(!session.has_active_overlay());

    let event = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert!(
        matches!(&event, Some(InlineEvent::LaunchEditor { draft }) if draft.is_empty()),
        "composer Ctrl+G must keep the draft-editor path, got {event:?}"
    );
}
