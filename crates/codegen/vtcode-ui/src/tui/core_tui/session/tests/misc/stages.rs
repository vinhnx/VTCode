use super::*;

#[test]
fn background_activity_animates_loading_shimmer_without_looking_busy() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.background_status_shimmer_active());

    session.set_background_activity_count(2);
    assert!(session.has_background_activity());
    assert!(session.background_status_shimmer_active(), "live background work must animate the loading shimmer");
    assert!(!session.is_running_activity(), "background work must never register as an in-flight turn");
    assert_eq!(session.background_activity_status_text().as_deref(), Some("Running 2 background tasks..."));

    session.set_background_activity_count(0);
    assert!(!session.background_status_shimmer_active());
    assert!(session.background_activity_status_text().is_none());
}
#[test]
fn progress_ticks_follow_reduce_motion_override_and_screen_reader_policy() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.thinking_spinner.start();
    session.set_background_activity_count(1);

    let initial_spinner = session.thinking_spinner.current_frame();
    let initial_phase = session.shimmer_state.phase();
    std::thread::sleep(Duration::from_millis(100));
    session.handle_tick();

    assert_ne!(session.thinking_spinner.current_frame(), initial_spinner);
    assert_ne!(session.shimmer_state.phase(), initial_phase);

    session.appearance.reduce_motion_mode = true;
    let reduced_spinner = session.thinking_spinner.current_frame();
    let reduced_phase = session.shimmer_state.phase();
    std::thread::sleep(Duration::from_millis(100));
    session.handle_tick();

    assert_eq!(session.thinking_spinner.current_frame(), reduced_spinner);
    assert_eq!(session.shimmer_state.phase(), reduced_phase);

    session.appearance.reduce_motion_keep_progress_animation = true;
    session.handle_tick();

    assert_ne!(session.thinking_spinner.current_frame(), reduced_spinner);
    assert_ne!(session.shimmer_state.phase(), reduced_phase);

    session.appearance.reduce_motion_mode = false;
    session.appearance.screen_reader_mode = true;
    let screen_reader_spinner = session.thinking_spinner.current_frame();
    let screen_reader_phase = session.shimmer_state.phase();
    std::thread::sleep(Duration::from_millis(100));
    session.handle_tick();

    assert_eq!(session.thinking_spinner.current_frame(), screen_reader_spinner);
    assert_eq!(session.shimmer_state.phase(), screen_reader_phase);
}
#[test]
fn animation_tick_needed_only_for_active_or_pending_work() {
    use crate::tui::core_tui::session::mouse_selection::{DragAutoScroll, DragAutoScrollDirection};

    // A fresh idle session rests — input, commands (new messages), and
    // crossterm events already render immediately, so no periodic tick is needed.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.needs_animation_tick(), "fresh idle session must not need animation ticks");

    // Thinking spinner needs active-rate ticks for its 80ms frames.
    session.thinking_spinner.start();
    assert!(session.needs_animation_tick(), "active spinner must need ticks");
    session.thinking_spinner.stop();
    assert!(!session.needs_animation_tick(), "stopped spinner must rest");

    // Shimmer status needs ticks for its 33ms frames; clearing it needs one
    // final tick to repaint, then rests.
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });
    assert!(session.needs_animation_tick(), "shimmer status must need ticks");
    session.handle_command(InlineCommand::SetInputStatus { left: None, right: None });
    session.handle_tick();
    assert!(!session.needs_animation_tick(), "cleared shimmer must rest after final tick");

    // Background work animates the shimmer without marking a turn busy.
    session.set_background_activity_count(1);
    assert!(session.needs_animation_tick(), "background work must need ticks");
    session.set_background_activity_count(0);
    assert!(!session.needs_animation_tick(), "finished background work must rest");

    // Copy notification expiry needs a tick to clear promptly.
    session.show_copy_notification(5);
    assert!(session.needs_animation_tick(), "copy notification must need ticks");
    session.copy_notification_until = Some(Instant::now().checked_sub(Duration::from_secs(1)).unwrap());
    session.handle_tick();
    assert!(!session.needs_animation_tick(), "expired copy notification must rest");

    // Drag auto-scroll needs ticks to step while the pointer rests at an edge.
    session.drag_auto_scroll = Some(DragAutoScroll {
        direction: DragAutoScrollDirection::Down,
        column: 0,
        row: 0,
        last_step: Instant::now(),
    });
    assert!(session.needs_animation_tick(), "armed drag auto-scroll must need ticks");
    session.cancel_drag_auto_scroll();
    assert!(!session.needs_animation_tick(), "released drag must rest");

    // Reduce-motion suppresses spinner/shimmer frames but not expiry cleanup.
    session.thinking_spinner.start();
    session.appearance.reduce_motion_mode = true;
    assert!(!session.needs_animation_tick(), "reduce-motion must suppress spinner ticks");
    session.show_copy_notification(5);
    assert!(session.needs_animation_tick(), "expiries still need ticks under reduce-motion");
}
#[test]
fn active_pty_observer_overrides_idle_stage_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let active_pty_sessions = Arc::new(AtomicUsize::new(1));
    session.active_pty_sessions = Some(Arc::clone(&active_pty_sessions));
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Building));

    assert_eq!(session.status_left_text(), Some("Running PTY command..."));
    assert!(session.has_status_spinner());

    active_pty_sessions.store(0, Ordering::Relaxed);

    assert_eq!(session.status_left_text(), Some("Building..."));
    assert!(!session.has_status_spinner());
}
#[test]
fn task_panel_visibility_is_independent_from_logs() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_task_panel_visible(true);
    let initial_task_panel = session.show_task_panel;
    let initial_logs = session.core.show_logs;

    session.core.toggle_logs();

    assert_eq!(session.show_task_panel, initial_task_panel);
    assert_ne!(session.core.show_logs, initial_logs);
}
#[test]
fn stage_states_keep_input_enabled() {
    for state in [ActivityState::Planning, ActivityState::Building] {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        session.handle_command(InlineCommand::SetActivityState(state));

        assert!(session.input_enabled(), "{state:?} should keep input enabled");
        assert_eq!(session.status_left_text(), state.status());
        assert!(!session.is_running_activity(), "{state:?} with no tool should not spin");
    }
}
#[test]
fn git_status_is_not_rendered_in_footer() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("git: main*".to_string()),
        right: Some("10:30".to_string()),
    });

    let area = Rect::new(0, 0, VIEW_WIDTH, 1);
    let mut buffer = Buffer::empty(area);
    SessionWidget::new(&mut session)
        .layout_mode(LayoutMode::Compact)
        .render(area, &mut buffer);

    let rendered = buffer.content().iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(!rendered.contains("main*"), "git status should not appear in footer: {rendered}");
}
#[test]
fn stage_state_with_running_tool_shows_tool_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Planning));
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: edit_file".to_string()),
        right: None,
    });

    // The live tool status must replace the stage label so the footer
    // reflects what the agent is actually doing, not a frozen "Planning...".
    assert_eq!(session.status_left_text(), Some("Running tool: edit_file"));
    assert!(session.has_status_spinner(), "stage label must animate while a tool runs");
    assert!(session.is_running_activity());
    assert!(session.input_enabled(), "stage states keep input usable mid-tool");
}
#[test]
fn planning_stage_surfaces_live_drafting_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Planning));
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Drafting plan... (42 chars)".to_string()),
        right: None,
    });

    // The live synthesis status must replace the stage label so the footer
    // shows the drafting progress (and shimmers) instead of a frozen
    // "Planning...". Input stays usable: Planning is a non-busy stage.
    assert_eq!(session.status_left_text(), Some("Drafting plan... (42 chars)"));
    assert!(session.has_status_spinner(), "drafting status must animate");
    assert!(session.is_running_activity());
    assert!(session.input_enabled(), "planning keeps input usable mid-draft");
}
#[test]
fn stage_state_idle_shows_stage_label() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Planning));
    // No active tool — the status refresh may set a git-branch left text that
    // does not indicate active work. The stage label must still be shown.
    session.handle_command(InlineCommand::SetInputStatus { left: Some("main*".to_string()), right: None });

    assert_eq!(session.status_left_text(), Some("Planning..."));
    assert!(!session.has_status_spinner(), "idle stage must not spin");
    assert!(!session.is_running_activity());
}
#[test]
fn busy_state_disables_input_and_shows_busy_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetActivityState(ActivityState::StartingBuild));

    assert!(!session.input_enabled(), "busy states must block input");
    assert!(session.is_running_activity());
    assert!(session.has_status_spinner());
}
#[test]
fn blocked_state_persists_status_for_direct_left_status_readers() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Blocked));

    assert!(session.input_enabled(), "blocked state must keep input enabled");
    assert!(!session.is_running_activity(), "blocked state must not look like an active turn");
    assert!(!session.has_status_spinner(), "blocked state must not animate as active work");
    let left = session
        .input_status_left
        .as_deref()
        .expect("blocked state must persist its status for direct left-status readers");
    assert_eq!(left, ActivityState::Blocked.status().expect("blocked state has a status"));
    assert_eq!(session.status_left_text(), Some(left));
}
#[test]
fn idle_state_clears_left_status() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Blocked));
    session.handle_command(InlineCommand::SetActivityState(ActivityState::Idle));

    assert_eq!(session.input_status_left, None, "idle must clear the persisted blocked status");
}
#[test]
fn hidden_terminal_suppresses_animation_ticks_and_resumes_on_visible() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.is_terminal_visible(), "fresh session must default to visible (fail-visible)");

    session.thinking_spinner.start();
    assert!(session.needs_animation_tick(), "active spinner must need ticks while visible");

    session.apply_terminal_visibility_report(false);
    assert!(!session.is_terminal_visible());
    assert!(!session.needs_animation_tick(), "hidden view must suppress animation ticks");

    session.apply_terminal_visibility_report(true);
    assert!(session.is_terminal_visible());
    assert!(session.needs_animation_tick(), "returning visible must resume animation ticks");
}
#[test]
fn duplicate_visibility_reports_are_noops() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let _ = session.take_redraw();

    session.apply_terminal_visibility_report(true);
    assert!(!session.take_redraw(), "duplicate visible report must not mark dirty");

    session.apply_terminal_visibility_report(false);
    assert!(!session.take_redraw(), "hiding must not mark dirty");

    session.apply_terminal_visibility_report(false);
    assert!(!session.take_redraw(), "duplicate hidden report must not mark dirty");

    session.apply_terminal_visibility_report(true);
    assert!(session.take_redraw(), "hidden-to-visible transition must force a repaint");
}
#[test]
#[cfg(vendored_crossterm)]
fn visibility_report_event_updates_terminal_visibility() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let (sender, _receiver) = mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::VisibilityReport { visible: false }, &sender, None);
    assert!(!session.is_terminal_visible(), "hidden report must hide the session");

    session.handle_event(CrosstermEvent::VisibilityReport { visible: true }, &sender, None);
    assert!(session.is_terminal_visible(), "visible report must show the session again");
}
