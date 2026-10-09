use super::*;
use crate::tui::core_tui::types::InlineSegment;
use crate::tui::core_tui::widgets::TranscriptWidget;
use ratatui::{Terminal, backend::TestBackend};

fn rendered_text(buf: &Buffer) -> String {
    buf.content.iter().map(|cell| cell.symbol()).collect()
}

#[derive(Clone)]
struct DiagnosticWriter(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for DiagnosticWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn progress_feedback_metric_observes_footer_once_after_visible_paint() {
    let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
    let writer = DiagnosticWriter(Arc::clone(&captured));
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let operation = ProgressOperation::start();
    tracing::subscriber::with_default(subscriber, || {
        let mut session = Session::new(InlineTheme::default(), None, 20);
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation,
            phase: ProgressPhase::WaitingForModel,
        }));
        session.show_copy_notification(7);
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        for suppressed in [true, false, false] {
            if !suppressed {
                session.copy_notification_until = None;
            }
            terminal
                .draw(|frame| {
                    let layout = session.prepare_frame_layout(frame, 0).unwrap();
                    session.render_base_frame(frame, &layout, Rect::ZERO);
                    session.render_input(frame, layout.input_area);
                    session.observe_progress_feedback();
                })
                .unwrap();
            assert_eq!(session.progress.active.unwrap().feedback_observed, !suppressed);
            assert_eq!(rendered_text(terminal.backend().buffer()).contains("Waiting for model"), !suppressed);
        }
    });
    let diagnostics = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert_eq!(diagnostics.matches("accepted_to_feedback_ms=").count(), 1, "{diagnostics}");
    assert!(diagnostics.contains(&format!("operation_id={}", operation.id())));
}

#[test]
fn progress_feedback_waits_until_fullscreen_viewer_is_closed() {
    use crate::tui::core_tui::app::{
        session::AppSession,
        types::{InlineCommand as AppCommand, LocalAgentsTransientRequest, TransientRequest},
    };

    for width in [120, 48] {
        let mut session = AppSession::new(InlineTheme::default(), None, 24);
        session.core.set_fullscreen_active(true);
        session.handle_command(AppCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::WaitingForModel,
        }));
        session.show_transient(TransientRequest::LocalAgents(LocalAgentsTransientRequest { visible: Some(true) }));
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal.draw(|frame| session.render(frame)).unwrap();
        assert!(!rendered_text(terminal.backend().buffer()).contains("Waiting for model"));
        assert!(!session.core.progress.active.unwrap().feedback_observed);
        session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        terminal.draw(|frame| session.render(frame)).unwrap();
        assert_eq!(rendered_text(terminal.backend().buffer()).matches("Waiting for model").count(), 1);
        assert!(session.core.progress.active.unwrap().feedback_observed);
    }
}

#[test]
fn progress_feedback_does_not_count_an_ellipsis_only_footer() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation: ProgressOperation::start(),
        phase: ProgressPhase::SavingCheckpoint,
    }));
    let mut terminal = Terminal::new(TestBackend::new(1, 20)).unwrap();
    terminal
        .draw(|frame| {
            let layout = session.prepare_frame_layout(frame, 0).unwrap();
            session.render_base_frame(frame, &layout, Rect::ZERO);
            session.render_input(frame, layout.input_area);
            session.observe_progress_feedback();
        })
        .unwrap();
    assert!(!session.progress.active.unwrap().feedback_observed);
    assert!(!rendered_text(terminal.backend().buffer()).contains('S'));
}

#[test]
fn progress_feedback_occlusion_uses_painted_text_columns() {
    for overlay_x in [0, 60] {
        let mut session = Session::new(InlineTheme::default(), None, 20);
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::WaitingForModel,
        }));
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| {
                session.begin_frame(frame).unwrap();
                session.render_progress(Rect::new(0, 0, 80, 1), frame.buffer_mut());
                let overlay = Rect::new(overlay_x, 0, 20, 1);
                Clear.render(overlay, frame.buffer_mut());
                session.occlude_progress_feedback(overlay);
                session.observe_progress_feedback();
            })
            .unwrap();
        let visible = rendered_text(terminal.backend().buffer()).contains("Waiting for model");
        assert_eq!(visible, overlay_x == 60);
        assert_eq!(session.progress.active.unwrap().feedback_observed, visible);
    }
}

#[test]
fn progress_has_one_loading_label_in_full_sessions_with_and_without_logs() {
    use crate::tui::core_tui::app::{session::AppSession, types::InlineCommand as AppCommand};

    for fullscreen in [false, true] {
        for width in [120, 48] {
            for show_logs in [false, true] {
                let mut session = AppSession::new(InlineTheme::default(), None, 24);
                session.core.set_fullscreen_active(fullscreen);
                session.core.show_logs = show_logs;
                session.core.log_lines.push_back(Arc::new(Text::from("fixture log")));
                session.core.thinking_spinner.start();
                session
                    .core
                    .handle_command(InlineCommand::SetActivityState(ActivityState::Building));
                let operation = ProgressOperation::start();
                session.handle_command(AppCommand::UpdateProgress(ProgressUpdate::Begin {
                    operation,
                    phase: ProgressPhase::WaitingForModel,
                }));
                let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
                terminal.draw(|frame| session.render(frame)).unwrap();
                let text = rendered_text(terminal.backend().buffer());
                assert_eq!(text.matches("Waiting for model").count(), 1, "{text}");
                assert!(!text.contains("Thinking"), "legacy spinner must not repeat progress");
                assert!(!text.contains("Building..."), "legacy stage must not repeat progress");
                assert!(session.core.progress_row_visible());
                if show_logs && width == 120 {
                    assert!(text.contains("fixture log"));
                }
                assert!(session.core.transcript_export_text().is_empty());
                session.handle_command(AppCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
                terminal.draw(|frame| session.render(frame)).unwrap();
                let text = rendered_text(terminal.backend().buffer());
                assert!(!text.contains("Waiting for model"));
                assert!(text.contains("Building..."), "normal footer status returns after progress");
            }
        }
    }
}

#[test]
fn transcript_progress_keeps_git_branch_and_status_in_the_footer() {
    use crate::tui::core_tui::app::{session::AppSession, types::InlineCommand as AppCommand};

    for fullscreen in [false, true] {
        for width in [120, 48] {
            let mut session = AppSession::new(InlineTheme::default(), None, 24);
            session.core.set_fullscreen_active(fullscreen);
            session.core.thinking_spinner.start();
            session
                .core
                .handle_command(InlineCommand::SetActivityState(ActivityState::Building));
            session.handle_command(AppCommand::UpdateProgress(ProgressUpdate::Begin {
                operation: ProgressOperation::start(),
                phase: ProgressPhase::WaitingForModel,
            }));
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            for (git, expected, indicator, indicator_color) in [
                ("git: feature/login | ✓", "feature/login ✓", "✓", ratatui::style::Color::Green),
                ("git: fix/footer | *", "fix/footer *", "*", ratatui::style::Color::Red),
                ("git: fix/blocked | *", "fix/blocked *", "*", ratatui::style::Color::Red),
            ] {
                session.core.handle_command(InlineCommand::SetConfiguredInputStatus {
                    left: Some(git.to_owned()),
                    right: Some("10:30".to_owned()),
                });
                terminal.draw(|frame| session.render(frame)).unwrap();
                let text = rendered_text(terminal.backend().buffer());
                assert_eq!(text.matches("Waiting for model").count(), 1, "{text}");
                assert!(text.contains(expected), "{text}");
                assert!(text.contains("10:30"), "{text}");
                assert!(!text.contains("Thinking") && !text.contains("Building..."));
                let footer = session.core.render_input_status_line(width).unwrap();
                let footer_text: String = footer.spans.iter().map(|span| span.content.as_ref()).collect();
                assert!(footer_text.contains(expected), "{footer_text}");
                assert!(!footer_text.contains("Waiting for model") && !footer_text.contains("git:"));
                let indicator_span = footer.spans.iter().find(|span| span.content == indicator).unwrap();
                assert_eq!(indicator_span.style.fg, Some(indicator_color));
                session
                    .core
                    .handle_command(InlineCommand::SetActivityState(ActivityState::StartingBuild));
                terminal.draw(|frame| session.render(frame)).unwrap();
                let text = rendered_text(terminal.backend().buffer());
                assert!(text.contains(expected), "Git status survives activity changes: {text}");
            }
        }
    }
}

#[test]
fn git_branches_with_activity_words_remain_static_footer_context() {
    for status in ["git: fix/blocked | *", "fix/blocked*", "fix/blocked✓"] {
        let mut session = Session::new(InlineTheme::default(), None, 20);
        session.appearance.hide_header = false;
        session.handle_command(InlineCommand::SetConfiguredInputStatus { left: Some(status.to_owned()), right: None });
        assert!(!session.is_running_activity(), "Git status must not claim input authority: {status}");
        let header: String = session
            .header_lines()
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect();
        assert!(!header.contains("Blocked"), "Git status must not select a blocked badge: {header}");
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::WaitingForModel,
        }));
        session.handle_command(InlineCommand::SetActivityState(ActivityState::Building));
        let area = Rect::new(0, 0, 80, 1);
        TranscriptWidget::new(&mut session).render(area, &mut Buffer::empty(area));
        let footer = session.render_input_status_line(80).unwrap();
        let text: String = footer.spans.iter().map(|span| span.content.as_ref()).collect();
        assert!(text.contains("fix/"), "Git context disappeared: {status}: {text}");
        assert!(!text.contains("Waiting for model") && !text.contains("Building..."));
        assert!(!status_requires_shimmer(status), "Git status must remain static: {status}");
    }
    // A command ending in a wildcard remains an activity label.
    assert!(status_requires_shimmer("Running tool: grep blocked*"));
}

#[test]
fn configured_custom_status_and_hidden_mode_survive_progress_and_runtime_updates() {
    use crate::tui::core_tui::app::{session::AppSession, types::InlineCommand as AppCommand};

    for fullscreen in [false, true] {
        for width in [120, 48] {
            let mut session = AppSession::new(InlineTheme::default(), None, 24);
            session.core.set_fullscreen_active(fullscreen);
            session.handle_command(AppCommand::UpdateProgress(ProgressUpdate::Begin {
                operation: ProgressOperation::start(),
                phase: ProgressPhase::WaitingForModel,
            }));
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            for configured in [Some("Running custom dashboard"), None, Some("custom reloaded")] {
                session.handle_command(AppCommand::SetConfiguredInputStatus {
                    left: configured.map(str::to_owned),
                    right: None,
                });
                session.handle_command(AppCommand::SetActivityState(ActivityState::Building));
                session.handle_command(AppCommand::SetInputStatus {
                    left: Some("Running tool: edit_file".to_owned()),
                    right: None,
                });
                terminal.draw(|frame| session.render(frame)).unwrap();
                let rendered = rendered_text(terminal.backend().buffer());
                assert_eq!(rendered.matches("Waiting for model").count(), 1, "{rendered}");
                let footer: String = session
                    .core
                    .render_input_status_line(width)
                    .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
                    .unwrap_or_default();
                assert_eq!(
                    rendered.contains("Running custom dashboard"),
                    configured == Some("Running custom dashboard"),
                    "{rendered}"
                );
                assert_eq!(rendered.contains("custom reloaded"), configured == Some("custom reloaded"), "{rendered}");
                assert_eq!(
                    footer.contains("Running custom dashboard"),
                    configured == Some("Running custom dashboard"),
                    "{footer}"
                );
                assert_eq!(footer.contains("custom reloaded"), configured == Some("custom reloaded"), "{footer}");
                assert!(!footer.contains("edit_file") && !footer.contains("Building..."), "{footer}");
                assert!(!footer.contains("Waiting for model"));
            }
        }
    }
}

#[test]
fn transcript_progress_suppresses_legacy_tool_status_in_the_footer() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.handle_command(InlineCommand::SetInputStatus {
        left: Some("Running tool: edit_file".to_owned()),
        right: Some("10:30".to_owned()),
    });
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation: ProgressOperation::start(),
        phase: ProgressPhase::RunningTools,
    }));
    let area = Rect::new(0, 0, 80, 1);
    TranscriptWidget::new(&mut session).render(area, &mut Buffer::empty(area));
    let footer = session.render_input_status_line(80).unwrap();
    let text: String = footer.spans.iter().map(|span| span.content.as_ref()).collect();
    assert!(text.contains("10:30"));
    assert!(!text.contains("Running"), "{text}");
}

#[test]
fn transcript_progress_retains_git_through_tool_status_and_clears_removed_context() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.handle_command(InlineCommand::SetConfiguredInputStatus {
        left: Some("topic/retained*".to_owned()),
        right: Some("10:30".to_owned()),
    });
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation: ProgressOperation::start(),
        phase: ProgressPhase::RunningTools,
    }));
    let area = Rect::new(0, 0, 80, 1);
    TranscriptWidget::new(&mut session).render(area, &mut Buffer::empty(area));
    for (command, expected_git) in [
        (
            InlineCommand::SetInputStatus {
                left: Some("Running tool: edit_file".to_owned()),
                right: None,
            },
            true,
        ),
        (InlineCommand::SetInputStatus { left: None, right: None }, true),
        (InlineCommand::SetConfiguredInputStatus { left: None, right: None }, false),
        (InlineCommand::SetConfiguredInputStatus { left: Some("  ".to_owned()), right: None }, false),
    ] {
        session.handle_command(command);
        let text: String = session
            .render_input_status_line(80)
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
            .unwrap_or_default();
        assert_eq!(text.contains("topic/retained*"), expected_git, "{text}");
        assert!(!text.contains("Running"), "{text}");
    }
}

#[test]
fn progress_footer_fallback_tracks_current_frame_even_without_a_transcript_body() {
    for static_label in [false, true] {
        let mut session = Session::new(InlineTheme::default(), None, 20);
        session.appearance.screen_reader_mode = static_label;
        session.handle_command(InlineCommand::SetConfiguredInputStatus {
            left: Some("topic/footer*".to_owned()),
            right: None,
        });
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::SavingCheckpoint,
        }));
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        // Alternate visible, hidden, progress-only, and hidden allocations so
        // stale geometry cannot hide the footer or create a duplicate.
        for height in [4, 0, 1, 0, 4] {
            terminal
                .draw(|frame| {
                    let layout = session.prepare_frame_layout(frame, 0).unwrap();
                    let area = Rect::new(layout.main_area.x, layout.main_area.y, layout.main_area.width, height);
                    session.render_base_frame(frame, &layout, area);
                    session.render_input(frame, layout.input_area);
                })
                .unwrap();
            let text = rendered_text(terminal.backend().buffer());
            assert_eq!(text.matches("Saving checkpoint").count(), 1, "height {height}: {text}");
            assert_eq!(session.progress_row_visible(), height > 0);
            if height <= 1 {
                assert!(session.transcript_area().is_none());
            }
            let footer_text: String = session
                .render_input_status_line(80)
                .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
                .unwrap_or_default();
            assert_eq!(footer_text.contains("Saving checkpoint"), height == 0);
            assert!(footer_text.contains("topic/footer*"), "configured context survives fallback: {footer_text}");
        }
    }
}

#[test]
fn accepted_operation_is_visible_without_changing_input_authority() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session.set_input("hello");
    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(InlineEvent::Submit(_))
    ));
    assert!(!session.progress.is_active(), "a submission is not yet an accepted runtime operation");
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::PreparingContext,
    }));
    assert!(session.progress.is_active());
    assert_eq!(session.activity_state, ActivityState::Idle);
    assert!(!session.is_running_activity());
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    terminal.draw(|frame| session.render(frame)).unwrap();
    assert!(rendered_text(terminal.backend().buffer()).contains("Preparing context"));
    assert!(session.lines.is_empty());
    assert!(session.transcript_export_text().is_empty());
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
    assert!(!session.progress.is_active());
}

#[test]
fn another_submission_during_preparation_does_not_replace_active_progress() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    let operation = ProgressOperation::start();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation,
        phase: ProgressPhase::SavingCheckpoint,
    }));
    session.set_input("next message");
    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(InlineEvent::Submit(_))
    ));
    assert_eq!(session.progress.active.unwrap().operation, operation);
    assert_eq!(session.progress.text().as_deref(), Some("Saving checkpoint · 0s"));
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
    assert!(!session.progress.is_active());
}

#[test]
fn copy_outcome_keeps_static_footer_style_during_animated_progress() {
    let mut session = Session::new(InlineTheme::default(), None, 20);
    session
        .handle_command(InlineCommand::SetConfiguredInputStatus { left: Some("topic/copy*".to_owned()), right: None });
    session.show_copy_notification(5);
    let expected = session.render_input_status_line(120).unwrap();
    session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
        operation: ProgressOperation::start(),
        phase: ProgressPhase::WaitingForModel,
    }));
    assert_eq!(session.render_input_status_line(120).unwrap(), expected);
    let area = Rect::new(0, 0, 120, 1);
    TranscriptWidget::new(&mut session).render(area, &mut Buffer::empty(area));
    assert!(session.progress_row_visible());
    assert_eq!(session.render_input_status_line(120).unwrap(), expected);
}

#[test]
fn progress_reserves_one_row_and_cleans_up_at_wide_and_narrow_widths() {
    for width in [120, 48, 12] {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        for index in 0..20 {
            session.push_line(
                InlineMessageKind::Agent,
                vec![InlineSegment {
                    text: format!("row {index}"),
                    style: Arc::new(InlineTextStyle::default()),
                }],
            );
        }
        let export = session.transcript_export_text();
        let operation = ProgressOperation::start();
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation,
            phase: ProgressPhase::SavingCheckpoint,
        }));
        let area = Rect::new(0, 0, width, 8);
        let mut buf = Buffer::empty(area);
        TranscriptWidget::new(&mut session).render(area, &mut buf);
        let body = session.transcript_area().unwrap();
        assert_eq!(body.height, 7);
        assert_eq!(body.bottom(), 7, "progress must be outside selection and link geometry");
        let row: String = (0..width).map(|x| buf[(x, 7)].symbol()).collect();
        assert!(row.contains("Saving"));
        assert_eq!(session.transcript_export_text(), export);
        session
            .mouse_selection
            .set_selection((body.x, body.y), (area.right().saturating_sub(1), 7));
        let selected = session.mouse_selection.extract_text(&buf, body);
        assert!(!selected.contains("Saving"));
        assert!(selected.contains("row"));
        let revision = session.current_transcript_revision();
        session.handle_tick();
        assert_eq!(session.current_transcript_revision(), revision, "animation must not reflow history");
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
        let mut cleared = Buffer::empty(area);
        TranscriptWidget::new(&mut session).render(area, &mut cleared);
        assert_eq!(session.transcript_area().unwrap().height, 8);
        assert!(!rendered_text(&cleared).contains("Saving"));
    }
}

#[test]
fn progress_approval_and_accessibility_fallbacks_keep_static_labels() {
    for screen_reader in [false, true] {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.appearance.reduce_motion_mode = !screen_reader;
        session.appearance.screen_reader_mode = screen_reader;
        let operation = ProgressOperation::start();
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation,
            phase: ProgressPhase::WaitingForModel,
        }));
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        session.render_progress(area, &mut buf);
        assert!(rendered_text(&buf).contains("Waiting for model"));
        let phase = session.shimmer_state.phase();
        session.handle_tick();
        assert_eq!(session.shimmer_state.phase().to_bits(), phase.to_bits());
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Phase {
            operation,
            phase: ProgressPhase::WaitingForApproval,
        }));
        session.progress.elapsed_secs = 7;
        assert!(!session.progress.tick());
        assert_eq!(session.progress.text().as_deref(), Some("Waiting for approval · 7s"));
        assert!(!session.progress.is_animated());
    }
}

#[test]
fn progress_rejects_stale_updates_and_late_restart() {
    let old = ProgressOperation::start();
    let new = ProgressOperation::start();
    let mut progress = TransientProgress::default();
    assert!(progress.apply(ProgressUpdate::Begin {
        operation: old,
        phase: ProgressPhase::PreparingContext
    }));
    assert!(progress.apply(ProgressUpdate::Begin {
        operation: new,
        phase: ProgressPhase::SavingCheckpoint
    }));
    assert!(!progress.apply(ProgressUpdate::Phase { operation: old, phase: ProgressPhase::RunningTools }));
    assert!(!progress.apply(ProgressUpdate::Finish { operation: old }));
    assert_eq!(progress.text().as_deref(), Some("Saving checkpoint · 0s"));
    assert!(progress.apply(ProgressUpdate::Finish { operation: new }));
    assert!(!progress.apply(ProgressUpdate::Begin { operation: old, phase: ProgressPhase::Retrying }));
    assert!(!progress.apply(ProgressUpdate::Phase { operation: new, phase: ProgressPhase::Processing }));
    assert!(!progress.is_active());
}

#[test]
fn model_phases_are_deduplicated_against_current_presentation() {
    let operation = ProgressOperation::start();
    let mut progress = TransientProgress::default();
    assert!(progress.apply(ProgressUpdate::Begin { operation, phase: ProgressPhase::ReceivingResponse }));
    assert!(!progress.apply(ProgressUpdate::Phase { operation, phase: ProgressPhase::ReceivingResponse }));
    for phase in [ProgressPhase::RunningTools, ProgressPhase::WaitingForApproval] {
        assert!(progress.apply(ProgressUpdate::Phase { operation, phase }));
        assert!(progress.apply(ProgressUpdate::Phase { operation, phase: ProgressPhase::ReceivingResponse }));
        assert!(!progress.apply(ProgressUpdate::Phase { operation, phase: ProgressPhase::ReceivingResponse }));
    }
}

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

    // The transcript row still owns the phase.
    let area = Rect::new(0, 0, 80, 1);
    let mut buf = Buffer::empty(area);
    session.render_progress(area, &mut buf);
    assert!(rendered_text(&buf).contains("Running tools"), "progress row must own the phase");

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
