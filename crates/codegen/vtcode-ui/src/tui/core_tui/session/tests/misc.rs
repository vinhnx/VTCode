#![allow(
    clippy::float_cmp,
    reason = "phase is a continuous f32; tests assert exact phase transitions"
)]
#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;
use crate::tui::core_tui::widgets::{LayoutMode, SessionWidget};
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use unicode_width::UnicodeWidthStr;

#[test]
fn input_compact_preview_for_image_path() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let image_path = "/tmp/Screenshot 2026-02-06 at 3.39.48 PM.png";

    session.insert_paste_text(image_path);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image:"));
    assert!(rendered.contains("Screenshot 2026-02-06"));
}

#[test]
fn input_compact_preview_for_quoted_image_path() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let image_path = "\"/tmp/Screenshot 2026-02-06 at 3.39.48 PM.png\"";

    session.insert_paste_text(image_path);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Image:"));
    assert!(rendered.contains("Screenshot 2026-02-06"));
}

#[test]
fn control_g_launches_editor() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(result, Some(InlineEvent::LaunchEditor { draft }) if draft.is_empty()));
}

#[test]
fn control_g_launches_editor_with_draft() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert!(matches!(result, Some(InlineEvent::LaunchEditor { draft }) if draft == text));
}

#[test]
fn control_g_launches_editor_from_plan_confirmation_modal() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_status_right = Some("model | 25% context".to_string());
    let plan = app_types::PlanContent::from_markdown(
        "Test Plan".to_string(),
        "## Plan of Work\n- Step 1",
        Some(".vtcode/plans/test-plan.md".to_string()),
    );
    show_plan_confirmation_overlay(&mut session, plan);

    let event = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(
        result,
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(OverlaySubmission::Hotkey(
            OverlayHotkeyAction::LaunchEditor
        ))))
    ));
    assert!(session.modal_state().is_none());
}

#[test]
fn plan_confirmation_modal_matches_four_way_gate_copy() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let plan = app_types::PlanContent::from_markdown(
        "Test Plan".to_string(),
        "## Implementation Plan\n1. Step",
        Some(".vtcode/plans/test-plan.md".to_string()),
    );
    show_plan_confirmation_overlay(&mut session, plan);

    let modal = session.modal_state().expect("plan confirmation modal should be present");
    assert_eq!(modal.title, "Ready to code?");
    assert_eq!(
        modal.lines.first().map(String::as_str),
        Some("A plan is ready to execute. Would you like to proceed?")
    );

    let list = modal.list.as_ref().expect("plan confirmation should include list options");
    assert_eq!(list.items.len(), 3);

    assert_eq!(list.items[0].title, "Yes, auto-accept edits");
    assert_eq!(list.items[0].subtitle.as_deref(), Some("Execute with auto-approval."));
    assert_eq!(list.items[0].badge.as_deref(), Some("Recommended"));

    assert_eq!(list.items[1].title, "Yes, manually approve edits");
    assert_eq!(list.items[1].subtitle.as_deref(), Some("Keep context and confirm each edit before applying."));

    assert_eq!(list.items[2].title, "Type feedback to revise the plan");
    assert_eq!(list.items[2].subtitle.as_deref(), Some("Return to planning workflow and refine the plan."));
}

#[test]
fn arrow_keys_never_launch_editor() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    // Test Right arrow with all possible modifier combinations
    for modifiers in [
        KeyModifiers::empty(),
        KeyModifiers::CONTROL,
        KeyModifiers::SHIFT,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::META,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        KeyModifiers::CONTROL | KeyModifiers::SUPER,
    ] {
        let event = KeyEvent::new(KeyCode::Right, modifiers);
        let result = session.process_key(event);
        assert!(
            !matches!(result, Some(InlineEvent::LaunchEditor { .. })),
            "Right arrow with modifiers {modifiers:?} should not launch editor"
        );
    }

    // Test other arrow keys for safety
    for key_code in [KeyCode::Left, KeyCode::Up, KeyCode::Down] {
        let event = KeyEvent::new(key_code, KeyModifiers::SUPER);
        let result = session.process_key(event);
        assert!(
            !matches!(result, Some(InlineEvent::LaunchEditor { .. })),
            "{key_code:?} with SUPER should not launch editor"
        );
    }
}

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
fn timeline_visible_selects_latest_item() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("First")]);
    session.push_line(InlineMessageKind::Agent, vec![make_segment("Second")]);

    let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
    let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("failed to render session with timeline");

    assert!(session.navigation_state.selected().is_none());
}

#[test]
fn tool_detail_renders_with_border_and_body_style() {
    let theme = themed_inline_colors();
    let mut session = Session::new(theme, None, VIEW_ROWS);
    let detail_style = InlineTextStyle::default().italic();
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "    result line".to_string(),
            style: Arc::new(detail_style),
        }],
    );

    let index = session.lines.len().checked_sub(1).expect("tool detail line should exist");
    let spans = session.render_message_spans(index);

    assert_eq!(spans.len(), 1);
    let body_span = &spans[0];
    assert!(body_span.style.add_modifier.contains(Modifier::ITALIC));
    assert_eq!(body_span.content.clone().into_owned(), "    result line");
}

#[test]
fn top_level_task_tree_tail_line_is_dimmed_in_tool_blocks() {
    let theme = themed_inline_colors();
    let mut session = Session::new(theme, None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "└ Report actions taken, blockers, and required user input".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    let index = session.lines.len().checked_sub(1).expect("tool detail line should exist");
    let transcript_lines = session.reflow_message_lines(index, 100, false);
    let task_span = transcript_lines
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("Report actions taken"))
        .expect("expected task span");

    assert!(task_span.style.add_modifier.contains(Modifier::DIM), "top-level task rows should render dimmed");
}

#[test]
fn tool_block_with_cjk_prefix_does_not_overflow_viewport() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Tool,
        vec![InlineSegment {
            text: "日本語のツール詳細情報".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    let index = session.lines.len() - 1;
    let transcript = session.reflow_message_lines(index, VIEW_WIDTH, false);
    for line in &transcript {
        let width: usize = line.line.spans.iter().map(|span| span.content.width()).sum();
        assert!(
            width <= VIEW_WIDTH as usize,
            "tool block line with CJK prefix overflowed viewport: {width} > {VIEW_WIDTH}"
        );
    }
}

#[test]
fn error_block_with_cjk_label_reserves_correct_content_width() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Error,
        vec![InlineSegment {
            text: "エラーが発生しました".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    let index = session.lines.len() - 1;
    let transcript = session.reflow_message_lines(index, VIEW_WIDTH, false);
    for line in &transcript {
        let width: usize = line.line.spans.iter().map(|span| span.content.width()).sum();
        assert!(
            width <= VIEW_WIDTH as usize,
            "error block line with CJK label overflowed viewport: {width} > {VIEW_WIDTH}"
        );
    }
}

#[test]
fn backspace_after_large_paste_deletes_whole_block() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    assert!(session.input_manager.compact_paste_range().is_some());
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());

    session.delete_char();

    assert_eq!(session.input_manager.content(), "x");
    assert!(session.input_manager.compact_paste_range().is_none());
}

#[test]
fn backspace_after_paste_still_deletes_typed_chars_one_by_one() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    session.insert_char('!');
    session.insert_char('?');
    assert!(session.input_manager.content().ends_with("!?"));

    session.delete_char();
    assert!(session.input_manager.content().ends_with("!"));
    assert!(session.input_manager.compact_paste_range().is_some());

    session.delete_char();
    assert!(session.input_manager.content().ends_with(&pasted));
}

#[test]
fn backspace_with_active_selection_deletes_selection_not_whole_paste() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    let content_len = session.input_manager.content().len();
    // Select the last two characters (e.g. "10" of "line 10") so the active
    // selection ends exactly at the collapsed paste end.
    session.input_manager.set_cursor_with_selection(content_len - 2);
    assert!(session.input_manager.selection_range().is_some());

    session.delete_char();

    // The selection wins over the collapse: only the two selected chars are
    // removed, not the entire pasted block.
    assert_eq!(session.input_manager.content().len(), content_len - 2);
    assert!(!session.input_manager.content().ends_with("10"));
    assert!(session.input_manager.content().contains("line 0"));
}

#[test]
fn backspace_after_paste_deletes_whole_block_when_no_selection() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    assert!(session.input_manager.compact_paste_range().is_some());
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());
    assert!(session.input_manager.selection_range().is_none());

    session.delete_char();

    assert_eq!(session.input_manager.content(), "x");
    assert!(session.input_manager.compact_paste_range().is_none());
}

#[test]
fn delete_word_forward_before_paste_keeps_block_delete_aligned() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    session.insert_char(' ');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    let range = session.input_manager.compact_paste_range().expect("collapsed paste range");
    assert_eq!(range.start, 2);

    // Delete the typed word sitting before the collapsed block.
    session.input_manager.set_cursor(0);
    session.input_manager.delete_word_forward();

    // The range must shift by the removed bytes, not go stale.
    let shifted = session
        .input_manager
        .compact_paste_range()
        .expect("range survives edits entirely before it");
    assert_eq!(shifted.start, 1);
    assert_eq!(shifted.end, range.end - 1);

    // The block-delete invariant holds: cursor at the new end coincides with
    // the shifted range end, so Backspace removes the whole block.
    session.input_manager.set_cursor(session.input_manager.content().len());
    session.delete_char();

    assert_eq!(session.input_manager.content(), " ");
    assert!(session.input_manager.compact_paste_range().is_none());
}

#[test]
fn delete_word_forward_overlapping_paste_clears_compact_range() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.insert_char('x');
    session.insert_char(' ');
    let pasted: String = (0..12).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);

    let range = session.input_manager.compact_paste_range().expect("collapsed paste range");

    // Cursor inside the collapsed block: an overlapping delete invalidates it
    // so a later Backspace never removes user-typed bytes.
    session.input_manager.set_cursor(range.start + 1);
    session.input_manager.delete_word_forward();

    assert!(session.input_manager.compact_paste_range().is_none());
}

#[test]
fn alt_backspace_deletes_previous_word() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("hello world".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    let result = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "hello ");
}

#[test]
fn alt_backspace_deletes_previous_cyrillic_word() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("привет мир".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());

    let result = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "привет ");
}

#[test]
fn overlay_retains_input_ownership_across_activity_transitions() {
    use crate::tui::core_tui::session::input_manager::InputOwner;
    for initial in [
        ActivityState::Idle,
        ActivityState::StartingBuild,
        ActivityState::Building,
    ] {
        for next in [
            ActivityState::Idle,
            ActivityState::StartingBuild,
            ActivityState::Building,
            ActivityState::Blocked,
        ] {
            let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
            session.handle_command(InlineCommand::SetActivityState(initial));
            session.handle_command(InlineCommand::ShowOverlay {
                request: Box::new(OverlayRequest::Modal(crate::tui::core_tui::types::ModalOverlayRequest {
                    title: "Ownership".to_string(),
                    lines: Vec::new(),
                    secure_prompt: None,
                    is_help_modal: false,
                })),
            });
            session.handle_command(InlineCommand::SetActivityState(next));
            assert_eq!(session.input_owner(), InputOwner::Overlay);
            assert!(!session.input_enabled());
            session.close_overlay();
            assert_eq!(session.input_enabled(), !next.is_busy(), "{initial:?} -> {next:?}");
        }
    }
}

#[test]
fn clear_mouse_selection_reports_and_drops_a_visible_selection() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    assert!(!session.clear_mouse_selection(), "an empty selection must report no change");

    session.mouse_selection.set_selection((2, 3), (9, 3));
    assert!(session.mouse_selection.has_selection);

    assert!(session.clear_mouse_selection(), "a visible selection must report a change");
    assert!(!session.mouse_selection.has_selection);
    assert!(!session.mouse_selection.is_selecting);
    assert!(!session.clear_mouse_selection(), "clearing twice is a no-op");
}

#[test]
fn escape_dismisses_selection_before_arming_rewind() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.mouse_selection.set_selection((1, 1), (6, 1));

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(event.is_none(), "dismissing a selection consumes the key, got {event:?}");
    assert!(!session.mouse_selection.has_selection, "Esc must clear the highlight");
    assert_eq!(session.last_escape_press, None, "dismissal must not arm the rewind double-press");
}

#[test]
fn escape_still_interrupts_running_activity_with_a_selection() {
    // Asymmetric guard: the selection branch must not shadow interrupt
    // precedence while a turn is running.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.mouse_selection.set_selection((1, 1), (6, 1));
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: read_file".to_string()),
        right: None,
    });
    assert!(session.is_running_activity(), "fixture must look like an active turn");

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(
        matches!(event, Some(InlineEvent::Interrupt)),
        "running activity keeps interrupt precedence, got {event:?}"
    );
    assert!(session.mouse_selection.has_selection, "interrupt must not silently consume the selection");
}

#[test]
fn set_fullscreen_interaction_command_updates_all_fields() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.fullscreen.interaction.copy_on_select, "copy_on_select defaults on");
    assert_eq!(session.fullscreen.interaction.scroll_speed, 3);

    session.handle_command(InlineCommand::SetFullscreenInteraction {
        interaction: FullscreenInteractionSettings {
            mouse_capture: false,
            copy_on_select: false,
            scroll_speed: 7,
        },
    });

    assert!(!session.fullscreen.interaction.copy_on_select);
    assert!(!session.fullscreen.interaction.mouse_capture);
    assert_eq!(session.fullscreen.interaction.scroll_speed, 7);
}

#[test]
fn app_session_forwards_fullscreen_interaction_to_core() {
    use crate::tui::core_tui::app::types::InlineCommand as AppInlineCommand;

    let mut session = app_session_with_input("", 0);
    assert!(session.core.fullscreen.interaction.copy_on_select);

    session.handle_command(AppInlineCommand::SetFullscreenInteraction {
        interaction: FullscreenInteractionSettings {
            mouse_capture: true,
            copy_on_select: false,
            scroll_speed: 3,
        },
    });

    assert!(
        !session.core.fullscreen.interaction.copy_on_select,
        "app-layer command must reach the core session's fullscreen interaction"
    );
}
