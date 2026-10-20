#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use super::super::*;
use super::helpers::*;
use crate::tui::core_tui::app::types::InlineEvent as AppInlineEvent;
use crate::tui::core_tui::types::{ContentPart, SubmittedInput};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

#[test]
fn shift_enter_inserts_newline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.input_manager.set_content("queued".to_string());

    let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), "queued\n");
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());
}

#[test]
fn paste_preserves_all_newlines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let pasted = (0..15).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
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

    session.append_pasted_message(InlineMessageKind::User, pasted_text.clone(), pasted_lines.len());

    let user_line = session
        .lines
        .iter()
        .find(|line| line.kind == InlineMessageKind::User)
        .expect("user line should exist");
    let combined: String = user_line.segments.iter().map(|segment| segment.text.as_str()).collect();
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
    let preview_line = session.lines.get(collapsed_index).expect("collapsed line exists");
    let preview_text: String = preview_line.segments.iter().map(|segment| segment.text.as_str()).collect();
    assert!(preview_text.contains("showing last"));
    assert!(preview_text.contains("\"end\": true"));

    assert!(session.expand_collapsed_paste_at_line_index(collapsed_index));
    assert!(session.collapsed_pastes.is_empty());

    let expanded_line = session.lines.get(collapsed_index).expect("expanded line exists");
    let expanded_text: String = expanded_line.segments.iter().map(|segment| segment.text.as_str()).collect();
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
fn input_compact_preview_keeps_text_after_large_paste_visible() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("before".to_string());
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.insert_paste_text(&pasted_text);
    session.insert_char(' ');
    for ch in "after".chars() {
        session.insert_char(ch);
    }

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("before"));
    assert!(rendered.contains("[Pasted Content"));
    assert!(rendered.contains("after"));
    assert_eq!(session.input_manager.content(), format!("before{pasted_text} after"));
}

#[test]
fn shift_enter_after_large_paste_inserts_newline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello ".to_string());
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 1;
    let pasted_lines: Vec<String> = (1..=line_total).map(|idx| format!("line-{idx}")).collect();
    let pasted_text = pasted_lines.join("\n");

    session.insert_paste_text(&pasted_text);
    session.insert_char(' ');
    for ch in "and what are you talking about??".chars() {
        session.insert_char(ch);
    }

    let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));

    assert!(result.is_none());
    assert_eq!(session.input_manager.content(), format!("hello {pasted_text} and what are you talking about??\n"));
    assert_eq!(session.input_manager.cursor(), session.input_manager.content().len());

    let data = session.build_input_widget_data(VIEW_WIDTH, VIEW_ROWS);
    let rendered = text_content(&data.text);
    assert!(rendered.contains("talking about??\n"));
    assert_eq!(data.cursor_y, 1);
    assert!(session.desired_input_lines(VIEW_WIDTH) >= 2);
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
fn tab_cycles_primary_agent_with_submission_text() {
    // Plain Tab now submits like Ctrl+Enter; Shift+Tab cycles.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    session.set_input("queued".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "queued"));
    assert_eq!(session.input_manager.content(), "");

    session.set_input("queued".to_string());
    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
    assert!(matches!(cycle, Some(InlineEvent::CyclePrimaryAgent)));
    assert_eq!(session.input_manager.content(), "queued");
}

#[test]
fn busy_escape_interrupts() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Interrupt)));

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(second, Some(InlineEvent::Interrupt)));
}

#[test]
fn repeated_control_c_exits_without_double_escape() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let first = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(matches!(first, Some(InlineEvent::Interrupt)));

    let second = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(matches!(second, Some(InlineEvent::Exit)));
}

#[test]
fn double_idle_escape_submits_rewind() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(first, Some(InlineEvent::Cancel)));

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(&second, Some(InlineEvent::Submit(value)) if value == "/rewind"));

    // A third press starts a fresh single-press cancel cycle.
    let third = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(third, Some(InlineEvent::Cancel)));
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
    with_terminal_env(Some("/tmp/tmux-1000/default,123,0"), Some("tmux-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        set_queued_inputs(&mut session, vec!["first".to_string(), "second".to_string()]);

        let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(matches!(event, Some(InlineEvent::EditQueue)));
        assert_eq!(session.input_manager.content(), "second");
    });
}

#[test]
fn app_session_shift_left_edits_latest_queued_input_in_tmux() {
    with_terminal_env(Some("/tmp/tmux-1000/default,123,0"), Some("tmux-256color"), || {
        let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);

        set_app_session_queued_inputs(&mut session, vec!["first".to_string(), "second".to_string()]);

        let event = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert!(matches!(event, Some(app_types::InlineEvent::EditQueue)));
        assert_eq!(session.core.input_manager.content(), "second");
    });
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

        assert_footer_contains(&mut session, 10, "\u{21B3} [3/3] third queued message");
        assert_footer_contains(&mut session, 10, "\u{21B3} [2/3] second queued message");
        assert_footer_contains(&mut session, 10, &queue_edit_hint());
    });
}

#[test]
fn queued_inputs_overlay_shows_shift_left_hint_in_tmux() {
    with_terminal_env(Some("/tmp/tmux-1000/default,123,0"), Some("tmux-256color"), || {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        set_queued_inputs(&mut session, vec!["first queued message".to_string(), "second queued message".to_string()]);

        assert_footer_contains(&mut session, 10, &tmux_queue_edit_hint());
    });
}

#[test]
fn running_activity_not_overlaid_above_queue_lines() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_queued_inputs(&mut session, vec!["first queued message".to_string(), "second queued message".to_string()]);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running command: test".to_string()),
        right: None,
    });

    let mut visible = vec![TranscriptLine::default(); 6];
    session.overlay_queue_lines(&mut visible, VIEW_WIDTH);
    let rendered: Vec<String> = visible.iter().map(|line| line_text(&line.line)).collect();

    assert!(
        !rendered.iter().any(|line| line.contains("Running command: test")),
        "running status should not be overlaid in transcript"
    );
    assert!(
        rendered
            .iter()
            .any(|line| line.contains("\u{21B3} [2/2] second queued message")),
        "latest queued message should remain visible"
    );
    assert!(
        rendered.iter().any(|line| line.contains("\u{21B3} [1/2] first queued message")),
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
        !rendered.iter().any(|line| line.contains("Running tool: grep")),
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

    assert_eq!(session.input_manager.content(), "Initial draft\n\nReview the latest diff.");
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
    session.set_inline_prompt_suggestion("Review the current.diff and summarize it".to_string(), true);

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "Review the current.diff and summarize it");
    assert!(session.inline_prompt_suggestion.suggestion.is_none());
}

#[test]
fn tab_cycles_primary_agent_when_no_inline_prompt_suggestion_is_visible() {
    // Without a suggestion, plain Tab submits like Ctrl+Enter.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("Review the current.diff".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "Review the current.diff"));
    assert_eq!(session.input_manager.content(), "");

    session.set_input("Review the current.diff".to_string());
    let cycle = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));

    assert!(matches!(cycle, Some(InlineEvent::CyclePrimaryAgent)));
    assert_eq!(session.input_manager.content(), "Review the current.diff");
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
fn busy_enter_queues_for_next_turn() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("keep searching in docs/".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "keep searching in docs/");
            assert!(!value.batchable, "busy plain-Enter must stay its own turn");
        }
        other => panic!("expected non-batchable QueueSubmit, got: {other:?}"),
    }
    // Synchronization + UI feedback: optimistic local queue updated on the
    // same &mut borrow that emitted the event, composer cleared, overlay
    // shows the queued entry above the input.
    assert_eq!(session.queued_inputs, vec!["keep searching in docs/".to_string()]);
    assert_eq!(session.input_manager.content(), "");
    let transcript = visible_transcript(&mut session);
    assert!(
        transcript.iter().any(|line| line.contains("↳ [1/1] keep searching in docs/")),
        "queued input must render in the queue overlay, got: {transcript:?}"
    );
}

#[test]
fn retained_foreground_session_queues_busy_composer() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));
    session.set_input("start a new turn".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "start a new turn"));

    set_busy_status(&mut session);
    session.set_input("queue while busy".to_string());
    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "queue while busy");
            assert!(!value.batchable, "busy plain-Enter must stay its own turn");
        }
        other => panic!("expected non-batchable QueueSubmit, got: {other:?}"),
    }
    assert_eq!(session.queued_inputs, vec!["queue while busy".to_string()]);
}

#[test]
fn app_retained_foreground_session_queues_busy_composer() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.active_pty_sessions = Some(Arc::new(AtomicUsize::new(1)));
    session.core.set_input("start a new turn".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(AppInlineEvent::Submit(value)) if value.text == "start a new turn"));

    set_app_session_busy_status(&mut session);
    session.core.set_input("queue while busy".to_string());
    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(AppInlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "queue while busy");
            assert!(!value.batchable, "busy plain-Enter must stay its own turn");
        }
        other => panic!("expected non-batchable QueueSubmit, got: {other:?}"),
    }
    assert_eq!(session.core.queued_inputs, vec!["queue while busy".to_string()]);
}

#[test]
fn busy_enter_keeps_slash_commands_on_queue_path() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("/model gpt-4o".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::QueueSubmit(value)) if value == "/model gpt-4o"));
}

#[test]
fn busy_control_enter_queues_batchable_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("keep searching in docs/".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "keep searching in docs/");
            assert!(value.batchable, "busy Ctrl+Enter plain text must coalesce like the app path");
        }
        other => panic!("expected batchable QueueSubmit, got: {other:?}"),
    }
    assert_eq!(session.queued_inputs, vec!["keep searching in docs/".to_string()]);
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn busy_control_enter_slash_stays_non_batchable() {
    // Asymmetric counterpart: slash commands keep one-per-turn dispatch and
    // must not merge with a batchable batch.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("/model gpt-4o".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "/model gpt-4o");
            assert!(!value.batchable, "slash commands must stay one per turn");
        }
        other => panic!("expected non-batchable QueueSubmit, got: {other:?}"),
    }
}

#[test]
fn busy_tab_queues_batchable_submission() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("tabbed follow-up".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "tabbed follow-up");
            assert!(value.batchable, "busy Tab plain text must coalesce like Ctrl+Enter");
        }
        other => panic!("expected batchable QueueSubmit, got: {other:?}"),
    }
    assert_eq!(session.queued_inputs, vec!["tabbed follow-up".to_string()]);
}

#[test]
fn app_busy_control_enter_queues_batchable_submission() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("batch me".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(
        matches!(&event, Some(AppInlineEvent::QueueSubmit(value)) if value.text == "batch me" && value.batchable),
        "expected batchable QueueSubmit, got: {event:?}"
    );
}

#[test]
fn app_busy_plain_enter_queues_for_next_turn() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("one turn".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(AppInlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "one turn");
            assert!(!value.batchable, "busy plain-Enter must stay its own turn");
        }
        other => panic!("expected non-batchable QueueSubmit, got: {other:?}"),
    }
    assert_eq!(session.core.queued_inputs, vec!["one turn".to_string()]);
    assert_eq!(session.core.input_manager.content(), "");
}

#[test]
fn busy_plain_enter_queues_preserve_fifo_order() {
    // Asymmetric FIFO check: two distinct busy Enters must land oldest-first
    // so the first typed runs first after the current turn.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);

    session.set_input("first queued".to_string());
    let first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&first, Some(InlineEvent::QueueSubmit(value)) if value.text == "first queued" && !value.batchable),
        "expected first non-batchable QueueSubmit, got: {first:?}"
    );

    session.set_input("second queued".to_string());
    let second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&second, Some(InlineEvent::QueueSubmit(value)) if value.text == "second queued" && !value.batchable),
        "expected second non-batchable QueueSubmit, got: {second:?}"
    );

    assert_eq!(session.queued_inputs, vec!["first queued".to_string(), "second queued".to_string()]);
    let transcript = visible_transcript(&mut session);
    assert!(
        transcript.iter().any(|line| line.contains("↳ [1/2] first queued")),
        "oldest queued entry must render first, got: {transcript:?}"
    );
    assert!(
        transcript.iter().any(|line| line.contains("↳ [2/2] second queued")),
        "newest queued entry must render second, got: {transcript:?}"
    );
}

#[test]
fn composer_status_yields_to_turn_activity_status() {
    // The active turn's own phase status owns the composer line; a queued
    // acknowledgement must not displace it.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.push_queued_input("queued follow-up".to_string());

    let line = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .expect("composer status line");
    let text: String = line.iter().map(|span| span.content.as_ref()).collect();
    assert!(text.contains("Running tool: code_search"), "expected turn status, got: {text:?}");
    assert!(!text.contains("Queued:"), "queued ack must not displace turn status, got: {text:?}");
}

#[test]
fn composer_status_hides_queued_acknowledgement_when_idle() {
    // Idle queues are already visible in the transcript overlay; the composer
    // line must not keep asserting a running turn.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.push_queued_input("idle queued draft".to_string());

    let text = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .map(|line| line.iter().map(|span| span.content.as_ref()).collect::<String>())
        .unwrap_or_default();
    assert!(
        !text.contains("Queued:") && !text.contains("turns queued"),
        "idle composer must not claim a queued turn, got: {text:?}"
    );
}

#[test]
fn composer_status_reports_single_queued_follow_up_while_busy() {
    // The active turn's own "Running tool: …" status is the reserved phase
    // acknowledgement, so it keeps priority over the queued preview.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("check the failing test next".to_string());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(session.queued_inputs.len(), 1);

    let line = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .expect("composer status line");
    let text: String = line.iter().map(|span| span.content.as_ref()).collect();
    assert!(text.contains("Running tool: code_search"), "expected turn status, got: {text:?}");
}

#[test]
fn composer_status_reports_queued_count_while_transcript_progress_owns_row() {
    // Reported gap: while the transcript loading row shows a progress phase
    // the composer keeps no runtime status text, so the queued follow-up
    // acknowledgement must still surface there.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.handle_command(InlineCommand::UpdateProgress(vtcode_commons::ui_protocol::ProgressUpdate::Begin {
        operation: vtcode_commons::ui_protocol::ProgressOperation::start(),
        phase: vtcode_commons::ui_protocol::ProgressPhase::RunningTools,
    }));
    for text in ["first", "second"] {
        session.set_input(text.to_string());
        let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }
    assert_eq!(session.queued_inputs.len(), 2);

    let line = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .expect("composer status line");
    let text: String = line.iter().map(|span| span.content.as_ref()).collect();
    assert!(
        text.contains("2 turns queued — running after this turn"),
        "expected queued count in the composer status line, got: {text:?}"
    );
}

#[test]
fn composer_status_truncates_long_single_queued_follow_up() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.handle_command(InlineCommand::UpdateProgress(vtcode_commons::ui_protocol::ProgressUpdate::Begin {
        operation: vtcode_commons::ui_protocol::ProgressOperation::start(),
        phase: vtcode_commons::ui_protocol::ProgressPhase::WaitingForModel,
    }));
    let long = "a".repeat(120);
    session.set_input(long.clone());
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let line = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .expect("composer status line");
    let text: String = line.iter().map(|span| span.content.as_ref()).collect();
    assert!(text.contains(&format!("Queued: {}…", "a".repeat(40))), "expected truncated preview, got: {text:?}");
    assert!(!text.contains(&long), "composer must not echo the full queued text, got: {text:?}");
}

#[test]
fn composer_status_queued_acknowledgement_yields_to_configured_context() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.handle_command(InlineCommand::SetConfiguredInputStatus {
        left: Some("Configured context".to_string()),
        right: None,
    });
    session.push_queued_input("queued follow-up".to_string());

    let line = session
        .build_input_status_widget_data(VIEW_WIDTH)
        .expect("composer status line");
    let text: String = line.iter().map(|span| span.content.as_ref()).collect();
    assert!(text.contains("Configured context"), "configured context must keep priority, got: {text:?}");
    assert!(!text.contains("Queued:"), "queued ack must not displace configured context, got: {text:?}");
}

#[test]
fn queued_inputs_cap_keeps_newest() {
    // Asymmetric boundary: 260 pushes must keep the newest 256, dropping the
    // 4 oldest, so the optimistic overlay mirrors the authoritative cap.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    for i in 0..260 {
        session.push_queued_input(format!("msg {i}"));
    }
    assert_eq!(session.queued_inputs.len(), 256);
    assert_eq!(session.queued_inputs.first().map(String::as_str), Some("msg 4"));
    assert_eq!(session.queued_inputs.last().map(String::as_str), Some("msg 259"));
}

#[test]
fn busy_plain_enter_with_attachments_queues_single_turn() {
    // Queued attachments must stay with their message as one turn; the old
    // Steer path rejected attachments with a warning + draft restore.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("see image".to_string());
    session
        .input_manager
        .set_attachments(vec![ContentPart::image("encoded", "image/png")]);

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    match &event {
        Some(InlineEvent::QueueSubmit(value)) => {
            assert_eq!(value.text, "see image");
            assert!(value.has_attachments(), "attachment must stay queued with its message");
            assert!(!value.batchable, "attachment queue entries must dispatch alone");
        }
        other => panic!("expected QueueSubmit with attachment, got: {other:?}"),
    }
    assert_eq!(session.queued_inputs, vec!["see image".to_string()]);
}

#[test]
fn app_busy_slash_command_copy_submits_immediately() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/copy".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(&event, Some(AppInlineEvent::Submit(value)) if value.text == "/copy"),
        "expected immediate Submit, got: {event:?}"
    );
}

#[test]
fn app_busy_control_enter_queues_slash_command_as_non_batchable() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    set_app_session_busy_status(&mut session);
    session.core.set_input("/model gpt-4o".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    assert!(
        matches!(&event, Some(AppInlineEvent::QueueSubmit(value)) if value.text == "/model gpt-4o" && !value.batchable),
        "expected non-batchable QueueSubmit for a slash command, got: {event:?}"
    );
}

#[test]
fn restore_input_draft_command_preserves_attachments_in_order() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let first_attachment = ContentPart::image("first-image", "image/png");
    let second_attachment = ContentPart::image("second-image", "image/png");

    session.handle_command(InlineCommand::RestoreInputDraft(SubmittedInput::new(
        "keep searching in docs/",
        vec![first_attachment.clone(), second_attachment.clone()],
    )));

    assert_eq!(session.input_manager.content(), "keep searching in docs/");
    assert_eq!(session.input_manager.attachments(), &[first_attachment, second_attachment]);
}

#[test]
fn app_restore_input_draft_command_preserves_attachments_in_order() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    let first_attachment = ContentPart::image("first-image", "image/png");
    let second_attachment = ContentPart::image("second-image", "image/png");

    session.handle_command(app_types::InlineCommand::RestoreInputDraft(SubmittedInput::new(
        "keep searching in docs/",
        vec![first_attachment.clone(), second_attachment.clone()],
    )));

    assert_eq!(session.core.input_manager.content(), "keep searching in docs/");
    assert_eq!(session.core.input_manager.attachments(), &[first_attachment, second_attachment]);
}

#[test]
fn busy_tab_does_not_cycle_primary_agent() {
    // Busy Tab queues like Ctrl+Enter instead of cycling.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("queue this next".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::QueueSubmit(value)) if value == "queue this next"));
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn busy_tab_shows_mode_switch_notice() {
    // Mode-switch notice now lives on Shift+Tab (BackTab); plain Tab queues.
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);

    let _ = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    let transcript = visible_transcript(&mut session);
    assert!(
        transcript.iter().any(|line| line.contains("Mode switching is disabled")),
        "expected a mode-switch busy notice, got: {transcript:?}"
    );
}

#[test]
fn command_backspace_clears_single_line_entire_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello world".to_string());
    session.input_manager.set_cursor(5);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn command_a_clears_single_line_entire_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn command_backspace_clears_only_current_line_in_multiline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    // Cursor inside "second" (offset 8).
    session.input_manager.set_cursor(8);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "first\n\nthird");
    assert_eq!(session.input_manager.cursor(), 6);
}

#[test]
fn command_a_clears_only_current_line_in_multiline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    session.input_manager.set_cursor(1);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "\nsecond\nthird");
}

#[test]
fn command_left_and_right_move_within_current_line() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    session.input_manager.set_cursor(8);

    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 6);

    let _ = session.process_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 12);
}

#[test]
fn command_left_and_right_single_line_match_buffer_edges() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());
    session.input_manager.set_cursor(2);

    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 0);

    let _ = session.process_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER));
    assert_eq!(session.input_manager.cursor(), 5);
}

#[test]
fn command_backspace_clears_compact_paste_block_atomically() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    let line_total = ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD + 2;
    let pasted = (0..line_total).map(|i| format!("paste-{i}")).collect::<Vec<_>>().join("\n");
    session.insert_paste_text(&pasted);
    assert!(session.input_manager.compact_paste_range().is_some());
    // Cursor in the middle of the collapsed block.
    let mid = session.input_manager.content().len() / 2;
    session.input_manager.set_cursor(mid);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
    assert!(session.input_manager.compact_paste_range().is_none());
}

#[test]
fn command_backspace_clears_single_image_block() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("[Image #1]".to_string());
    session
        .input_manager
        .set_attachments(vec![ContentPart::image("encoded", "image/png")]);

    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::SUPER));
    assert!(event.is_none());
    assert_eq!(session.input_manager.content(), "");
    assert!(session.input_manager.attachments().is_empty());
}

#[test]
fn double_escape_clears_single_line_entire_input() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    assert_eq!(session.input_manager.content(), "hello");

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn double_escape_clears_only_current_line_in_multiline() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("first\nsecond\nthird".to_string());
    session.input_manager.set_cursor(8);

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    assert_eq!(session.input_manager.content(), "first\nsecond\nthird");

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    assert_eq!(session.input_manager.content(), "first\n\nthird");
}

#[test]
fn single_escape_followed_by_other_key_does_not_clear() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("hello".to_string());

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    let _ = session.process_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    // Second Esc is a fresh first press, so content remains.
    assert_eq!(session.input_manager.content(), "hello");
}

#[test]
fn tab_enqueues_like_control_enter_when_idle() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.set_input("queue me".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::Submit(value)) if value == "queue me"));
    assert_eq!(session.input_manager.content(), "");
}

#[test]
fn tab_enqueues_like_control_enter_when_busy() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    set_busy_status(&mut session);
    session.set_input("busy draft".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(event, Some(InlineEvent::QueueSubmit(value)) if value == "busy draft"));
}

#[test]
fn shift_tab_cycles_forward_and_backtab_cycles_previous() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

    let forward = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
    assert!(matches!(forward, Some(InlineEvent::CyclePrimaryAgent)));

    let previous = session.process_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert!(matches!(previous, Some(InlineEvent::CyclePrimaryAgentPrevious)));
}

#[test]
fn app_tab_enqueues_like_control_enter_when_idle() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("app draft".to_string());

    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(
        matches!(&event, Some(AppInlineEvent::Submit(value)) if value.text == "app draft"),
        "expected Submit, got: {event:?}"
    );
    assert_eq!(session.core.input_manager.content(), "");
}

#[test]
fn app_double_escape_clears_current_line_in_multiline() {
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("first\nsecond\nthird".to_string());
    session.core.input_manager.set_cursor(8);

    let first = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(first.is_none());
    assert_eq!(session.core.input_manager.content(), "first\nsecond\nthird");

    let second = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(second.is_none());
    assert_eq!(session.core.input_manager.content(), "first\n\nthird");
}

#[test]
fn app_legacy_control_codes_match_cmd_line_editing() {
    // Legacy terminals encode Cmd+Left/Right/Backspace as 0x01/0x05/0x15.
    // They must reach the same line-wise handlers as the SUPER path in the app
    // session (the interactive TUI entry point).
    let mut session = AppSession::new(InlineTheme::default(), None, VIEW_ROWS);
    session.core.set_input("first\nsecond\nthird".to_string());
    session.core.input_manager.set_cursor(8);

    let left = session.process_key(KeyEvent::new(KeyCode::Char('\u{1}'), KeyModifiers::NONE));
    assert!(left.is_none());
    assert_eq!(session.core.input_manager.cursor(), 6);

    let right = session.process_key(KeyEvent::new(KeyCode::Char('\u{5}'), KeyModifiers::NONE));
    assert!(right.is_none());
    assert_eq!(session.core.input_manager.cursor(), 12);

    let clear = session.process_key(KeyEvent::new(KeyCode::Char('\u{15}'), KeyModifiers::NONE));
    assert!(clear.is_none());
    assert_eq!(session.core.input_manager.content(), "first\n\nthird");
}

#[test]
fn modified_enter_preserves_command_drafts_and_literal_backslashes() {
    for running in [false, true] {
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::ALT] {
            for draft in ["/stop", "/plan", "/help", "/stop\\", "/plan\\", "/help\\", "literal\\"] {
                let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
                if running {
                    session.handle_command(InlineCommand::SetActivityState(ActivityState::Building));
                }
                session.input_manager.set_content(draft.to_string());
                let event = session.process_key(KeyEvent::new(KeyCode::Enter, modifiers));
                assert!(event.is_none(), "{draft} {modifiers:?} running={running}");
                assert_eq!(session.input_manager.content(), format!("{draft}\n"));
                assert!(session.queued_inputs.is_empty());
            }
        }
    }
}

#[test]
fn composer_enter_and_control_j_keep_existing_newline_and_submit_behavior() {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.input_manager.set_content("literal\\".to_string());
    assert!(session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).is_none());
    assert_eq!(session.input_manager.content(), "literal\n");
    session.input_manager.set_content("/help".to_string());
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert_eq!(session.input_manager.content(), "/help\n");
    for modifiers in [
        KeyModifiers::NONE,
        KeyModifiers::CONTROL,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ] {
        session.input_manager.set_content("send this".to_string());
        assert!(
            matches!(session.process_key(KeyEvent::new(KeyCode::Enter, modifiers)), Some(InlineEvent::Submit(input)) if input.text == "send this")
        );
    }
}
