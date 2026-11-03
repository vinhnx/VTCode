use super::*;

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
    exited.status = crate::tui::core_tui::types::LocalAgentStatus::Exited { code: Some(0) };
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
