use super::*;

#[test]
fn progress_only_transcript_excludes_drag_and_completed_selection() {
    for completed in [false, true] {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::SavingCheckpoint,
        }));
        let area = Rect::new(0, 2, 80, 1);
        session.mouse_selection.start_selection(0, 2);
        session.mouse_selection.update_selection(20, 2);
        if completed {
            session.mouse_selection.finish_selection(20, 2);
        }
        let mut terminal = Terminal::new(TestBackend::new(80, 4)).unwrap();
        terminal
            .draw(|frame| {
                TranscriptWidget::new(&mut session).render(area, frame.buffer_mut());
                assert!(session.transcript_area().is_none());
                frame.buffer_mut().set_style(
                    area,
                    ratatui::style::Style::new()
                        .fg(ratatui::style::Color::White)
                        .bg(ratatui::style::Color::Black),
                );
                let before = frame.buffer_mut().clone();
                let viewport = frame.area();
                session.finalize_mouse_selection(frame, viewport);
                assert_eq!(*frame.buffer_mut(), before, "progress must not be highlighted without a transcript body");
            })
            .unwrap();

        // Overlay selection keeps its own geometry even when the body is absent.
        session.mouse_selection.start_overlay_selection(0, 2);
        session.mouse_selection.finish_selection(7, 2);
        terminal
            .draw(|frame| {
                Paragraph::new("overlay text")
                    .style(
                        ratatui::style::Style::new()
                            .fg(ratatui::style::Color::White)
                            .bg(ratatui::style::Color::Black),
                    )
                    .render(area, frame.buffer_mut());
                let before = frame.buffer_mut()[(0, 2)].clone();
                let viewport = frame.area();
                session.finalize_mouse_selection(frame, viewport);
                assert_ne!(frame.buffer_mut()[(0, 2)], before);
                assert_eq!(session.mouse_selection.extract_text(frame.buffer_mut(), viewport), "overlay");
            })
            .unwrap();
    }
}
#[test]
fn persistent_footer_coordinates_survive_phases_elapsed_time_and_runtime_status() {
    for width in [120, 48, 24, 12, 4] {
        for context in [Some("topic/stable*"), Some("Running custom dashboard"), None] {
            let mut session = Session::new(InlineTheme::default(), None, 20);
            session.handle_command(InlineCommand::SetPrimaryAgent { name: Some("build".to_owned()), color: None });
            session.handle_command(InlineCommand::SetConfiguredInputStatus {
                left: context.map(str::to_owned),
                right: Some("10:30".to_owned()),
            });
            let operation = ProgressOperation::start();
            session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
                operation,
                phase: ProgressPhase::PreparingContext,
            }));
            let mut persistent = None;
            for phase in [
                ProgressPhase::PreparingContext,
                ProgressPhase::SavingCheckpoint,
                ProgressPhase::WaitingForModel,
                ProgressPhase::ReceivingResponse,
                ProgressPhase::WaitingForApproval,
            ] {
                session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Phase { operation, phase }));
                session.handle_command(InlineCommand::SetInputStatus {
                    left: Some("Running tool: apply_patch".to_owned()),
                    right: None,
                });
                for elapsed in [0, 9999] {
                    session.progress.elapsed_secs = elapsed;
                    let line = session.render_input_status_line(width).unwrap();
                    let mut buffer = Buffer::empty(Rect::new(0, 0, width, 1));
                    Paragraph::new(line).render(buffer.area, &mut buffer);
                    let cells = buffer.content.iter().map(|cell| cell.symbol().to_owned()).collect::<Vec<_>>();
                    // Independently locate persistent text from rendered cells,
                    // rather than reusing the footer's allocation calculations.
                    let text = cells.concat();
                    let locate =
                        |needle: &str| cells.windows(needle.chars().count()).position(|slice| slice.concat() == needle);
                    let positions = (locate("Build"), locate("10:30"), context.and_then(locate));
                    if let Some(expected) = persistent {
                        assert_eq!(positions, expected, "{width}: {text}");
                    }
                    persistent = Some(positions);
                    assert!(!text.contains("apply_patch"));
                    if width >= 24 {
                        assert!(text.contains("Build") && text.contains("10:30"), "{text}");
                    }
                    if width >= 48
                        && let Some(context) = context
                    {
                        assert!(text.contains(context), "{text}");
                    }
                }
            }
            session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, 1));
            Paragraph::new(session.render_input_status_line(width).unwrap()).render(buffer.area, &mut buffer);
            let cells = buffer.content.iter().map(|cell| cell.symbol().to_owned()).collect::<Vec<_>>();
            let locate =
                |needle: &str| cells.windows(needle.chars().count()).position(|slice| slice.concat() == needle);
            let positions = (locate("Build"), locate("10:30"), context.and_then(locate));
            assert_eq!(Some(positions), persistent, "persistent slots survive completion: {}", cells.concat());
        }
    }
}
#[test]
fn zero_left_allocation_preserves_mode_in_full_session() {
    for width in [7, 8] {
        for context in [None, Some("topic/stable*")] {
            let mut session = Session::new(InlineTheme::default(), None, 20);
            session.handle_command(InlineCommand::SetPrimaryAgent { name: Some("build".to_owned()), color: None });
            session.handle_command(InlineCommand::SetConfiguredInputStatus {
                left: context.map(str::to_owned),
                right: None,
            });
            let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
            terminal.draw(|frame| session.render(frame)).unwrap();
            let rows = terminal
                .backend()
                .buffer()
                .content
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>();
            assert!(
                rows.iter()
                    .any(|row| row == &format!("{}• Build", " ".repeat(usize::from(width - 7)))),
                "{rows:?}"
            );
        }
    }
}
#[test]
fn loading_moves_background_off_bottom_line_onto_transcript_row() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.handle_command(InlineCommand::SetConfiguredInputStatus {
        left: Some("topic/stable*".to_owned()),
        right: Some("10:30".to_owned()),
    });
    session.set_background_activity_count(2);

    // Idle: bottom line carries the background copy and the header badge is short/static.
    let idle: String = session
        .render_input_status_line(80)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(idle.contains("Running 2 background tasks"), "{idle}");
    assert!(idle.contains("topic/stable*"), "{idle}");
    assert_eq!(session.background_header_badge_text().as_deref(), Some("• 2 bg"));
    assert!(session.header_meta_line().to_string().contains("• 2 bg"));

    // Loading: transcript owns the row, bottom line keeps only configured context.
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::WaitingForModel,
    }));
    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);
    session.render_progress(area, &mut buf);
    assert!(session.progress_row_visible());
    let row_text = rendered_text(&buf);
    assert!(row_text.contains("Waiting for model"), "{row_text}");
    assert!(row_text.contains("2 bg"), "transcript row must carry the live count: {row_text}");

    let loading: String = session
        .render_input_status_line(80)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(!loading.contains("background task"), "bottom line must not blink during loading: {loading}");
    assert!(!loading.contains("Ctrl+B"), "background hint must leave the bottom line during loading: {loading}");
    assert!(loading.contains("topic/stable*"), "configured context survives loading: {loading}");
    // No bottom-line click target while busy: keyboard entry points
    // (`Ctrl+B`, `Alt+S`, `/jobs`, empty-Enter) stay available and the header
    // carries discovery.
    let (loading_line, loading_hits) = session.render_input_status_line_with_hit(80).expect("loading status line");
    let loading_hit_text: String = loading_line.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(loading_hit_text.contains("topic/stable*"), "{loading_hit_text}");
    assert!(loading_hits.is_empty(), "no drawer click target while loading: {loading_hits:?}");

    // Completion restores the idle bottom-line copy without a state toggle.
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
    let restored: String = session
        .render_input_status_line(80)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(restored.contains("Running 2 background tasks"), "{restored}");
}
#[test]
fn busy_bottom_line_hides_pty_hint_while_header_keeps_discovery() {
    use std::sync::atomic::AtomicUsize;

    let mut session = Session::new(InlineTheme::default(), None, 20);
    let pty = Arc::new(AtomicUsize::new(1));
    session.active_pty_sessions = Some(Arc::clone(&pty));
    session.handle_command(InlineCommand::SetConfiguredInputStatus {
        left: Some("topic/stable*".to_owned()),
        right: Some("10:30".to_owned()),
    });
    session.set_background_activity_count(1);

    // Foreground command running outside loading: bottom line keeps only
    // configured context, never the `· Ctrl+B background` flicker after the
    // branch status. The header carries discovery instead.
    let running: String = session
        .render_input_status_line(80)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(running.contains("topic/stable*"), "{running}");
    assert!(!running.contains("Ctrl+B"), "bottom line must not flicker PTY hint: {running}");
    assert!(
        running.contains("Running 1 background task"),
        "stable background count survives PTY busy: {running}"
    );
    let header_text: String = session
        .header_suggestions_line()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(header_text.contains("Ctrl+B"), "header must keep discovery while busy: {header_text}");

    // Loading as well: still clean, with no click targets to reflow.
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::RunningTools,
    }));
    let loading: String = session
        .render_input_status_line(80)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(loading.contains("topic/stable*"), "{loading}");
    assert!(!loading.contains("Ctrl+B"), "loading bottom line must not flicker PTY hint: {loading}");
    assert!(!loading.contains("background task"), "live count stays off the bottom line: {loading}");
    let (pty_line, pty_hits) = session.render_input_status_line_with_hit(80).expect("pty status line");
    let pty_text: String = pty_line.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(pty_text.contains("topic/stable*"), "{pty_text}");
    assert!(pty_hits.is_empty(), "no bottom-line click target while busy: {pty_hits:?}");

    // The transcript row still owns the phase plus the inline PTY hint.
    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);
    session.render_progress(area, &mut buf);
    let row_text = rendered_text(&buf);
    assert!(row_text.contains("Running tools"), "progress row must own the phase: {row_text}");
    assert!(row_text.contains("Ctrl+B"), "progress row must surface the inline PTY hint: {row_text}");
    assert!(row_text.contains("background"), "progress row must explain background: {row_text}");

    // Narrow rows keep the no-overflow contract with no hint competing for
    // the fallback budget.
    let narrow = session.render_input_status_line(24).expect("narrow status line");
    assert!(narrow.width() <= 24, "narrow bottom line must not overflow");

    // Command and loading both clear: header discovery drops with the PTY,
    // while the idle background count returns to the bottom line.
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
    pty.store(0, std::sync::atomic::Ordering::Relaxed);
    let idle: String = session
        .render_input_status_line(80)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(idle.contains("Running 1 background task"), "{idle}");
    let header_text: String = session
        .header_suggestions_line()
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(!header_text.contains("Ctrl+B"), "header discovery must drop with the PTY: {header_text}");
}
#[test]
fn progress_row_pty_hint_follows_pty_liveness_and_rebound_shortcut() {
    use std::sync::atomic::AtomicUsize;

    use crate::tui::core_tui::session::action::BindingStore;

    fn row_text(session: &mut Session, width: u16) -> String {
        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        session.render_progress(area, &mut buf);
        rendered_text(&buf)
    }

    // Loading without a foreground command: phase only, no background hint.
    let mut session = Session::new(InlineTheme::default(), None, 20);
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::RunningTools,
    }));
    let row = row_text(&mut session, 80);
    assert!(row.contains("Running tools"), "{row}");
    assert!(!row.contains("Ctrl+B"), "no PTY, no background hint: {row}");
    assert!(!row.contains("background"), "no PTY, no background hint: {row}");
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));

    // Foreground command starts under a new loading operation: the static
    // inline suffix appears alongside the phase.
    let pty = Arc::new(AtomicUsize::new(1));
    session.active_pty_sessions = Some(Arc::clone(&pty));
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::RunningTools,
    }));
    let row = row_text(&mut session, 80);
    assert!(row.contains("Running tools"), "{row}");
    assert!(row.contains("Ctrl+B"), "running PTY must surface the inline hint: {row}");
    assert!(row.contains("background"), "running PTY must explain background: {row}");

    // Rebound shortcut flows through instead of a hardcoded `Ctrl+B`.
    let mut overlay = hashbrown::HashMap::new();
    overlay.insert("background_operation".to_owned(), vec!["ctrl+x".to_owned()]);
    session.set_bindings(BindingStore::new(overlay));
    let row = row_text(&mut session, 80);
    assert!(row.contains("Ctrl+X"), "row must use the rebound shortcut: {row}");
    assert!(!row.contains("Ctrl+B"), "row must not keep the stale shortcut: {row}");

    // Narrow rows keep the phase label even with the suffix competing.
    let narrow = row_text(&mut session, 20);
    assert!(narrow.contains("Running"), "phase must survive narrow truncation: {narrow}");

    // PTY ends while loading continues: suffix drops, phase stays.
    pty.store(0, std::sync::atomic::Ordering::Relaxed);
    let row = row_text(&mut session, 80);
    assert!(row.contains("Running tools"), "{row}");
    assert!(!row.contains("background"), "hint must drop with the PTY: {row}");
}
#[test]
fn loading_fallback_without_transcript_row_keeps_header_only_background() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.handle_command(InlineCommand::SetConfiguredInputStatus {
        left: Some("topic/stable*".to_owned()),
        right: None,
    });
    session.set_background_activity_count(3);

    // Progress accepted but no transcript row painted yet (zero-height body,
    // overlay cover, progress-only allocation): the bounded footer fallback
    // shows the phase, never the background copy.
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::WaitingForModel,
    }));
    assert!(!session.progress_row_visible());
    let fallback: String = session
        .render_input_status_line(80)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(fallback.contains("Waiting for model"), "{fallback}");
    assert!(fallback.contains("topic/stable*"), "{fallback}");
    assert!(!fallback.contains("background task"), "{fallback}");
    assert!(session.header_meta_line().to_string().contains("• 3 bg"));
}
#[test]
fn narrow_transcript_row_prioritizes_phase_with_header_as_guaranteed_home() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.set_background_activity_count(2);
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::WaitingForModel,
    }));
    let area = Rect::new(0, 0, 20, 1);
    let mut buf = Buffer::empty(area);
    session.render_progress(area, &mut buf);
    let row_text = rendered_text(&buf);
    // Phase label is primary and must survive truncation; the `bg` suffix is
    // best-effort and the header badge is the guaranteed home.
    assert!(row_text.contains("Waiting"), "{row_text}");
    assert!(session.header_meta_line().to_string().contains("• 2 bg"));
}
#[test]
fn idle_background_survives_running_turn_status_without_blinking() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.set_background_activity_count(1);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: edit_file".to_owned()),
        right: None,
    });
    assert!(session.is_running_activity(), "fixture must look like an active turn");

    // No progress row owns loading here, so the bottom line keeps both the
    // turn status and the background count stably instead of hiding one.
    let text: String = session
        .render_input_status_line(100)
        .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
        .unwrap_or_default();
    assert!(text.contains("Running tool: edit_file"), "{text}");
    assert!(text.contains("Running 1 background task"), "{text}");
}
