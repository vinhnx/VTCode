use super::*;

#[test]
fn sticky_prompt_keeps_compact_review_hint_hit_regions_aligned() {
    let mut session = build_session();
    session
        .core
        .push_line(InlineMessageKind::User, vec![text_segment("original prompt")]);
    for _ in 0..24 {
        session.core.push_line(InlineMessageKind::Agent, vec![text_segment("answer")]);
    }
    add_compact_activity(&mut session, 42, "printf sticky");
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render sticky activity");
    let body = session.core.transcript_area().unwrap();
    let region = session
        .compact_activity_hit_regions
        .first()
        .copied()
        .expect("review hint below header");
    assert_eq!(region.review_anchor, 42);
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(body.x, body.y - 1)].symbol(), "o", "sticky prompt is visible");
    assert!(buffer[(region.area.x, region.area.y)].modifier.contains(Modifier::UNDERLINED));
    let original_top = session.core.transcript_view_top;
    for _ in 0..3 {
        terminal.draw(|frame| session.render(frame)).expect("repeat frame");
        assert_eq!(session.core.transcript_view_top, original_top);
        assert_eq!(session.core.transcript_area(), Some(body));
    }
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: region.area.x,
            row: region.area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );
    assert!(session.tool_output_viewer_state().is_some());
    terminal.draw(|frame| session.render(frame)).expect("render covering viewer");
    assert!(
        !session.core.handle_sticky_prompt_click(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: body.x,
            row: body.y - 1,
            modifiers: KeyModifiers::NONE,
        }),
        "covered header target is invalidated"
    );
}
#[test]
fn compact_review_hint_click_opens_focused_transcript_review() {
    let mut session = build_session();
    add_compact_activity(&mut session, 41, "printf hello");
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render compact activity");

    let region = session
        .compact_activity_hit_regions
        .first()
        .copied()
        .expect("visible compact review hint should have a hit region");
    assert_eq!(region.review_anchor, 41);

    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: region.area.x,
            row: region.area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );

    assert!(session.tool_output_viewer_state().is_some());
}
#[test]
fn compact_review_hint_hit_regions_survive_narrow_reflow() {
    let mut session = build_session();
    session.core.apply_transcript_width(12);
    add_compact_activity(&mut session, 45, "printf narrow");
    let backend = ratatui::backend::TestBackend::new(12, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render compact activity");

    assert!(!session.compact_activity_hit_regions.is_empty());
    assert!(
        session
            .compact_activity_hit_regions
            .iter()
            .all(|region| region.area.width > 0 && region.area.height == 1)
    );
}
#[test]
fn compact_review_body_click_does_not_open_viewer() {
    let mut session = build_session();
    add_compact_activity(&mut session, 42, "printf body");
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render compact activity");
    let region = session
        .compact_activity_hit_regions
        .first()
        .copied()
        .expect("visible compact review hint should have a hit region");
    let body_column = region.area.x.saturating_sub(1);

    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: body_column,
            row: region.area.y,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );

    assert!(session.tool_output_viewer_state().is_none());
}
#[test]
fn expanded_pty_capture_anchors_to_live_header() {
    let mut session = build_session();
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Pty,
        segments: vec![text_segment("• Ran cargo check")],
    });
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 47,
        lines: vec!["• Ran cargo check".to_string(), "  └ captured output".to_string()],
    });

    assert_eq!(session.tool_output_blocks[0].anchor_line, Some(0));
}
#[test]
fn wrapped_pty_capture_anchors_to_the_folded_live_header() {
    let mut session = build_session();
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Pty,
        segments: vec![text_segment("• Ran cargo nextest run -p vtcode-ui --profile")],
    });
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Pty,
        segments: vec![text_segment("  │ quick --no-fail-fast")],
    });
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 48,
        lines: vec![
            "• Ran cargo nextest run -p vtcode-ui --profile quick --no-fail-fast".to_string(),
            "  └ captured output".to_string(),
        ],
    });

    assert_eq!(session.tool_output_blocks[0].anchor_line, Some(0));
}
#[test]
fn transcript_eviction_shifts_app_level_review_anchors() {
    use crate::tui::config::constants::ui;

    let mut session = build_session();
    for index in 0..1_500 {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![text_segment(format!("prefix-{index}"))],
        });
    }
    add_compact_activity(&mut session, 49, "printf retained");
    let original_line = session.compact_activity_entries[0].line_index;
    assert_eq!(session.tool_output_blocks[0].anchor_line, Some(original_line));
    session
        .compact_activity_hit_regions
        .push(CompactActivityHitRegion { area: Rect::new(1, 1, 1, 1), review_anchor: 49 });

    let append_count = ui::TUI_TRANSCRIPT_MAX_MSGS + 1 - session.core.lines.len();
    for index in 0..append_count {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Agent,
            segments: vec![text_segment(format!("tail-{index}"))],
        });
    }

    let shifted_line = original_line - ui::TUI_TRANSCRIPT_EVICT_CHUNK;
    assert_eq!(session.compact_activity_entries[0].line_index, shifted_line);
    assert_eq!(session.tool_output_blocks[0].anchor_line, Some(shifted_line));
    assert!(session.compact_activity_hit_regions.is_empty());
    assert!(session.compact_activity_for_line(shifted_line).is_some());
}
#[test]
fn collapsing_pty_group_reanchors_only_group_members() {
    let mut session = build_session();
    add_compact_activity(&mut session, 44, "printf first");
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 99,
        lines: vec!["• Ran failed command".to_string(), "    failed".to_string()],
    });
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Pty,
        segments: vec![text_segment("• Ran printf second")],
    });
    session.handle_command(InlineCommand::CollapsePtyBlock(CompactActivityMetadata {
        group_id: 44,
        command_count: 2,
        command: None,
        hidden_line_count: 2,
        suffix: None,
        review_anchor: Some(44),
        review_anchors: vec![44],
    }));

    assert_eq!(session.tool_output_blocks[0].anchor_line, Some(0));
    assert_eq!(session.tool_output_blocks[1].anchor_line, None);
    assert_eq!(session.compact_activity_entries.len(), 1);
    assert_eq!(session.compact_activity_entries[0].metadata.command_count, 2);
}
#[test]
fn transcript_review_title_mode_click_toggles_rendering() {
    let mut session = build_session();
    add_compact_activity(&mut session, 43, "printf mode");
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render review");

    let mode_column = (0..80)
        .find(|column| {
            session
                .tool_output_viewer_state()
                .is_some_and(|viewer| viewer.mode_control_contains(*column, 0))
        })
        .expect("rendered review title should expose a mode hit region");
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: mode_column,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );

    assert!(
        session
            .tool_output_viewer_state()
            .is_some_and(|viewer| viewer.render_mode() == tool_output_viewer::TranscriptRenderMode::Raw)
    );
}
#[test]
fn review_evidence_alt_click_uses_wrapped_rows_without_stealing_plain_selection() {
    for width in [24, 100] {
        let mut session = build_session();
        let target = format!("vtcode-evidence:session:0:{}", "a".repeat(64));
        session.record_tool_output_block(81, vec![format!("Inspect storage [evidence]({target})")]);
        session.open_tool_output_viewer(width, 24, Some(81));
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, 24)).expect("terminal");
        terminal.draw(|frame| session.render(frame)).expect("render");
        let row = (0..24)
            .find(|row| {
                session
                    .tool_output_viewer_state()
                    .is_some_and(|viewer| viewer.evidence_at(1, *row).is_some())
            })
            .expect("evidence row");
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut mouse = MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 1,
            row,
            modifiers: KeyModifiers::NONE,
        };
        session.handle_event(CrosstermEvent::Mouse(mouse), &sender, None);
        assert!(receiver.try_recv().is_err());
        mouse.kind = MouseEventKind::Up(crossterm::event::MouseButton::Left);
        session.handle_event(CrosstermEvent::Mouse(mouse), &sender, None);
        mouse.kind = MouseEventKind::Down(crossterm::event::MouseButton::Left);
        mouse.modifiers = KeyModifiers::ALT;
        session.handle_event(CrosstermEvent::Mouse(mouse), &sender, None);
        assert!(matches!(receiver.try_recv(), Ok(InlineEvent::OpenUrl(url)) if url == target));
        assert!(session.tool_output_viewer_state().is_some());
        session.tool_output_viewer_state_mut().unwrap().toggle_render_mode();
        terminal.draw(|frame| session.render(frame)).expect("raw render");
        assert_eq!(session.tool_output_viewer_state().unwrap().evidence_at(1, row), Some(target.as_str()));
        assert!(session.tool_output_viewer_state().unwrap().evidence_at(0, 0).is_none());
    }
}
#[test]
fn transcript_review_header_close_button_closes_on_mouse_click_and_shows_guide() {
    let mut session = build_session();
    add_compact_activity(&mut session, 46, "printf close");
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
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
    let rendered = rendered_buffer_text(&terminal);
    assert!(rendered.contains("[close]"));
    assert!(rendered.contains("Ctrl+T open/close"));
    assert!(rendered.contains("R rich/raw"));
    assert!(rendered.contains("Esc close"));

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

    assert!(session.tool_output_viewer_state().is_none());
}
#[test]
fn alt_t_still_toggles_tool_display_mode() {
    let mut session = build_session();

    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT)),
        Some(InlineEvent::ToggleToolDisplayMode)
    ));
}
