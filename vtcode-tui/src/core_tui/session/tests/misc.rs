use super::super::*;
use super::helpers::*;

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
fn control_e_launches_editor() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
    let result = session.process_key(event);

    assert!(matches!(result, Some(InlineEvent::LaunchEditor)));
}

#[test]
fn control_e_moves_cursor_to_end_when_input_has_content() {
    let text = "hello world";
    let mut session = session_with_input(text, 0);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.cursor(), text.len());
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
        Some(InlineEvent::Overlay(OverlayEvent::Submitted(
            OverlaySubmission::Hotkey(OverlayHotkeyAction::LaunchEditor)
        )))
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

    let modal = session
        .modal_state()
        .expect("plan confirmation modal should be present");
    assert_eq!(modal.title, "Ready to code?");
    assert_eq!(
        modal.lines.first().map(String::as_str),
        Some("A plan is ready to execute. Would you like to proceed?")
    );

    let list = modal
        .list
        .as_ref()
        .expect("plan confirmation should include list options");
    assert_eq!(list.items.len(), 3);

    assert_eq!(list.items[0].title, "Yes, auto-accept edits");
    assert_eq!(
        list.items[0].subtitle.as_deref(),
        Some("Execute with auto-approval.")
    );
    assert_eq!(list.items[0].badge.as_deref(), Some("Recommended"));

    assert_eq!(list.items[1].title, "Yes, manually approve edits");
    assert_eq!(
        list.items[1].subtitle.as_deref(),
        Some("Keep context and confirm each edit before applying.")
    );

    assert_eq!(list.items[2].title, "Type feedback to revise the plan");
    assert_eq!(
        list.items[2].subtitle.as_deref(),
        Some("Return to plan mode and refine the plan.")
    );
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
            !matches!(result, Some(InlineEvent::LaunchEditor)),
            "Right arrow with modifiers {:?} should not launch editor",
            modifiers
        );
    }

    // Test other arrow keys for safety
    for key_code in [KeyCode::Left, KeyCode::Up, KeyCode::Down] {
        let event = KeyEvent::new(key_code, KeyModifiers::SUPER);
        let result = session.process_key(event);
        assert!(
            !matches!(result, Some(InlineEvent::LaunchEditor)),
            "{:?} with SUPER should not launch editor",
            key_code
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
fn active_file_operation_indicator_renders_spinner_frame() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Info,
        vec![make_segment("❋ Editing vtcode.toml...")],
    );
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: edit_file".to_string()),
        right: None,
    });

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("⠋ Editing vtcode.toml...")),
        "active file operation indicator should show a spinner frame"
    );
    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("❋ Editing vtcode.toml...")),
        "spinner should replace the static file operation marker while active"
    );
}

#[test]
fn non_file_tool_status_keeps_static_file_operation_indicator() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_line(
        InlineMessageKind::Info,
        vec![make_segment("❋ Editing vtcode.toml...")],
    );
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: unified_search".to_string()),
        right: None,
    });

    let rendered = rendered_transcript_widget_lines(&mut session, VIEW_WIDTH, VIEW_ROWS);

    assert!(
        rendered
            .iter()
            .any(|line| line.contains("❋ Editing vtcode.toml...")),
        "non-file tool activity should not animate stale file operation indicators"
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

    let index = session
        .lines
        .len()
        .checked_sub(1)
        .expect("tool detail line should exist");
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

    let index = session
        .lines
        .len()
        .checked_sub(1)
        .expect("tool detail line should exist");
    let transcript_lines = session.reflow_message_lines(index, 100);
    let task_span = transcript_lines
        .iter()
        .flat_map(|line| line.line.spans.iter())
        .find(|span| span.content.contains("Report actions taken"))
        .expect("expected task span");

    assert!(
        task_span.style.add_modifier.contains(Modifier::DIM),
        "top-level task rows should render dimmed"
    );
}
