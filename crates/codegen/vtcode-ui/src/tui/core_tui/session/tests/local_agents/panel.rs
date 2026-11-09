use super::*;

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
    running.status = crate::tui::core_tui::types::LocalAgentStatus::Running;
    let mut finished = sample_local_agent_entry_with_id("a2", "done-agent", app_types::LocalAgentKind::Delegated);
    finished.status = crate::tui::core_tui::types::LocalAgentStatus::Completed;
    let mut failed_bg = sample_local_agent_entry_with_id("b1", "failed-bg", app_types::LocalAgentKind::Background);
    failed_bg.status = crate::tui::core_tui::types::LocalAgentStatus::Error;

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
    finished.status = crate::tui::core_tui::types::LocalAgentStatus::Completed;

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
fn background_shortcut_toggle_closes_panel_but_keeps_state() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![
            sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated),
            sample_local_agent_entry_with_id("a2", "done-agent", app_types::LocalAgentKind::Delegated),
        ],
    });
    // Select the second row and expand so state retention is observable.
    assert!(session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)).is_none());
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL))
            .is_none()
    );
    let _ = rendered_app_session_lines(&mut session, 30);
    assert!(session.local_agents_is_expanded());

    let toggle = session.process_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(toggle.is_none(), "toggle-close must not emit a runloop event");
    assert!(!session.local_agents_visible());

    // Reopening restores the same selection and expanded state: the close was
    // temporary, nothing was discarded.
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::LocalAgents(app_types::LocalAgentsTransientRequest {
            visible: Some(true),
        })),
    });
    assert!(session.local_agents_visible());
    assert!(session.local_agents_is_expanded(), "expanded state must survive a toggle-close");
    let inspect = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        inspect,
        Some(app_types::InlineEvent::Submit(value)) if value == "/agent inspect a2"
    ));
}
#[test]
fn background_shortcut_keeps_foreground_priority_while_panel_open() {
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });
    assert!(session.local_agents_visible());
    session.core.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));
    let _ = rendered_app_session_lines(&mut session, 30);

    let toggle = session.process_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(
        matches!(toggle, Some(app_types::InlineEvent::BackgroundOperation)),
        "live foreground command must still reach the runloop, got: {toggle:?}"
    );
    assert!(session.local_agents_visible(), "panel must stay open when foreground keeps priority");
}
#[test]
fn background_shortcut_rebound_still_toggles_panel() {
    let mut overlay = hashbrown::HashMap::new();
    overlay.insert("background_operation".to_owned(), vec!["ctrl+x".to_owned()]);
    let mut session = app_session_with_input("", 0);
    session.handle_command(app_types::InlineCommand::SetKeyBindings { bindings: overlay });
    session.handle_command(app_types::InlineCommand::SetLocalAgents {
        entries: vec![sample_local_agent_entry(app_types::LocalAgentKind::Delegated)],
    });
    assert!(session.local_agents_visible());

    // Rebound key toggles the panel closed with no runloop event.
    let toggle = session.process_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
    assert!(toggle.is_none());
    assert!(!session.local_agents_visible());

    // The stale default no longer resolves, so it must not toggle anything.
    session.handle_command(app_types::InlineCommand::ShowTransient {
        request: Box::new(app_types::TransientRequest::LocalAgents(app_types::LocalAgentsTransientRequest {
            visible: Some(true),
        })),
    });
    assert!(session.local_agents_visible());
    let stale = session.process_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert!(stale.is_none());
    assert!(session.local_agents_visible(), "unbound Ctrl+B must leave the panel alone");
}
#[test]
fn background_indicator_hit_targets_status_text() {
    let mut running = sample_local_agent_entry_with_id("a1", "running-agent", app_types::LocalAgentKind::Delegated);
    running.status = crate::tui::core_tui::types::LocalAgentStatus::Running;

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
    running.status = crate::tui::core_tui::types::LocalAgentStatus::Running;

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
    running.status = crate::tui::core_tui::types::LocalAgentStatus::Running;

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
