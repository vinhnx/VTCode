use super::super::*;
use super::helpers::*;
use unicode_width::UnicodeWidthStr;

const AREA: Rect = Rect::new(4, 2, 48, 8);

fn push(session: &mut Session, kind: InlineMessageKind, text: &str) {
    session.push_line(kind, vec![make_segment(text)]);
}

fn history() -> Session {
    let mut session = Session::new(InlineTheme::default(), None, 16);
    push(&mut session, InlineMessageKind::Info, "session start");
    push(&mut session, InlineMessageKind::User, "same prompt");
    push(&mut session, InlineMessageKind::User, "second line");
    for i in 0..16 {
        push(&mut session, InlineMessageKind::Agent, &format!("first answer {i}"));
    }
    push(&mut session, InlineMessageKind::User, "same prompt");
    for i in 0..16 {
        push(&mut session, InlineMessageKind::Pty, &format!("tool output {i}"));
    }
    session
}

fn render(session: &mut Session, area: Rect) -> Buffer {
    let mut buffer = Buffer::empty(area);
    TranscriptWidget::new(session).render(area, &mut buffer);
    buffer
}

fn browse_to(session: &mut Session, index: usize, row_in_message: usize) -> usize {
    render(session, AREA);
    let row = session.transcript_message_row_range(AREA.width, index).unwrap().0 + row_in_message;
    session.ensure_scroll_metrics();
    session
        .scroll_manager
        .set_offset(session.scroll_manager.max_offset().saturating_sub(row));
    session.user_scrolled = true;
    session.invalidate_transcript_viewport();
    row
}

fn header_text(buffer: &Buffer, area: Rect) -> String {
    let mut text = String::new();
    let mut x = area.x;
    while x < area.right() {
        let symbol = buffer[(x, area.y)].symbol();
        text.push_str(symbol);
        x += UnicodeWidthStr::width(symbol).max(1) as u16;
    }
    text.trim_end().to_owned()
}

fn click() -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: AREA.x + 1,
        row: AREA.y,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn sticky_prompt_owns_multiline_partial_prompt_and_tool_output_by_index() {
    let mut session = history();
    for (visible_index, row_in_message, prompt_index, preview) in [
        (2, 0, 1, "same prompt second line"),
        (7, 0, 1, "same prompt second line"),
        (20, 1, 19, "same prompt"),
    ] {
        let row = browse_to(&mut session, visible_index, row_in_message);
        let buffer = render(&mut session, AREA);
        assert_eq!(header_text(&buffer, AREA), preview);
        assert_eq!(session.transcript_view_top, row);
        assert_eq!(session.transcript_area(), Some(Rect::new(AREA.x, AREA.y + 1, AREA.width, AREA.height - 1)));
        assert!(session.handle_sticky_prompt_click(click()));
        render(&mut session, AREA);
        let expected = session.transcript_message_row_range(AREA.width, prompt_index).unwrap().0;
        assert_eq!(session.transcript_view_top, expected, "duplicate prompt must navigate by identity");
        assert!(session.sticky_prompt_target.is_none());
        assert!(session.user_scrolled, "jump away from bottom must pause following");
    }
}

#[test]
fn sticky_prompt_click_is_consumed_by_both_mouse_event_paths() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut session = history();
    browse_to(&mut session, 7, 0);
    render(&mut session, AREA);
    session.mouse_selection.start_selection(AREA.x, AREA.y + 2);
    session.pending_link_open = Some("pending link".into());
    session.handle_event(CrosstermEvent::Mouse(click()), &tx, None);
    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            ..click()
        }),
        &tx,
        None,
    );
    render(&mut session, AREA);
    let expected_row = session.transcript_message_row_range(AREA.width, 1).unwrap().0;
    assert_eq!(session.transcript_view_top, expected_row);
    assert!(!session.mouse_selection.is_selecting && !session.mouse_selection.has_selection);
    assert_eq!(session.mouse_drag_target, MouseDragTarget::None);
    assert!(session.pending_link_open.is_none());
    assert!(rx.try_recv().is_err(), "header must not dispatch a link");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut app = AppSession::new(InlineTheme::default(), None, 16);
    app.core = history();
    app.core.fullscreen.interaction.mouse_capture = true;
    browse_to(&mut app.core, 24, 0);
    render(&mut app.core, AREA);
    app.handle_event(CrosstermEvent::Mouse(click()), &tx, None);
    app.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            ..click()
        }),
        &tx,
        None,
    );
    render(&mut app.core, AREA);
    let expected_row = app.core.transcript_message_row_range(AREA.width, 19).unwrap().0;
    assert_eq!(app.core.transcript_view_top, expected_row);
    assert!(!app.core.mouse_selection.is_selecting && !app.core.mouse_selection.has_selection);
    assert_eq!(app.core.mouse_drag_target, MouseDragTarget::None);
    assert!(rx.try_recv().is_err());
}

#[test]
fn sticky_prompt_hidden_for_original_row_no_owner_and_tiny_viewports() {
    let mut session = history();
    browse_to(&mut session, 1, 0);
    render(&mut session, AREA);
    assert!(session.sticky_prompt_target.is_none());
    browse_to(&mut session, 0, 0);
    render(&mut session, AREA);
    assert!(session.sticky_prompt_target.is_none());
    for height in 0..4 {
        browse_to(&mut session, 7, 0);
        render(&mut session, Rect::new(AREA.x, AREA.y, AREA.width, height));
        assert!(session.sticky_prompt_target.is_none(), "height {height}");
    }
    let mut orphan = Session::new(InlineTheme::default(), None, 16);
    for _ in 0..20 {
        push(&mut orphan, InlineMessageKind::Agent, "orphan answer");
    }
    render(&mut orphan, AREA);
    assert!(orphan.sticky_prompt_target.is_none());
}

#[test]
fn sticky_prompt_unicode_preview_collapses_whitespace_and_uses_user_prefix() {
    let mut session = Session::new(InlineTheme::default(), None, 16);
    session.labels.user = Some("you: ".into());
    push(&mut session, InlineMessageKind::User, "  你好\t世界\n  keep going with more words");
    for _ in 0..20 {
        push(&mut session, InlineMessageKind::Agent, "answer");
    }
    let area = Rect::new(0, 0, 16, 8);
    let buffer = render(&mut session, area);
    let text = header_text(&buffer, area);
    assert!(text.starts_with("you: 你好 世界"), "{text:?}");
    assert!(text.ends_with('…'), "{text:?}");
    assert!(UnicodeWidthStr::width(text.as_str()) <= usize::from(area.width));
    assert!(!text.contains("  ") && !text.contains('\t') && !text.contains('\n'));
}

#[test]
fn sticky_prompt_streaming_resize_and_repeated_frames_preserve_reading_anchor() {
    let mut session = history();
    let row = browse_to(&mut session, 7, 0);
    let first = render(&mut session, AREA);
    let cached = Arc::clone(&session.visible_lines_cache.as_ref().unwrap().3);
    for _ in 0..4 {
        assert_eq!(render(&mut session, AREA), first);
        assert_eq!(session.transcript_view_top, row);
        assert!(Arc::ptr_eq(&cached, &session.visible_lines_cache.as_ref().unwrap().3));
    }
    session.append_inline(InlineMessageKind::Pty, make_segment(" streaming more"));
    assert!(session.sticky_prompt_target.is_none(), "mutation invalidates rendered target");
    render(&mut session, AREA);
    assert_eq!(session.transcript_view_top, row);
    let anchor = session.transcript_scroll_anchor().unwrap();
    render(&mut session, Rect::new(2, 4, 24, 12));
    assert_eq!(session.transcript_scroll_anchor().unwrap(), anchor);
    let after_resize = session.transcript_scroll_anchor().unwrap();
    render(&mut session, Rect::new(0, 0, 24, 3));
    assert!(session.sticky_prompt_target.is_none());
    assert_eq!(session.transcript_scroll_anchor().unwrap(), after_resize);
    render(&mut session, AREA);
    assert!(session.sticky_prompt_target.is_some());
}

#[test]
fn sticky_prompt_live_bottom_yields_to_visible_next_prompt_without_oscillation() {
    let mut session = history();
    // Find a viewport where reserving a row moves the live top onto a User
    // group head. Use actual row offsets/padding so this covers reflow geometry.
    let prompt_row = session.total_transcript_rows(48);
    push(&mut session, InlineMessageKind::User, "next prompt");
    for _ in 0..4 {
        push(&mut session, InlineMessageKind::Agent, "next answer");
    }
    let next_row = session.transcript_message_row_range(48, 36).unwrap().0;
    assert!(next_row >= prompt_row);
    let total = session.total_transcript_rows(48);
    let height = (4..=40)
        .find(|height| {
            let h = usize::from(*height);
            total + ui::effective_transcript_bottom_padding(h) - h == next_row.saturating_sub(1)
                && total + ui::effective_transcript_bottom_padding(h - 1) - (h - 1) == next_row
        })
        .expect("boundary viewport");
    let area = Rect::new(0, 0, 48, height);
    let first = render(&mut session, area);
    assert!(session.sticky_prompt_target.is_none());
    for _ in 0..4 {
        assert_eq!(render(&mut session, area), first);
        assert_eq!(session.scroll_manager.offset(), 0);
        assert!(session.sticky_prompt_target.is_none());
    }
    push(&mut session, InlineMessageKind::Agent, "live output advances");
    render(&mut session, area);
    // At the boundary the prompt itself is still visible; after enough live
    // rows arrive it becomes the owning sticky prompt.
    for _ in 0..4 {
        push(&mut session, InlineMessageKind::Agent, "more live output");
    }
    assert_eq!(header_text(&render(&mut session, area), area), "next prompt");
    assert_eq!(session.scroll_offset(), 0);
}

#[test]
fn sticky_prompt_invalidates_on_clear_replace_layout_and_eviction() {
    let mut session = history();
    render(&mut session, AREA);
    assert!(session.sticky_prompt_target.is_some());
    session.replace_last(16, InlineMessageKind::Agent, vec![vec![make_segment("replacement")]], None);
    assert!(!session.handle_sticky_prompt_click(click()));
    render(&mut session, AREA);
    session.set_transcript_area(None);
    assert!(!session.handle_sticky_prompt_click(click()));
    render(&mut session, AREA);
    session.clear_screen();
    assert!(!session.handle_sticky_prompt_click(click()));
    render(&mut session, AREA);
    assert!(session.sticky_prompt_target.is_none());

    // Evict the first part of a multiline User group, retaining only its
    // continuation. That continuation must never impersonate the lost prompt.
    for _ in 0..ui::TUI_TRANSCRIPT_MAX_MSGS {
        push(&mut session, InlineMessageKind::User, "continuation");
    }
    render(&mut session, AREA);
    assert!(session.sticky_prompt_target.is_some());
    push(&mut session, InlineMessageKind::Agent, "triggers eviction");
    assert!(session.evicted_message_count > 0);
    assert!(!session.handle_sticky_prompt_click(click()));
    assert!(session.leading_user_prompt_truncated);
    render(&mut session, AREA);
    assert!(session.sticky_prompt_target.is_none());
}

#[test]
fn sticky_prompt_overlay_and_modified_clicks_do_not_navigate() {
    let mut session = history();
    browse_to(&mut session, 7, 0);
    render(&mut session, AREA);
    let before = session.scroll_offset();
    assert!(!session.handle_sticky_prompt_click(MouseEvent { modifiers: KeyModifiers::CONTROL, ..click() }));
    assert!(!session.handle_sticky_prompt_click(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        ..click()
    }));
    session.show_modal("overlay".into(), vec!["owner".into()], None);
    assert!(!session.handle_sticky_prompt_click(click()));
    assert_eq!(session.scroll_offset(), before);
}

#[test]
fn sticky_prompt_body_links_and_drag_coordinates_start_below_header() {
    let mut session = history();
    // Use a detected URL as a body coordinate probe.
    push(&mut session, InlineMessageKind::Agent, "https://example.com/sticky");
    for _ in 0..12 {
        push(&mut session, InlineMessageKind::Agent, "trailing");
    }
    browse_to(&mut session, 36, 0);
    let buffer = render(&mut session, AREA);
    let body = session.transcript_area().unwrap();
    let url_row = (body.y..body.bottom())
        .find(|row| buffer[(body.x, *row)].symbol() == "h")
        .unwrap();
    assert!(session.update_transcript_file_link_hover(body.x, url_row));
    session.update_transcript_file_link_hover(body.x, AREA.y);
    assert!(session.hovered_transcript_file_link.is_none(), "header is not a body link target");
    session.mouse_selection.start_selection(body.x, url_row);
    session.mouse_drag_target = MouseDragTarget::Transcript;
    session.update_drag_auto_scroll(body.x, AREA.y);
    assert_eq!(session.drag_auto_scroll.unwrap().row, body.y);
    session.mouse_selection.finish_selection(body.x + 5, url_row);
    assert_eq!(session.mouse_selection.extract_text(&buffer, body), "https");
}

#[test]
fn sticky_prompt_composer_remains_pinned_when_header_appears() {
    let mut session = history();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| session.render(frame)).unwrap();
    let input_area = session.input_area().unwrap();
    assert!(session.sticky_prompt_target.is_some());
    session.scroll_to_top();
    terminal.draw(|frame| session.render(frame)).unwrap();
    assert!(session.sticky_prompt_target.is_none());
    assert_eq!(session.input_area(), Some(input_area));
}

#[test]
fn sticky_prompt_grapheme_preview_does_not_split_joined_emoji() {
    let mut session = Session::new(InlineTheme::default(), None, 16);
    // Unicode scalars here form one joined, two-column grapheme.
    push(&mut session, InlineMessageKind::User, "\u{1f469}\u{200d}\u{1f4bb} long prompt");
    for _ in 0..16 {
        push(&mut session, InlineMessageKind::Agent, "answer");
    }
    let area = Rect::new(0, 0, 3, 8);
    let buffer = render(&mut session, area);
    assert_eq!(header_text(&buffer, area), "\u{1f469}\u{200d}\u{1f4bb}…");
}

#[test]
fn sticky_prompt_queue_overlay_and_selection_exclude_header() {
    let mut session = history();
    session.push_queued_input("queued draft".into());
    let before = render(&mut session, AREA);
    assert_eq!(header_text(&before, AREA), "same prompt");
    let body = session.transcript_area().unwrap();
    let body_text = (body.y..body.bottom())
        .map(|row| header_text(&before, Rect::new(body.x, row, body.width, 1)))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(body_text.contains("queued draft"));
    session.mouse_selection.start_selection(body.x, body.y + 1);
    session.mouse_selection.update_selection(body.x + 5, AREA.y);
    let mut highlighted = before.clone();
    session.mouse_selection.apply_highlight(&mut highlighted, body);
    assert_eq!(header_text(&highlighted, AREA), header_text(&before, AREA));
    for x in AREA.x..AREA.right() {
        assert_eq!(highlighted[(x, AREA.y)], before[(x, AREA.y)], "header must not be highlighted");
    }
    assert!(!session.mouse_selection.extract_text(&before, body).contains("same prompt"));
}

#[test]
fn sticky_prompt_preserves_active_and_completed_modal_selection_in_both_render_paths() {
    for app_path in [false, true] {
        for completed in [false, true] {
            let mut session = AppSession::new(InlineTheme::default(), None, 40);
            session.core = history();
            session.core.fullscreen.interaction.mouse_capture = true;
            session.core.fullscreen.interaction.copy_on_select = false;
            session.handle_command(app_types::InlineCommand::ShowTransient {
                request: Box::new(app_types::TransientRequest::Modal(app_types::ModalOverlayRequest {
                    title: "selection owner".into(),
                    lines: vec!["selected overlay text".into()],
                    secure_prompt: None,
                    is_help_modal: false,
                })),
            });
            let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
            let draw = |session: &mut AppSession, terminal: &mut Terminal<TestBackend>| {
                terminal
                    .draw(|frame| {
                        if app_path {
                            session.render(frame);
                        } else {
                            session.core.render(frame);
                        }
                    })
                    .unwrap();
            };
            draw(&mut session, &mut terminal);
            assert!(session.core.sticky_prompt_target.is_some());
            let buffer = terminal.backend().buffer();
            let (column, row) = (0..24)
                .find_map(|row| {
                    let text = header_text(buffer, Rect::new(0, row, 80, 1));
                    text.find("selected overlay text").map(|column| (column as u16, row))
                })
                .expect("rendered modal text");
            let (core_tx, _) = mpsc::unbounded_channel();
            let (app_tx, _) = mpsc::unbounded_channel();
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Drag(MouseButton::Left),
            ] {
                let event = CrosstermEvent::Mouse(MouseEvent {
                    kind,
                    column: if matches!(kind, MouseEventKind::Drag(_)) {
                        column + 8
                    } else {
                        column
                    },
                    row,
                    modifiers: KeyModifiers::NONE,
                });
                if app_path {
                    session.handle_event(event, &app_tx, None);
                } else {
                    session.core.handle_event(event, &core_tx, None);
                }
            }
            assert_eq!(session.core.mouse_drag_target, MouseDragTarget::ModalText);
            if completed {
                let event = CrosstermEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: column + 8,
                    row,
                    modifiers: KeyModifiers::NONE,
                });
                if app_path {
                    session.handle_event(event, &app_tx, None);
                } else {
                    session.core.handle_event(event, &core_tx, None);
                }
                assert_eq!(session.core.mouse_drag_target, MouseDragTarget::None);
            }
            let selection_text = |session: &AppSession, terminal: &Terminal<TestBackend>| {
                session
                    .core
                    .mouse_selection
                    .extract_text(terminal.backend().buffer(), Rect::new(0, 0, 80, 40))
            };
            assert_eq!(selection_text(&session, &terminal), "selected");
            let highlighted_before = {
                draw(&mut session, &mut terminal);
                terminal.backend().buffer()[(column, row)].clone()
            };
            let offset_before = session.core.scroll_manager.offset();
            let wheel = CrosstermEvent::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 1,
                row: session.core.transcript_area().unwrap().y + 2,
                modifiers: KeyModifiers::NONE,
            });
            if app_path {
                session.handle_event(wheel, &app_tx, None);
            } else {
                session.core.handle_event(wheel, &core_tx, None);
            }
            assert!(session.core.scroll_manager.offset() > offset_before, "background transcript scrolls");
            draw(&mut session, &mut terminal);
            assert_eq!(selection_text(&session, &terminal), "selected", "background wheel preserves overlay selection");
            assert_eq!(terminal.backend().buffer()[(column, row)], highlighted_before);
            session.core.replace_last(
                session.core.lines.len(),
                InlineMessageKind::User,
                vec![vec![make_segment("short prompt")]],
                None,
            );
            draw(&mut session, &mut terminal);
            assert!(session.core.sticky_prompt_target.is_none(), "header disappeared");
            assert_eq!(
                selection_text(&session, &terminal),
                "selected",
                "app={app_path}, completed={completed}, disappearing header"
            );
            assert_eq!(terminal.backend().buffer()[(column, row)], highlighted_before, "highlight stays on modal text");
            for _ in 0..24 {
                push(&mut session.core, InlineMessageKind::Agent, "background output");
            }
            draw(&mut session, &mut terminal);
            assert!(session.core.sticky_prompt_target.is_some(), "header appeared");
            assert_eq!(
                selection_text(&session, &terminal),
                "selected",
                "app={app_path}, completed={completed}, appearing header"
            );
            assert_eq!(terminal.backend().buffer()[(column, row)], highlighted_before, "highlight stays on modal text");
        }
    }
}

#[test]
fn sticky_prompt_still_moves_active_and_completed_transcript_selections_with_body() {
    for completed in [false, true] {
        let mut session = history();
        browse_to(&mut session, 7, 0);
        let before = render(&mut session, AREA);
        let body = session.transcript_area().unwrap();
        let (column, row) = (body.x, body.y + 1);
        let (tx, _) = mpsc::unbounded_channel();
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
        ] {
            session.handle_event(
                CrosstermEvent::Mouse(MouseEvent {
                    kind,
                    column: if matches!(kind, MouseEventKind::Drag(_)) {
                        column + 5
                    } else {
                        column
                    },
                    row,
                    modifiers: KeyModifiers::NONE,
                }),
                &tx,
                None,
            );
        }
        if completed {
            session.handle_event(
                CrosstermEvent::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: column + 5,
                    row,
                    modifiers: KeyModifiers::NONE,
                }),
                &tx,
                None,
            );
            assert_eq!(session.mouse_drag_target, MouseDragTarget::None);
        }
        assert_eq!(session.mouse_selection.extract_text(&before, body), "first");
        let short_area = Rect::new(AREA.x, AREA.y, AREA.width, 3);
        let without_header = render(&mut session, short_area);
        assert!(session.sticky_prompt_target.is_none());
        assert_eq!(
            session.mouse_selection.extract_text(&without_header, short_area),
            "first",
            "disappearing header, completed={completed}"
        );
        let with_header = render(&mut session, AREA);
        assert!(session.sticky_prompt_target.is_some());
        assert_eq!(
            session
                .mouse_selection
                .extract_text(&with_header, session.transcript_area().unwrap()),
            "first",
            "appearing header, completed={completed}"
        );
    }
}
