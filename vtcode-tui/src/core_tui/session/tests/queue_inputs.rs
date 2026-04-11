use super::helpers::*;
use super::super::*;

#[test]
fn shift_enter_inserts_newline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.input_manager.set_content("queued".to_string());

    let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "queued\n");
    assert_eq!(
        session.input_manager.cursor(),
        session.input_manager.content().len()
    );
}

#[test]
fn paste_preserves_all_newlines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = (0..15)
        .map(|i| format!("line {}", i))
        .collect::<Vec<_>>()
        .join("\n");
    let (tx, _rx) = mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::Paste(pasted.clone()), &tx, None);

    assert_eq!(session.input_manager.content(), pasted);
}

#[test]
fn pasted_message_displays_full_content() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("paste-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.append_pasted_message(
        InlineMessageKind::User,
        pasted_text.clone(),
        pasted_lines.len(),
    );

    let user_line = session
        .lines
        .iter()
        .find(|line| line.kind == InlineMessageKind::User)
        .expect("user line should exist");
    let combined: String = user_line
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    assert!(combined.contains("paste-1"));
    assert!(session.collapsed_pastes.is_empty());
}

#[test]
fn pasted_message_collapses_large_json_for_tool() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let mut json = String::from("{\n");
    let line_total = ui::INLINE_JSON_COLLAPSE_LINE_THRESHOLD + 5;
    for idx in 0..line_total {
        json.push_str(&format!("  \"key{idx}\": \"value{idx}\",\n"));
    }
    json.push_str("  \"end\": true\n}");
    let line_count = json.lines().count();

    session.append_pasted_message(InlineMessageKind::Tool, json.clone(), line_count);

    assert_eq!(session.collapsed_pastes.len(), 1);
    let collapsed_index = session.collapsed_pastes[0].line_index;
    let preview_line = session
        .lines
        .get(collapsed_index)
        .expect("collapsed line exists");
    let preview_text: String = preview_line
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    assert!(preview_text.contains("showing last"));
    assert!(preview_text.contains("\"end\": true"));

    assert!(session.expand_collapsed_paste_at_line_index(collapsed_index));
    assert!(session.collapsed_pastes.is_empty());

    let expanded_line = session
        .lines
        .get(collapsed_index)
        .expect("expanded line exists");
    let expanded_text: String = expanded_line
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect();
    assert!(expanded_text.contains("\"key0\": \"value0\""));
    assert!(expanded_text.contains("\"end\": true"));
}

#[test]
fn input_compact_preview_for_large_paste() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.insert_paste_text(&pasted_text);

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("[Pasted Content"));
}

#[test]
fn idle_enter_submits_immediately() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "queued"));
}

#[test]
fn control_enter_submits_current_draft_immediately() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("process now".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "process now"));
}

#[test]
fn idle_control_enter_with_empty_input_processes_latest_queued_message() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_queued_input("first queued".to_string());
    session.push_queued_input("latest queued".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::ProcessLatestQueued)));
}

#[test]
fn control_l_submits_clear_command() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let event = session.process_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "/clear"));
}

#[test]
fn control_slash_toggles_inline_list_visibility() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
}

#[test]
fn control_i_toggles_inline_list_visibility() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert!(!session.inline_lists_visible());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    assert!(session.inline_lists_visible());
}

#[test]
fn tab_queues_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let queued = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
}

#[test]
fn busy_escape_interrupts_then_exits() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Interrupt)));

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(second, Some(InlineEvent::Exit)));
}

#[test]
fn busy_stop_command_interrupts_immediately() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/stop".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(app_types::InlineEvent::Interrupt)));
}

#[test]
fn busy_pause_command_emits_pause_event() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/pause".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(app_types::InlineEvent::Pause)));
}

#[test]
fn busy_resume_command_emits_resume_event() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/resume".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(app_types::InlineEvent::Resume)));
}

#[test]
fn double_escape_submits_rewind_when_idle() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let _ = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(matches!(second, Some(InlineEvent::Submit(value)) if value == "/rewind"));
}

#[test]
fn alt_up_edits_latest_queued_input() {
    with_terminal_env(None, Some("xterm-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        set_queued_inputs(&mut session, vec!["first".to_string(), "second".to_string()]);

        let event = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert!(matches!(event, Some(InlineEvent::EditQueue)));
        assert_eq!(session.input_manager.content(), "second");
    });
}

#[test]
fn shift_left_edits_latest_queued_input_in_tmux() {
    with_terminal_env(
        Some("/tmp/tmux-1000/default,123,0"),
        Some("tmux-256color"),
        || {
            let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

            set_queued_inputs(&mut session, vec!["first".to_string(), "second".to_string()]);

            let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
            assert!(matches!(event, Some(InlineEvent::EditQueue)));
            assert_eq!(session.input_manager.content(), "second");
        },
    );
}

#[test]
fn app_session_shift_left_edits_latest_queued_input_in_tmux() {
    with_terminal_env(
        Some("/tmp/tmux-1000/default,123,0"),
        Some("tmux-256color"),
        || {
            let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

            set_app_session_queued_inputs(
                &mut session,
                vec!["first".to_string(), "second".to_string()],
            );

            let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
            assert!(matches!(event, Some(app_types::InlineEvent::EditQueue)));
            assert_eq!(session.core.input_manager.content(), "second");
        },
    );
}

#[test]
fn consecutive_duplicate_submissions_not_stored_twice() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("repeat".to_string());
    let first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Submit(value)) if value == "repeat"));

    session.set_input("repeat".to_string());
    let second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(second, Some(InlineEvent::Submit(value)) if value == "repeat"));

    assert_eq!(session.input_manager.history().len(), 1);
}

fn queue_edit_hint() -> String {
    if cfg!(target_os = "macos") {
        "\u{2325} + \u{2191} edit".to_string()
    } else {
        "Alt + \u{2191} edit".to_string()
    }
}

fn tmux_queue_edit_hint() -> String {
    if cfg!(target_os = "macos") {
        "\u{21E7} + \u{2190} edit".to_string()
    } else {
        "Shift + \u{2190} edit".to_string()
    }
}

#[test]
fn queued_inputs_overlay_bottom_rows() {
    with_terminal_env(None, Some("xterm-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        set_queued_inputs(
            &mut session,
            vec![
                "first queued message".to_string(),
                "second queued message".to_string(),
                "third queued message".to_string(),
            ],
        );

        assert_footer_contains(&mut session, 10, "\u{21B3} third queued message");
        assert_footer_contains(&mut session, 10, "\u{21B3} second queued message");
        assert_footer_contains(&mut session, 10, &queue_edit_hint());
    });
}

#[test]
fn queued_inputs_overlay_shows_shift_left_hint_in_tmux() {
    with_terminal_env(
        Some("/tmp/tmux-1000/default,123,0"),
        Some("tmux-256color"),
        || {
            let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
            set_queued_inputs(
                &mut session,
                vec![
                    "first queued message".to_string(),
                    "second queued message".to_string(),
                ],
            );

            assert_footer_contains(&mut session, 10, &tmux_queue_edit_hint());
        },
    );
}

#[test]
fn running_activity_not_overlaid_above_queue_lines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_queued_inputs(
        &mut session,
        vec![
            "first queued message".to_string(),
            "second queued message".to_string(),
        ],
    );
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let mut visible = vec![TranscriptLine::default(); 6];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(|line| line_text(&line.line)).collect();

    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("Running command: test")),
        "running status should not be overlaid in transcript"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("\u{21B3} second queued message")),
        "latest queued message should remain visible"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("\u{21B3} first queued message")),
        "older queued message should remain visible"
    );
}

#[test]
fn running_activity_not_overlaid_without_queue() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: grep".to_string()),
        right: None,
    });

    let mut visible = vec![TranscriptLine::default(); 3];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(|line| line_text(&line.line)).collect();

    assert!(
        !rendered
            .iter()
            .any(|line| line.contains("Running tool: grep")),
        "running status should render only in bottom input status row"
    );
}

#[test]
fn apply_suggested_prompt_replaces_empty_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.apply_suggested_prompt("Review the latest diff.".to_string());

    assert_eq!(session.input_manager.content(), "Review the latest diff.");
    assert!(session.suggested_prompt_state.active);
}

#[test]
fn apply_suggested_prompt_appends_to_existing_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Initial draft".to_string());

    session.apply_suggested_prompt("Review the latest diff.".to_string());

    assert_eq!(
        session.input_manager.content(),
        "Initial draft\n\nReview the latest diff."
    );
    assert!(session.suggested_prompt_state.active);
}

#[test]
fn suggested_prompt_state_clears_after_manual_edit() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.apply_suggested_prompt("Review the latest diff.".to_string());

    session.insert_char('!');

    assert!(!session.suggested_prompt_state.active);
}

#[test]
fn alt_p_requests_inline_prompt_suggestion() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT));

    assert!(matches!(
        event,
        Some(InlineEvent::RequestInlinePromptSuggestion(ref value))
            if value == "Review the current"
    ));
}

#[test]
fn tab_accepts_visible_inline_prompt_suggestion() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current".to_string());
    session.set_inline_prompt_suggestion("Review the current.diff".to_string(), true);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "Review the current.diff");
    assert!(session.inline_prompt_suggestion.suggestion.is_none());
}

#[test]
fn tab_accepts_inline_prompt_suggestion_with_trailing_space_prefix() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current.diff ".to_string());
    session
        .set_inline_prompt_suggestion("Review the current.diff and summarize it".to_string(), true);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(
        session.input_manager.content(),
        "Review the current.diff and summarize it"
    );
    assert!(session.inline_prompt_suggestion.suggestion.is_none());
}

#[test]
fn tab_queues_when_no_inline_prompt_suggestion_is_visible() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current.diff".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(
        event,
        Some(InlineEvent::QueueSubmit(ref value)) if value == "Review the current.diff"
    ));
}

#[test]
fn inline_prompt_suggestion_clears_after_cursor_movement() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current".to_string());
    session.set_inline_prompt_suggestion("Review the current.diff".to_string(), false);

    let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));

    assert!(event.is_none());
    assert!(session.inline_prompt_suggestion.suggestion.is_none());
}

#[test]
fn streaming_state_set_on_agent_append_pasted_message() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    assert!(!session.is_streaming_final_answer);

    session.handle_command(InlineCommand::AppendPastedMessage {
        kind: InlineMessageKind::Agent,
        text: "Hello".to_string(),
        line_count: 1,
    });

    assert!(session.is_streaming_final_answer);
}

#[test]
fn busy_enter_queues_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("keep searching in docs/".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(event, Some(InlineEvent::QueueSubmit(value)) if value == "keep searching in docs/")
    );
}

#[test]
fn busy_control_enter_steers_active_run() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("keep searching in docs/".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(matches!(event, Some(InlineEvent::Steer(value)) if value == "keep searching in docs/"));
}

#[test]
fn busy_tab_still_queues_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("queue this next".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::QueueSubmit(value)) if value == "queue this next"));
}
