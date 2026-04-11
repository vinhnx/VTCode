use super::helpers::*;
use super::super::*;

#[test]
fn down_opens_local_agents_drawer_when_input_is_empty() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(
            app_types::LocalAgentKind::Delegated,
        )],
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
        entries: vec![sample_local_agent_entry(
            app_types::LocalAgentKind::Delegated,
        )],
    });

    assert!(session.local_agents_visible());
}

#[test]
fn local_agents_drawer_hides_input_while_open() {
    let mut session = app_session_with_input("draft command", "draft command".len());

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(
            app_types::LocalAgentKind::Delegated,
        )],
    });

    assert!(session.local_agents_visible());
    assert!(!session.core.input_enabled());
    assert!(
        !session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );

    let lines = rendered_app_session_lines(&mut session, 20);
    assert!(session.core.input_area().is_none());
    assert!(session.core.bottom_panel_area().is_some());
    assert!(
        lines.iter().any(|line| line.contains("Local Agents")),
        "drawer should still render"
    );
    assert!(
        !lines.iter().any(|line| line.contains("draft command")),
        "hidden composer should not render draft text"
    );
}

#[test]
fn closing_local_agents_drawer_restores_input_and_draft() {
    let mut session = app_session_with_input("draft command", "draft command".len());

    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(
            app_types::LocalAgentKind::Delegated,
        )],
    });
    let _ = rendered_app_session_lines(&mut session, 20);

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(!session.local_agents_visible());
    assert!(session.core.input_enabled());
    assert!(
        session
            .core
            .build_input_widget_data(VIEW_WIDTH, 1)
            .cursor_should_be_visible
    );
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
            sample_local_agent_entry_with_id(
                "agent-1",
                "rust-engineer",
                app_types::LocalAgentKind::Delegated,
            ),
            sample_local_agent_entry_with_id(
                "agent-2",
                "qa-reviewer",
                app_types::LocalAgentKind::Delegated,
            ),
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
        entries: vec![sample_local_agent_entry(
            app_types::LocalAgentKind::Background,
        )],
    });

    assert!(!session.local_agents_visible());
}

#[test]
fn empty_local_agents_drawer_stays_open_after_last_entry_is_removed() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(
            app_types::LocalAgentKind::Delegated,
        )],
    });

    session.handle_command(app_types::InlineCommand::SetLocalAgents { entries: vec![] });

    assert!(session.local_agents_visible());

    let lines = rendered_app_session_lines(&mut session, 20);
    assert!(
        lines
            .iter()
            .any(|line| line.contains("No local agents yet")),
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
fn header_suggestions_include_subagent_shortcuts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(
        app_types::LocalAgentKind::Delegated,
    )];

    let line = session
        .header_suggestions_line()
        .expect("header suggestions line");
    let rendered = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("Alt+S"));
    assert!(rendered.contains("Ctrl+B"));
}

#[test]
fn header_suggestions_hide_subagent_shortcuts_without_agents() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let line = session
        .header_suggestions_line()
        .expect("header suggestions line");
    let rendered = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(!rendered.contains("Alt+S"));
    assert!(!rendered.contains("Ctrl+B"));
}

#[test]
fn header_suggestions_hide_subagent_shortcuts_with_background_only() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(
        app_types::LocalAgentKind::Background,
    )];

    let line = session
        .header_suggestions_line()
        .expect("header suggestions line");
    let rendered = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(!rendered.contains("Alt+S"));
    assert!(!rendered.contains("Ctrl+B"));
}

#[test]
fn empty_input_status_shows_subagent_shortcuts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(
        app_types::LocalAgentKind::Delegated,
    )];

    let line = session
        .render_input_status_line(VIEW_WIDTH)
        .expect("input status line");
    let rendered = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert!(rendered.contains("Alt+S"));
    assert!(rendered.contains("Ctrl+B"));
}

#[test]
fn empty_input_status_hides_subagent_shortcuts_without_agents() {
    let session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .unwrap_or_default();

    assert!(!rendered.contains("Alt+S"));
    assert!(!rendered.contains("Ctrl+B"));
}

#[test]
fn empty_input_status_hides_subagent_shortcuts_with_background_only() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.local_agents = vec![sample_local_agent_entry(
        app_types::LocalAgentKind::Background,
    )];

    let rendered = session
        .render_input_status_line(VIEW_WIDTH)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
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

    let title = session
        .active_subagent_input_title()
        .expect("active subagent input title");
    let span = title.spans.first().expect("title span");
    assert_eq!(span.content.as_ref(), " rust-engineer ");
    assert_eq!(span.style.fg, Some(Color::Rgb(0xFF, 0xFF, 0xFF)));
    assert_eq!(span.style.bg, Some(Color::Rgb(0x4F, 0x8F, 0xD8)));
    assert!(span.style.add_modifier.contains(Modifier::BOLD));

    assert_eq!(session.input_block_extra_height(), 2);
}

#[test]
fn header_suggestions_do_not_show_memory_shortcut_when_enabled() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.header_context.persistent_memory = Some(InlineHeaderStatusBadge {
        text: "Memory: auto".to_string(),
        tone: InlineHeaderStatusTone::Ready,
    });

    let line = session
        .header_suggestions_line()
        .expect("header suggestions line");
    let summary = line_text(&line);

    assert!(!summary.contains("/memory"));
}

