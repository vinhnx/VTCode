use super::*;

#[test]
fn secure_prompt_modal_typing_populates_input_buffer() {
    let mut session = build_session_with_secure_prompt();

    // Typed characters must reach the input buffer, not be silently consumed.
    for ch in ['s', 'k', '-', 't', 'e', 's', 't'] {
        let event = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        assert!(event.is_none(), "character '{ch}' should not emit an event");
    }

    assert_eq!(session.core.input_manager.content(), "sk-test");
    assert!(session.has_active_overlay(), "modal should remain open while typing");
}
#[test]
fn secure_prompt_modal_enter_submits_and_closes() {
    let mut session = build_session_with_secure_prompt();

    for ch in ['s', 'k', '-', 't', 'e', 's', 't'] {
        let _ = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    match event {
        Some(InlineEvent::Submit(input)) => {
            assert_eq!(&*input, "sk-test", "Enter should submit the typed API key");
        }
        other => panic!("Enter should emit Submit, got {other:?}"),
    }
    assert!(!session.has_active_overlay(), "modal should be closed after Enter");
    assert!(session.core.input_manager.content().is_empty(), "input buffer should be cleared after submit");
}
#[test]
fn secure_prompt_modal_esc_cancels_and_closes() {
    let mut session = build_session_with_secure_prompt();

    for ch in ['s', 'k', '-', 't', 'e', 's', 't'] {
        let _ = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(event.is_none(), "Esc should not emit a submit event, got {event:?}");
    assert!(!session.has_active_overlay(), "modal should be closed after Esc");
    assert!(session.core.input_manager.content().is_empty(), "input buffer should be cleared after Esc");
}
#[test]
fn secure_prompt_modal_paste_auto_submits_and_closes() {
    let mut session = build_session_with_secure_prompt();

    let event = handle_paste(&mut session, "sk-pasted-key\n");

    match event {
        Some(InlineEvent::Submit(input)) => {
            assert_eq!(&*input, "sk-pasted-key", "paste should auto-submit trimmed key");
        }
        other => panic!("paste should emit Submit, got {other:?}"),
    }
    assert!(!session.has_active_overlay(), "modal should be closed after paste");
}
#[test]
fn secure_prompt_modal_enter_with_empty_input_does_not_close() {
    let mut session = build_session_with_secure_prompt();

    // Pressing Enter without typing anything should not close the modal.
    let event = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(event.is_none(), "Enter with empty input should not emit an event");
    assert!(session.has_active_overlay(), "modal should remain open when input is empty");
}
#[test]
fn secure_prompt_modal_backspace_and_ctrl_u_edit_input() {
    let mut session = build_session_with_secure_prompt();

    for ch in ['s', 'k', '-', 't', 'e', 's', 't'] {
        let _ = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(session.core.input_manager.content(), "sk-test");

    // Backspace deletes the last character.
    let event = session.process_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(event.is_none(), "Backspace should not emit an event");
    assert_eq!(session.core.input_manager.content(), "sk-tes");
    assert!(session.has_active_overlay(), "modal should remain open while editing");

    // Ctrl+U clears from cursor to start of line.
    let event = session.process_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(event.is_none(), "Ctrl+U should not emit an event");
    assert!(session.core.input_manager.content().is_empty(), "Ctrl+U should clear the buffer");
    assert!(session.has_active_overlay(), "modal should remain open after Ctrl+U");
}
#[test]
fn escape_dismisses_transcript_selection_before_interrupt() {
    let mut session = build_session();
    session.core.mouse_selection.set_selection((1, 1), (8, 1));

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(event.is_none(), "dismissing a selection consumes Esc, got {event:?}");
    assert!(!session.core.mouse_selection.has_selection, "Esc must clear the highlight");
}
#[test]
fn viewer_control_click_dismisses_stale_transcript_selection() {
    let mut session = build_session();
    add_compact_activity(&mut session, 47, "printf dismiss");
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    // A completed selection left over from the transcript must not survive a
    // click on a viewer control that owns that click.
    session.core.mouse_selection.set_selection((1, 1), (8, 1));
    assert!(session.core.mouse_selection.has_selection);

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render review");
    let close_column = (0..80)
        .find(|column| {
            session
                .tool_output_viewer_state()
                .is_some_and(|viewer| viewer.close_control_contains(*column, 0))
        })
        .expect("rendered review title should expose a close hit region");

    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: close_column,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );

    assert!(session.tool_output_viewer_state().is_none(), "close control must still act");
    assert!(
        !session.core.mouse_selection.has_selection,
        "clicking a viewer control must dismiss the stale highlight"
    );
}
#[test]
fn secure_prompt_modal_consumes_composer_shortcuts() {
    let mut session = build_session_with_secure_prompt();

    // Ctrl+M would normally submit "/model" from the composer; inside a secure
    // prompt it must be consumed so the modal stays open and no event leaks.
    let event = session.process_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL));
    assert!(event.is_none(), "Ctrl+M must not leak through the secure prompt, got {event:?}");
    assert!(session.has_active_overlay(), "modal should remain open when Ctrl+M is pressed");

    // Tab is consumed (not inserted, no agent cycling) while the secure prompt is open.
    let event = session.process_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(event.is_none(), "Tab must not leak through the secure prompt, got {event:?}");
    assert!(session.core.input_manager.content().is_empty(), "Tab must not insert a character");
    assert!(session.has_active_overlay(), "modal should remain open when Tab is pressed");
}
#[test]
fn modified_enter_preserves_command_drafts_and_literal_backslashes() {
    for running in [false, true] {
        for modifiers in [KeyModifiers::SHIFT, KeyModifiers::ALT] {
            for draft in ["/stop", "/plan", "/help", "/stop\\", "/plan\\", "/help\\", "literal\\"] {
                let mut session = build_session();
                if running {
                    session.core.handle_command(CoreInlineCommand::SetActivityState(
                        vtcode_commons::ui_protocol::ActivityState::Building,
                    ));
                }
                session.core.input_manager.set_content(draft.to_string());
                let event = session.process_key(KeyEvent::new(KeyCode::Enter, modifiers));
                assert!(event.is_none(), "{draft} {modifiers:?} running={running}");
                assert_eq!(session.core.input_manager.content(), format!("{draft}\n"));
                assert!(session.core.queued_inputs.is_empty());
                assert!(!session.has_active_overlay());
            }
        }
    }
}
#[test]
fn composer_enter_and_control_j_keep_existing_newline_and_submit_behavior() {
    let mut session = build_session();
    session.core.input_manager.set_content("literal\\".to_string());
    assert!(session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).is_none());
    assert_eq!(session.core.input_manager.content(), "literal\n");
    session.core.input_manager.set_content("/help".to_string());
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert_eq!(session.core.input_manager.content(), "/help\n");
    for modifiers in [
        KeyModifiers::NONE,
        KeyModifiers::CONTROL,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ] {
        session.core.input_manager.set_content("send this".to_string());
        assert!(
            matches!(session.process_key(KeyEvent::new(KeyCode::Enter, modifiers)), Some(InlineEvent::Submit(input)) if input.text == "send this")
        );
    }
}
#[test]
fn diagnostic_details_affordance_click_expands_through_the_production_event_path() {
    use ratatui::backend::TestBackend;
    use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

    // Prose evidence keeps the in-place disclosure.
    let diagnostic = Diagnostic {
        severity: DiagnosticSeverity::Info,
        code: "mcp.catalog_ready".into(),
        summary: "MCP tools are ready. Use /mcp tools to inspect the catalog.".into(),
        agent_details: "Catalog refresh completed for the configured servers.".into(),
        correlation_id: "mcp:catalog".into(),
        actions: Vec::new(),
    };

    let mut session = build_session();
    session.core.upsert_diagnostic(diagnostic.clone());
    assert!(
        !session.core.lines[0].diagnostic.as_ref().unwrap().expanded,
        "the readiness diagnostic starts collapsed"
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render diagnostic");
    let buffer = terminal.backend().buffer();
    let affordance_row = (0..buffer.area.height)
        .find(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, *row)].symbol())
                .collect::<String>()
                .contains("Details")
        })
        .expect("the Details affordance is rendered");
    let body = session.core.transcript_area().expect("transcript body");
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: body.x + 1,
            row: affordance_row,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );
    assert!(
        session.core.lines[0].diagnostic.as_ref().unwrap().expanded,
        "clicking the Details affordance through the production mouse path must expand the diagnostic"
    );

    // A fresh session: clicking the summary row must not toggle.
    let mut session = build_session();
    session.core.upsert_diagnostic(diagnostic);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render diagnostic");
    let body = session.core.transcript_area().expect("transcript body");
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: body.x + 1,
            row: body.y,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );
    assert!(
        !session.core.lines[0].diagnostic.as_ref().unwrap().expanded,
        "the summary row must preserve selection behavior and not toggle details"
    );
}
#[test]
fn diagnostic_click_while_streaming_expands_and_pins_the_viewport() {
    use ratatui::backend::TestBackend;
    use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

    // Production shape from tool failure diagnosis: a user-visible command
    // failure with prose recovery evidence that the transcript discloses in
    // place. Routine `tool.execution_failed` incidents stay silent under the
    // default style, so streaming mechanics pin the visible class instead.
    let diagnostic = Diagnostic {
        severity: DiagnosticSeverity::Warning,
        code: "tool.command_failed".into(),
        summary: "Tool 'exec_command' failed.".into(),
        agent_details: "Likely cause: duplicate create.\nNext action: run task_tracker.list and continue.".into(),
        correlation_id: "tool-diagnosis:streaming-click".into(),
        actions: Vec::new(),
    };

    let mut session = build_session();
    // A running turn: busy activity plus a streaming tail below the incident so
    // the transcript overflows the viewport and bottom-follows.
    session
        .core
        .handle_command(CoreInlineCommand::SetActivityState(vtcode_commons::ui_protocol::ActivityState::Building));
    for index in 0..40 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![text_segment(format!("streamed line {index}"))],
        });
    }
    session.core.upsert_diagnostic(diagnostic);
    for index in 0..2 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![text_segment(format!("tail line {index}"))],
        });
    }
    let is_expanded = |session: &Session| {
        session
            .core
            .lines
            .iter()
            .any(|line| line.diagnostic.as_ref().is_some_and(|entry| entry.expanded))
    };
    assert!(!is_expanded(&session), "the incident starts collapsed");

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("render streaming transcript");
    let affordance_row = |terminal: &Terminal<TestBackend>, needle: &str| {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .find(|row| {
                (0..buffer.area.width)
                    .map(|col| buffer[(col, *row)].symbol())
                    .collect::<String>()
                    .contains(needle)
            })
            .unwrap_or_else(|| panic!("expected a rendered row containing {needle:?}"))
    };
    let body = session.core.transcript_area().expect("transcript body");
    let click = |session: &mut Session, row: u16| {
        let (events, _received) = tokio::sync::mpsc::unbounded_channel();
        session.handle_event(
            CrosstermEvent::Mouse(MouseEvent {
                kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: body.x + 1,
                row,
                modifiers: KeyModifiers::NONE,
            }),
            &events,
            None,
        );
    };

    // Perfect aim at the freshly painted affordance: hit-testing resolves
    // against the painted state, so the toggle itself must land.
    click(&mut session, affordance_row(&terminal, "click or F8 to expand"));
    assert!(is_expanded(&session), "clicking the visible affordance must expand while streaming");

    // Continued streaming must not scroll the just-expanded evidence away: the
    // toggle pins the viewport instead of bottom-following.
    for index in 0..24 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![text_segment(format!("later streamed line {index}"))],
        });
    }
    terminal
        .draw(|frame| session.render(frame))
        .expect("render after continued streaming");
    let rendered = rendered_buffer_text(&terminal);
    assert!(
        rendered.contains("duplicate create"),
        "expanded evidence must stay visible while streaming continues: {rendered}"
    );

    // Asymmetric direction: collapsing through the same path restores the
    // concise view and releases the pin back to bottom-follow.
    click(&mut session, affordance_row(&terminal, "click or F8 to collapse"));
    assert!(!is_expanded(&session), "clicking the affordance again must collapse");
    terminal.draw(|frame| session.render(frame)).expect("render after collapse");
    let rendered = rendered_buffer_text(&terminal);
    assert!(!rendered.contains("duplicate create"), "collapsed evidence must leave the transcript: {rendered}");
    assert!(
        session.core.scroll_manager.offset() > 0,
        "collapsing keeps the pinned viewport stable like any scroll-away; jump-to-bottom resumes follow"
    );
}
#[test]
fn diagnostic_f8_while_streaming_expands_and_pins_the_viewport() {
    use ratatui::backend::TestBackend;
    use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

    // Visible command-failure class: routine incidents stay silent under the
    // default style (see the click test above), so streaming F8 mechanics pin
    // the class users actually see.
    let diagnostic = Diagnostic {
        severity: DiagnosticSeverity::Warning,
        code: "tool.command_failed".into(),
        summary: "Tool 'exec_command' failed.".into(),
        agent_details: "Likely cause: duplicate create.\nNext action: run task_tracker.list and continue.".into(),
        correlation_id: "tool-diagnosis:streaming-f8".into(),
        actions: Vec::new(),
    };

    let mut session = build_session();
    session
        .core
        .handle_command(CoreInlineCommand::SetActivityState(vtcode_commons::ui_protocol::ActivityState::Building));
    for index in 0..40 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![text_segment(format!("streamed line {index}"))],
        });
    }
    session.core.upsert_diagnostic(diagnostic);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| session.render(frame))
        .expect("render streaming transcript");
    let event = session.process_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    assert!(event.is_none(), "F8 toggles locally and emits no runtime event");
    assert!(
        session
            .core
            .lines
            .iter()
            .any(|line| line.diagnostic.as_ref().is_some_and(|entry| entry.expanded)),
        "F8 must expand the latest incident while streaming"
    );

    for index in 0..24 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![text_segment(format!("later streamed line {index}"))],
        });
    }
    terminal
        .draw(|frame| session.render(frame))
        .expect("render after continued streaming");
    let rendered = rendered_buffer_text(&terminal);
    assert!(
        rendered.contains("duplicate create"),
        "F8-expanded evidence must stay visible while streaming continues: {rendered}"
    );
}
#[test]
fn diagnostic_structured_payload_has_no_transcript_disclosure_affordance() {
    use ratatui::backend::TestBackend;
    use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity};

    // Production payload shape from `handle_mcp_updates` (mcp.catalog_ready).
    let diagnostic = Diagnostic {
        severity: DiagnosticSeverity::Info,
        code: "mcp.catalog_ready".into(),
        summary: "MCP tools are ready. Use /mcp tools to inspect the catalog.".into(),
        agent_details: r#"{"phase":"ready:2_registered","error":null}"#.into(),
        correlation_id: "mcp:catalog".into(),
        actions: Vec::new(),
    };

    let mut session = build_session();
    session.core.upsert_diagnostic(diagnostic);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render diagnostic");
    let buffer = terminal.backend().buffer();
    let rendered = (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !rendered.contains("Details"),
        "a structured payload stays out of the transcript, so it has no affordance: {rendered}"
    );
    assert!(
        !rendered.contains("ready:2_registered"),
        "a structured payload stays out of the transcript: {rendered}"
    );
    assert!(
        !session.core.lines[0].diagnostic.as_ref().unwrap().expanded,
        "a structured payload cannot be expanded in the transcript"
    );
}
