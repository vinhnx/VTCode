use super::*;

#[test]
fn down_opens_local_agents_drawer_when_input_is_empty() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });
    session.close_transient();

    let event = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(session.local_agents_visible());
}
#[test]
fn new_local_agent_auto_opens_drawer() {
    let mut session = app_session_with_input("", 0);

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });

    assert!(session.local_agents_visible());
}
#[test]
fn local_agents_window_captures_keys_but_keeps_status_line() {
    let mut session = app_session_with_input("draft command", "draft command".len());

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });

    assert!(session.local_agents_visible());
    assert!(!session.core.input_enabled());
    assert!(!session.core.build_input_widget_data(VIEW_WIDTH, 1).cursor_should_be_visible);

    let lines = rendered_app_session_lines(&mut session, 20);
    // Inline bottom-dock: the input/status region stays painted above the
    // panel so the background indicator remains clickable.
    assert!(session.core.input_area().is_some());
    assert!(session.core.bottom_panel_area().is_some());
    assert!(lines.iter().any(|line| line.contains("Background")), "inline panel should render, got: {lines:?}");
}
#[test]
fn closing_local_agents_drawer_restores_input_and_draft() {
    let mut session = app_session_with_input("draft command", "draft command".len());

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });
    let _ = rendered_app_session_lines(&mut session, 20);

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(!session.local_agents_visible());
    assert!(session.core.input_enabled());
    assert!(session.core.build_input_widget_data(VIEW_WIDTH, 1).cursor_should_be_visible);
    assert_eq!(session.core.input_manager.content(), "draft command");

    let lines = rendered_app_session_lines(&mut session, 20);
    assert!(session.core.input_area().is_some());
    assert!(
        lines.iter().any(|line| line.contains("draft command")),
        "composer should re-render its preserved draft"
    );
}
#[test]
fn local_agents_drawer_navigation_works_with_existing_draft() {
    let mut session = app_session_with_input("draft command", "draft command".len());

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("agent-1", "rust-engineer", app_types::LocalAgentKind::Delegated),
            sample_local_agent_entry_with_id("agent-2", "qa-reviewer", app_types::LocalAgentKind::Delegated),
        ],
    });

    let down = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(down.is_none());

    let enter = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        enter,
        Some(app_types::InlineEvent::Submit(value)) if value == "/agent inspect agent-2"
    ));
    assert_eq!(session.core.input_manager.content(), "draft command");
}
#[test]
fn new_background_local_agent_does_not_auto_open_drawer() {
    let mut session = app_session_with_input("", 0);

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Background)],
    });

    assert!(!session.local_agents_visible());
}
#[test]
fn exec_session_entry_does_not_auto_open_drawer() {
    let mut session = app_session_with_input("", 0);

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::ExecSession)],
    });

    assert!(!session.local_agents_visible());
}
#[test]
fn mixed_local_agents_keep_exec_session_selection_when_snapshot_changes() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("agent-1", "rust-engineer", app_types::LocalAgentKind::Delegated),
            sample_local_agent_entry_with_id("managed-1", "managed-worker", app_types::LocalAgentKind::Background),
            sample_local_agent_entry_with_id("exec-42", "cargo test", app_types::LocalAgentKind::ExecSession),
        ],
    });
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::LocalAgents(app_types::LocalAgentsTransientRequest {
            visible: Some(true),
        })),
    });

    for _ in 0..2 {
        assert!(session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).is_none());
    }

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("agent-1", "rust-engineer", app_types::LocalAgentKind::Delegated),
            sample_local_agent_entry_with_id("managed-1", "managed-worker", app_types::LocalAgentKind::Background),
            {
                let mut entry = sample_local_agent_entry_with_id(
                    "exec-42",
                    "cargo test --locked",
                    app_types::LocalAgentKind::ExecSession,
                );
                entry.status = crate::tui::core_tui::types::LocalAgentStatus::Exited { code: Some(0) };
                entry.preview = "finished".to_string();
                entry
            },
        ],
    });

    let inspect = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        inspect,
        Some(app_types::InlineEvent::ExecSessionAction { id, action })
            if id == "exec-42" && action == app_types::ExecSessionAction::Inspect
    ));
}
#[test]
fn exec_session_drawer_actions_route_to_runloop_events() {
    for (key, expected_action) in [
        (KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), app_types::ExecSessionAction::Inspect),
        (
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            app_types::ExecSessionAction::GracefulTerminate,
        ),
        (
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            app_types::ExecSessionAction::ForceTerminateOrClose,
        ),
        (KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), app_types::ExecSessionAction::Focus),
        (KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL), app_types::ExecSessionAction::Preview),
    ] {
        let mut session = app_session_with_input("", 0);
        session.handle_command(app_types::InlineCommand::SetLocalAgents {
            entries: vec![sample_local_agent_entry_with_id(
                "exec-42",
                "cargo test",
                app_types::LocalAgentKind::ExecSession,
            )],
        });
        session.handle_command(app_types::InlineCommand::ShowTransient {
            request: Box::new(app_types::TransientRequest::LocalAgents(app_types::LocalAgentsTransientRequest {
                visible: Some(true),
            })),
        });

        let event = session.process_key(key);
        assert!(matches!(
            event,
            Some(app_types::InlineEvent::ExecSessionAction { id, action })
                if id == "exec-42" && action == expected_action
        ));
    }
}
#[test]
fn ctrl_r_still_opens_history_picker_for_non_exec_local_agents() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });

    let event = session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

    assert!(event.is_none());
    assert!(session.history_picker_state.active);
}
#[test]
fn auto_opened_local_agents_window_closes_when_delegated_work_finishes() {
    let mut running = sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated);
    running.status = crate::tui::core_tui::types::LocalAgentStatus::Running;
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![running.clone()] });
    assert!(session.local_agents_visible());

    let mut done = running;
    done.status = crate::tui::core_tui::types::LocalAgentStatus::Completed;
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![done] });
    assert!(!session.local_agents_visible(), "auto-opened window should close once live delegated work finishes");
}
#[test]
fn auto_opened_local_agents_drawer_closes_after_last_delegated_entry_is_removed() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });

    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![] });

    assert!(!session.local_agents_visible());
}
#[test]
fn manually_opened_empty_local_agents_drawer_stays_open() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::LocalAgents(app_types::LocalAgentsTransientRequest {
            visible: Some(true),
        })),
    });
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![] });

    assert!(session.local_agents_visible());

    let lines = rendered_app_session_lines(&mut session, 20);
    assert!(
        lines.iter().any(|line| line.contains("No local agents yet")),
        "drawer should remain visible and show the empty state"
    );
}
