#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;

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
                entry.status = "exited (0)".to_string();
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
    running.status = "running".to_string();
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![running.clone()] });
    assert!(session.local_agents_visible());

    let mut done = running;
    done.status = "completed".to_string();
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

#[test]
fn alt_s_remains_subprocesses_entrypoint() {
    let mut session = app_session_with_input("", 0);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));

    assert!(matches!(
        event,
        Some(app_types::InlineEvent::Submit(value)) if value == "/subprocesses"
    ));
}

#[test]
fn tab_cycles_primary_agent_when_composer_is_empty() {
    // Plain Tab now enqueues like Ctrl+Enter (empty idle -> ProcessLatestQueued);
    // agent cycling lives on Shift+Tab (BackTab).
    let mut session = app_session_with_input("", 0);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(app_types::InlineEvent::ProcessLatestQueued)));

    let cycle = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgentPrevious)));
}

#[test]
fn tab_character_cycles_primary_agent_when_composer_is_empty() {
    // Char('\t') without Shift enqueues; Char('\t')+Shift cycles.
    let mut session = app_session_with_input("", 0);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('\t'), KeyModifiers::NONE));

    assert!(matches!(event, Some(app_types::InlineEvent::ProcessLatestQueued)));

    let cycle = session.process_key(KeyEvent::new(KeyCode::Char('\t'), KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgent)));
}

#[test]
fn core_tab_cycles_primary_agent_when_composer_is_empty() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(InlineEvent::ProcessLatestQueued)));

    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(InlineEvent::CyclePrimaryAgent)));
}

#[test]
fn tab_does_not_cycle_primary_agent_while_running() {
    // Plain Tab enqueues (empty busy -> None), Shift+Tab cycling stays locked.
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);
    set_app_session_busy_status(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "");
}

#[test]
fn tab_does_not_cycle_primary_agent_while_running_with_draft() {
    // Busy Tab with draft queues like Ctrl+Enter instead of cycling.
    let mut session = app_session_with_input("Review this", "Review this".len());
    load_primary_agent_palette(&mut session);
    set_app_session_busy_status(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(
        event,
        Some(app_types::InlineEvent::QueueSubmit(value)) if value.text == "Review this"
    ));
    assert_eq!(session.core.input_manager.content(), "");
}

#[test]
fn shift_tab_does_not_cycle_primary_agent_while_running() {
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);
    set_app_session_busy_status(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert!(event.is_none());
}

#[test]
fn tab_cycles_primary_agent_back_to_default_after_last_agent() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetPrimaryAgent { name: Some("beta".to_string()), color: None });

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(event, Some(app_types::InlineEvent::CyclePrimaryAgent)));
}

#[test]
fn tab_accepts_inline_prompt_suggestion_before_primary_agent_cycle() {
    let mut session = app_session_with_input("Review the current", "Review the current".len());
    load_primary_agent_palette(&mut session);
    session
        .core
        .set_inline_prompt_suggestion("Review the current diff".to_string(), true);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.core.input_manager.content(), "Review the current diff");
    assert!(session.core.inline_prompt_suggestion.suggestion.is_none());
}

#[test]
fn tab_cycles_primary_agent_with_draft() {
    // Plain Tab submits the draft like Ctrl+Enter; Shift+Tab cycles.
    let mut session = app_session_with_input("Review this", "Review this".len());
    load_primary_agent_palette(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(
        event,
        Some(app_types::InlineEvent::Submit(value)) if value == "Review this"
    ));
    assert_eq!(session.core.input_manager.content(), "");

    let mut session = app_session_with_input("Review this", "Review this".len());
    load_primary_agent_palette(&mut session);
    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgent)));
    assert_eq!(session.core.input_manager.content(), "Review this");
}

#[test]
fn tab_cycles_primary_agent_when_queued_input_exists() {
    // Plain Tab with empty draft tries the queue path; Shift+Tab cycles.
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);
    set_app_session_queued_inputs(&mut session, vec!["queued follow-up".to_string()]);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(app_types::InlineEvent::ProcessLatestQueued)));
    assert_eq!(session.core.queued_inputs, vec!["queued follow-up"]);

    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(app_types::InlineEvent::CyclePrimaryAgent)));
}

#[test]
fn shift_tab_cycles_previous_primary_agent() {
    let mut session = app_session_with_input("", 0);
    load_primary_agent_palette(&mut session);

    let event = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert!(matches!(event, Some(app_types::InlineEvent::CyclePrimaryAgentPrevious)));
}

#[test]
fn tab_does_not_cycle_primary_agent_in_building_recovery_or_blocked_states() {
    for state in [ActivityState::Building, ActivityState::Recovery, ActivityState::Blocked] {
        // Plain Tab never cycles (it enqueues); Shift+Tab cycling stays locked.
        let mut session = app_session_with_input("", 0);
        load_primary_agent_palette(&mut session);
        session.handle_command(app_types::InlineCommand::SetActivityState(state));

        let tab = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(
            !matches!(
                tab,
                Some(app_types::InlineEvent::CyclePrimaryAgent | app_types::InlineEvent::CyclePrimaryAgentPrevious)
            ),
            "plain Tab must not cycle in {state:?}, got {tab:?}"
        );

        let shift_tab = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert!(shift_tab.is_none(), "mode switching must stay locked in {state:?}");
    }
}

#[test]
fn header_suggestions_include_subagent_shortcuts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)];

    let line = session.header_suggestions_line().expect("header suggestions line");
    let rendered = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert!(rendered.contains("Alt+S"));
    assert!(rendered.contains("Ctrl+B"));
}

#[test]
fn header_suggestions_hide_subagent_shortcuts_without_agents() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let line = session.header_suggestions_line().expect("header suggestions line");
    let rendered = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert!(!rendered.contains("Alt+S"));
    assert!(!rendered.contains("Ctrl+B"));
}

#[test]
fn header_suggestions_hide_subagent_shortcuts_with_background_only() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(app_types::LocalAgentKind::Background)];

    let line = session.header_suggestions_line().expect("header suggestions line");
    let rendered = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert!(!rendered.contains("Alt+S"));
    assert!(!rendered.contains("Ctrl+B"));
}

#[test]
fn header_suggestions_show_background_shortcut_when_foreground_pty_active() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    let line = session.header_suggestions_line().expect("header suggestions line");
    let rendered = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert!(rendered.contains("Ctrl+B"), "foreground PTY must surface Ctrl+B, got: {rendered:?}");
    assert!(rendered.contains("background"));
    assert!(!rendered.contains("Alt+S"));
}

#[test]
fn foreground_pty_hint_leaves_bottom_line_for_header_while_running() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("cargo check".to_string());
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    // Busy bottom line keeps only stable context: no PTY hint flicker even
    // with a non-empty composer.
    let line = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let rendered = line_text(&line);
    assert!(!rendered.contains("Ctrl+B"), "busy bottom line must not flicker PTY hint, got: {rendered:?}");
    assert!(!rendered.contains("background"), "busy bottom line keeps only stable context, got: {rendered:?}");

    // Discovery moves to the header while a foreground command runs.
    let header = session.header_suggestions_line().expect("header suggestions line");
    let header_text = line_text(&header);
    assert!(header_text.contains("Ctrl+B"), "header must keep one-click discovery, got: {header_text:?}");
    assert!(header_text.contains("background"), "header must explain background, got: {header_text:?}");
}

#[test]
fn foreground_pty_hint_surfaces_once_in_header_while_running() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)];
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    // While the foreground command runs the bottom line stays clean; the
    // header carries each shortcut exactly once.
    let line = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let rendered = line_text(&line);
    assert!(!rendered.contains("Ctrl+B"), "busy bottom line must not flicker PTY hint, got: {rendered:?}");
    assert!(!rendered.contains("Alt+S"), "drawer discovery waits for idle, got: {rendered:?}");

    let header = session.header_suggestions_line().expect("header suggestions line");
    let header_text = line_text(&header);
    assert!(header_text.contains("Alt+S"), "header must keep drawer shortcut, got: {header_text:?}");
    assert_eq!(
        header_text.matches("Ctrl+B").count(),
        1,
        "background shortcut must not duplicate, got: {header_text:?}"
    );
}

#[test]
fn empty_input_status_shows_subagent_shortcuts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)];

    let line = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let rendered = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert!(rendered.contains("Alt+S"));
    assert!(rendered.contains("Ctrl+B"));
}

#[test]
fn turn_busy_hides_drawer_hint_until_idle_restores_it() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)];

    // Idle: drawer discovery visible.
    let idle = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let idle_text = line_text(&idle);
    assert!(idle_text.contains("Alt+S"), "idle must surface drawer discovery, got: {idle_text:?}");
    assert!(idle_text.contains("Ctrl+B"), "idle must surface background discovery, got: {idle_text:?}");

    // In-flight turn with no progress row and no foreground PTY: the bottom
    // line keeps the turn status while the drawer hint leaves, so the line
    // never reflows across tool gaps. The header keeps discovery.
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: edit_file".to_owned()),
        right: None,
    });
    assert!(session.is_running_activity(), "fixture must look like an active turn");
    let busy = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let busy_text = line_text(&busy);
    assert!(busy_text.contains("Running tool: edit_file"), "{busy_text:?}");
    assert!(!busy_text.contains("Alt+S"), "drawer discovery waits for idle, got: {busy_text:?}");
    assert!(!busy_text.contains("Ctrl+B"), "busy bottom line must not flicker hints, got: {busy_text:?}");
    let header = session.header_suggestions_line().expect("header suggestions line");
    let header_text = line_text(&header);
    assert!(header_text.contains("Alt+S"), "header must keep drawer shortcut, got: {header_text:?}");
    assert!(header_text.contains("Ctrl+B"), "header must keep background shortcut, got: {header_text:?}");

    // Turn ends: drawer discovery returns without a state toggle.
    session.handle_command(InlineCommand::SetInputStatus { left: None, right: None });
    assert!(!session.is_running_activity());
    let restored = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let restored_text = line_text(&restored);
    assert!(restored_text.contains("Alt+S"), "drawer discovery must return when idle, got: {restored_text:?}");
    assert!(
        restored_text.contains("Ctrl+B"),
        "background discovery must return when idle, got: {restored_text:?}"
    );
}

#[test]
fn empty_input_status_hides_subagent_shortcuts_without_agents() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();

    assert!(!rendered.contains("Alt+S"));
    assert!(!rendered.contains("Ctrl+B"));
}

#[test]
fn empty_input_status_hides_subagent_shortcuts_with_background_only() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(app_types::LocalAgentKind::Background)];

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();

    assert!(!rendered.contains("Alt+S"));
    assert!(!rendered.contains("Ctrl+B"));
}

#[test]
fn active_subagent_input_border_adds_extra_height() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.subagent_badges = vec![InlineHeaderBadge {
        text: "rust-engineer".to_string(),
        style: InlineTextStyle {
            color: Some(AnsiColorEnum::Rgb(RgbColor(0xFF, 0xFF, 0xFF))),
            bg_color: Some(AnsiColorEnum::Rgb(RgbColor(0x4F, 0x8F, 0xD8))),
            ..InlineTextStyle::default()
        },
        full_background: true,
    }];

    assert_eq!(session.input_block_extra_height(), 2);
}

#[test]
fn header_shows_active_subagent_badge_with_full_background() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.subagent_badges = vec![InlineHeaderBadge {
        text: "rust-engineer".to_string(),
        style: InlineTextStyle {
            color: Some(AnsiColorEnum::Rgb(RgbColor(0xFF, 0xFF, 0xFF))),
            bg_color: Some(AnsiColorEnum::Rgb(RgbColor(0x4F, 0x8F, 0xD8))),
            ..InlineTextStyle::default()
        },
        full_background: true,
    }];

    let line = session.header_meta_line();
    let badge_span = line
        .spans
        .iter()
        .find(|span| span.content.as_ref() == " rust-engineer ")
        .expect("subagent badge span");

    assert_eq!(badge_span.style.fg, Some(Color::Rgb(0xFF, 0xFF, 0xFF)));
    assert_eq!(badge_span.style.bg, Some(Color::Rgb(0x4F, 0x8F, 0xD8)));
    assert!(badge_span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn input_block_shows_active_subagent_title_with_badge_style() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("review current code".to_string());
    session.header_context.subagent_badges = vec![InlineHeaderBadge {
        text: "rust-engineer".to_string(),
        style: InlineTextStyle {
            color: Some(AnsiColorEnum::Rgb(RgbColor(0xFF, 0xFF, 0xFF))),
            bg_color: Some(AnsiColorEnum::Rgb(RgbColor(0x4F, 0x8F, 0xD8))),
            ..InlineTextStyle::default()
        },
        full_background: true,
    }];

    let title = session.active_subagent_input_title().expect("active subagent input title");
    let span = title.spans.first().expect("title span");
    assert_eq!(span.content.as_ref(), " rust-engineer ");
    assert_eq!(span.style.fg, Some(Color::Rgb(0xFF, 0xFF, 0xFF)));
    assert_eq!(span.style.bg, Some(Color::Rgb(0x4F, 0x8F, 0xD8)));
    assert!(span.style.add_modifier.contains(Modifier::BOLD));

    assert_eq!(session.input_block_extra_height(), 2);
}

#[test]
fn background_activity_drives_global_shimmer_without_locking_guards() {
    use crate::tui::core_tui::runner::TuiSessionDriver;

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Background)],
    });

    // Drawer stays closed, yet the loading signal must still reach the core.
    assert!(!session.local_agents_visible());
    assert!(session.core.has_background_activity());
    assert_eq!(session.core.background_activity_status_text().as_deref(), Some("Running 1 background task..."));

    // Global loading reflects background work, but the turn-busy guard stays off.
    assert!(TuiSessionDriver::has_status_spinner(&session));
    assert!(!TuiSessionDriver::is_running_activity(&session));
}

#[test]
fn background_activity_status_pluralizes_and_clears_with_entries() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("agent-1", "rust-engineer", app_types::LocalAgentKind::Background),
            sample_local_agent_entry_with_id("exec-1", "cargo check", app_types::LocalAgentKind::ExecSession),
        ],
    });
    assert_eq!(session.core.background_activity_status_text().as_deref(), Some("Running 2 background tasks..."));

    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![] });
    assert!(!session.core.has_background_activity());
    assert!(session.core.background_activity_status_text().is_none());
}

#[test]
fn input_status_surfaces_background_activity_while_turn_is_idle() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("agent-1", "rust-engineer", app_types::LocalAgentKind::Background),
            sample_local_agent_entry_with_id("exec-1", "cargo check", app_types::LocalAgentKind::ExecSession),
        ],
    });

    let line = session.core.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let rendered = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();

    assert!(
        rendered.contains("Running 2 background tasks"),
        "input status must surface background activity with the drawer closed, got: {rendered:?}"
    );
}

#[test]
fn input_status_omits_background_activity_when_none_running() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![] });

    let rendered = session
        .core
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();

    assert!(
        !rendered.contains("background task"),
        "idle session must not claim background activity, got: {rendered:?}"
    );
}

#[test]
fn exited_exec_entries_do_not_drive_shimmer_while_running_ones_do() {
    use crate::tui::core_tui::runner::TuiSessionDriver;

    let mut exited =
        sample_local_agent_entry_with_id("exec-exited", "cargo check", app_types::LocalAgentKind::ExecSession);
    exited.status = "exited (0)".to_string();
    let running =
        sample_local_agent_entry_with_id("exec-running", "cargo test", app_types::LocalAgentKind::ExecSession);

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![exited.clone(), running] });

    // Only the live session counts: shimmer on, turn-busy guard off.
    assert!(session.core.has_background_activity());
    assert_eq!(session.core.background_activity_status_text().as_deref(), Some("Running 1 background task..."));
    assert!(TuiSessionDriver::has_status_spinner(&session));
    assert!(!TuiSessionDriver::is_running_activity(&session));

    let rendered = session.core.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let text = rendered.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
    assert!(text.contains("Running 1 background task"), "live exec must shimmer, got: {text:?}");

    // Once everything settles to `exited`, retained history switches the
    // indicator to a finished summary and must stop the loading shimmer.
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![exited] });
    assert!(!session.core.has_background_activity());
    assert_eq!(session.core.background_activity_status_text().as_deref(), Some("1 agent finished"));
    assert!(!TuiSessionDriver::has_status_spinner(&session));

    let rendered = session.core.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let text = rendered.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
    assert!(!text.contains("background task"), "exited exec must not shimmer, got: {text:?}");
    assert!(text.contains("1 agent finished"), "finished summary should show, got: {text:?}");
    assert!(text.contains("local agents"), "retained exec must keep the drawer hint, got: {text:?}");
}

#[test]
fn header_suggestions_do_not_show_memory_shortcut_when_enabled() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.persistent_memory = Some(InlineHeaderStatusBadge {
        text: "Memory: auto".to_string(),
        tone: InlineHeaderStatusTone::Ready,
    });

    let line = session.header_suggestions_line().expect("header suggestions line");
    let summary = line_text(&line);

    assert!(!summary.contains("/memory"));
}

fn load_primary_agent_palette(session: &mut AppSession) {
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::AgentPalette(app_types::AgentPaletteTransientRequest {
            agents: vec![
                app_types::AgentPaletteItem { name: "beta".to_string(), description: None },
                app_types::AgentPaletteItem { name: "alpha".to_string(), description: None },
            ],
            visible: None,
        })),
    });
    session.close_transient();
}

#[test]
fn foreground_pty_hint_lives_in_header_not_footer_while_running() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    let line = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let rendered = line_text(&line);
    assert!(!rendered.contains("Ctrl+B"), "busy bottom line must not flicker PTY hint, got: {rendered:?}");
    assert!(!rendered.contains("background"), "busy bottom line keeps only stable context, got: {rendered:?}");

    let header = session.header_suggestions_line().expect("header suggestions line");
    let header_text = line_text(&header);
    assert!(header_text.contains("Ctrl+B"), "header must show shortcut, got: {header_text:?}");
    assert!(header_text.contains("background"), "header must explain background, got: {header_text:?}");

    let key_span = header
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "Ctrl+B")
        .expect("shortcut must be its own styled span");
    assert!(
        key_span.style.add_modifier.contains(Modifier::BOLD),
        "shortcut key must be bold as a visual indicator"
    );
}

#[test]
fn foreground_pty_hint_follows_rebound_background_shortcut() {
    use crate::tui::core_tui::session::action::BindingStore;

    let mut overlay = hashbrown::HashMap::new();
    overlay.insert("background_operation".to_owned(), vec!["ctrl+x".to_owned()]);
    let bindings = BindingStore::new(overlay);

    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_bindings(bindings);
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    // Busy bottom line stays clean under a rebound shortcut; the header
    // carries the rebound label.
    let status = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let rendered = line_text(&status);
    assert!(!rendered.contains("Ctrl+X"), "busy bottom line must not flicker PTY hint, got: {rendered:?}");
    assert!(!rendered.contains("Ctrl+B"), "footer must not keep stale shortcut, got: {rendered:?}");

    let header = session.header_suggestions_line().expect("header suggestions line");
    let header_text = line_text(&header);
    assert!(header_text.contains("Ctrl+X"), "header must use rebound shortcut, got: {header_text:?}");
}

#[test]
fn combined_drawer_and_pty_hint_styles_both_shortcuts_once_in_header() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)];
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));

    // Busy bottom line stays clean; the header keeps each shortcut once,
    // styled as its own bold span.
    let line = session.render_input_status_line(VIEW_WIDTH).expect("input status line");
    let rendered = line_text(&line);
    assert!(!rendered.contains("Alt+S"), "drawer discovery waits for idle, got: {rendered:?}");
    assert!(!rendered.contains("Ctrl+B"), "busy bottom line must not flicker PTY hint, got: {rendered:?}");

    let header = session.header_suggestions_line().expect("header suggestions line");
    let header_text = line_text(&header);
    assert!(header_text.contains("Alt+S"), "header must keep drawer shortcut, got: {header_text:?}");
    assert!(header_text.contains("Ctrl+B"), "header must keep background shortcut, got: {header_text:?}");
    assert_eq!(
        header_text.matches("Ctrl+B").count(),
        1,
        "background shortcut must not duplicate, got: {header_text:?}"
    );

    for key in ["Alt+S", "Ctrl+B"] {
        let span = header
            .spans
            .iter()
            .find(|span| span.content.as_ref() == key)
            .unwrap_or_else(|| panic!("{key} must be its own styled span, got: {header_text:?}"));
        assert!(span.style.add_modifier.contains(Modifier::BOLD), "{key} must be bold as a visual indicator");
    }
}

#[test]
fn live_and_finished_counts_split_loading_from_history() {
    let mut running = sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated);
    running.status = "running".to_string();
    let mut finished = sample_local_agent_entry_with_id("a2", "done-agent", app_types::LocalAgentKind::Delegated);
    finished.status = "completed".to_string();
    let mut failed_bg = sample_local_agent_entry_with_id("b1", "failed-bg", app_types::LocalAgentKind::Background);
    failed_bg.status = "error".to_string();

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![running, finished, failed_bg] });

    // Live work drives the running indicator; finished rows stay out of it.
    assert_eq!(session.core.background_activity_status_text().as_deref(), Some("Running 1 background task..."));
    let lines = rendered_app_session_lines(&mut session, 30);
    assert!(
        lines.iter().any(|line| line.contains("1 running · 2 finished")),
        "window header should split live vs finished counts, got: {lines:?}"
    );
}

#[test]
fn finished_summary_applies_when_no_live_work_remains() {
    let mut finished = sample_local_agent_entry_with_id("a1", "done-agent", app_types::LocalAgentKind::Delegated);
    finished.status = "completed".to_string();

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![finished.clone(), finished] });

    assert!(!session.core.has_background_activity());
    assert_eq!(session.core.background_activity_status_text().as_deref(), Some("2 agents finished"));
}

#[test]
fn ctrl_e_toggles_compact_and_expanded_panel() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated),
            sample_local_agent_entry_with_id("a2", "done-agent", app_types::LocalAgentKind::Delegated),
        ],
    });
    assert!(session.local_agents_visible());

    let compact_lines = rendered_app_session_lines(&mut session, 30);
    let compact_height = session.core.bottom_panel_area().map(|area| area.height).unwrap_or(0);
    assert!(compact_height > 0, "compact dock must claim panel height");
    assert!(
        compact_lines.iter().any(|line| line.contains("Ctrl+E expand")),
        "compact hint must offer expand, got: {compact_lines:?}"
    );

    let toggle = session.process_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    assert!(toggle.is_none());
    assert!(session.local_agents_visible());

    let expanded_lines = rendered_app_session_lines(&mut session, 30);
    let expanded_height = session.core.bottom_panel_area().map(|area| area.height).unwrap_or(0);
    assert!(expanded_height > compact_height, "expanded {expanded_height} must exceed compact {compact_height}");
    assert!(
        expanded_lines.iter().any(|line| line.contains("expanded")),
        "expanded title must show, got: {expanded_lines:?}"
    );
    assert!(
        expanded_lines.iter().any(|line| line.contains("Ctrl+E collapse")),
        "expanded hint must offer collapse, got: {expanded_lines:?}"
    );
    // Input stays above the panel in both modes; transcript is not covered.
    assert!(session.core.input_area().is_some());

    let collapse = session.process_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    assert!(collapse.is_none());
    let _ = rendered_app_session_lines(&mut session, 30);
    let collapsed_height = session.core.bottom_panel_area().map(|area| area.height).unwrap_or(0);
    assert_eq!(collapsed_height, compact_height, "second Ctrl+E must restore compact height");
}

#[test]
fn expanded_panel_stays_within_three_quarters_and_keeps_transcript() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));

    let rows: u16 = 30;
    let lines = rendered_app_session_lines(&mut session, rows);
    let panel_height = session.core.bottom_panel_area().map(|area| area.height).unwrap_or(0);
    // 75% of the 30-row viewport is 22 rows; docked panel must not exceed it
    // (it is clamped to the smaller input-aware max).
    assert!(panel_height <= 22, "expanded panel {panel_height} must stay within 75% of {rows}");
    assert!(panel_height > 6, "expanded panel must grow beyond compact height");
    assert!(session.core.input_area().is_some());
    assert!(lines.iter().any(|line| line.contains("Background")), "expanded dock must render, got: {lines:?}");
}

#[test]
fn header_click_toggles_expanded_without_closing_panel() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use tokio::sync::mpsc::unbounded_channel;

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated),
            sample_local_agent_entry_with_id("a2", "done-agent", app_types::LocalAgentKind::Delegated),
        ],
    });
    let _ = rendered_app_session_lines(&mut session, 30);
    assert!(!session.local_agents_is_expanded());
    let compact_height = session.core.bottom_panel_area().map(|area| area.height).unwrap_or(0);

    let panel = session.core.bottom_panel_area().expect("docked panel must render");
    let (tx, _rx) = unbounded_channel();
    let header_click = CrosstermEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: panel.x.saturating_add(2),
        row: panel.y,
        modifiers: KeyModifiers::NONE,
    });

    session.handle_event(header_click, &tx, None);
    assert!(session.local_agents_visible(), "header click must not close the panel");
    assert!(session.local_agents_is_expanded(), "header click must expand");
    let _ = rendered_app_session_lines(&mut session, 30);
    let expanded_height = session.core.bottom_panel_area().map(|area| area.height).unwrap_or(0);
    assert!(expanded_height > compact_height);

    // Second header click collapses; the panel stays open.
    let panel = session.core.bottom_panel_area().expect("expanded panel must render");
    let collapse_click = CrosstermEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: panel.x.saturating_add(2),
        row: panel.y,
        modifiers: KeyModifiers::NONE,
    });
    session.handle_event(collapse_click, &tx, None);
    assert!(session.local_agents_visible());
    assert!(!session.local_agents_is_expanded(), "second header click must collapse");

    // Clicks outside the panel must neither toggle nor close it.
    let _ = rendered_app_session_lines(&mut session, 30);
    let outside_click = CrosstermEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    session.handle_event(outside_click, &tx, None);
    assert!(session.local_agents_visible(), "outside click must not close the panel");
    assert!(!session.local_agents_is_expanded(), "outside click must not toggle expand");
}

#[test]
fn background_indicator_hit_targets_status_text() {
    let mut running = sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated);
    running.status = "running".to_string();

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![running] });
    let _ = rendered_app_session_lines(&mut session, 20);

    let hits = session.core.background_indicator_hits();
    assert!(!hits.is_empty(), "running background status should be clickable");
    let hit = hits[0];
    assert!(hit.height >= 1);
    assert!(hit.width > 0);
    assert!(session.core.background_indicator_contains(hit.x, hit.y));
    let outside = hit.x.saturating_add(hit.width).saturating_add(2);
    if outside < VIEW_WIDTH {
        assert!(
            !session.core.background_indicator_contains(outside, hit.y),
            "columns past the indicator must not open the window"
        );
    }
    assert!(!session.core.background_indicator_contains(hit.x, hit.y.saturating_sub(1)));
}

#[test]
fn combined_hint_hit_covers_key_background_not_local_agents_label() {
    let mut running = sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated);
    running.status = "running".to_string();

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![running] });
    let lines = rendered_app_session_lines(&mut session, 20);
    assert!(
        lines.iter().any(|line| line.contains("Alt+S") && line.contains("background")),
        "combined hint expected, got: {lines:?}"
    );

    let hits = session.core.background_indicator_hits();
    assert!(hits.len() >= 2, "expected activity + key-background hits, got {hits:?}");
    let key_hit = hits.last().copied().expect("key background hit");
    let activity_hit = hits[0];
    assert!(
        key_hit.x >= activity_hit.x + activity_hit.width,
        "key hit must be disjoint from the activity span: {hits:?}"
    );
    // Positive: both the key label and the ` background` tail are clickable.
    assert!(
        session.core.background_indicator_contains(key_hit.x, key_hit.y),
        "key label start must be clickable"
    );
    assert!(
        session
            .core
            .background_indicator_contains(key_hit.x + key_hit.width.saturating_sub(1), key_hit.y),
        "background tail must be clickable"
    );
    // Negative: `Alt+S local agents` sits well left of `{key}` (the combined
    // hint is `↓ or Alt+S local agents · {key} background`).
    let alt_s_probe = key_hit.x.saturating_sub(6);
    assert!(
        !session.core.background_indicator_contains(alt_s_probe, key_hit.y),
        "Alt+S local agents label must not be clickable (probe {alt_s_probe})"
    );
}

#[test]
fn click_on_background_indicator_toggles_window() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use tokio::sync::mpsc::unbounded_channel;

    let mut running = sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated);
    running.status = "running".to_string();

    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![running] });
    let _ = rendered_app_session_lines(&mut session, 20);
    session.close_transient();
    assert!(!session.local_agents_visible());
    let _ = rendered_app_session_lines(&mut session, 20);

    let hit = session
        .core
        .background_indicator_hits()
        .first()
        .copied()
        .expect("indicator hit");
    let (tx, _rx) = unbounded_channel();
    let click = CrosstermEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::NONE,
    });

    session.handle_event(click.clone(), &tx, None);
    assert!(session.local_agents_visible(), "click should open the expanded window");

    // Re-paint so the indicator hit rect is current while the window is open.
    let _ = rendered_app_session_lines(&mut session, 20);
    let hit = session
        .core
        .background_indicator_hits()
        .first()
        .copied()
        .expect("indicator hit while open");
    let click = CrosstermEvent::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::NONE,
    });
    session.handle_event(click, &tx, None);
    assert!(!session.local_agents_visible(), "second click should close the window");
}
