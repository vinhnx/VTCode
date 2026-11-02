use super::*;

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
