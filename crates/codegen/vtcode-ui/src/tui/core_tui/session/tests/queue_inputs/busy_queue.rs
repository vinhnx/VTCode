use super::*;

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
