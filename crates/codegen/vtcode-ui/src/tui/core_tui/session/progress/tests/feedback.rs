use super::*;

#[test]
fn tool_output_probe_progress_is_visible_and_preserves_configured_footer() {
    for height in [6, 24] {
        let mut session = Session::new(InlineTheme::default(), None, height);
        session.handle_command(InlineCommand::SetConfiguredInputStatus {
            left: Some("branch: main".into()),
            right: Some("model: fixture".into()),
        });
        let operation = ProgressOperation::start();
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation,
            phase: ProgressPhase::RunningTools,
        }));
        let mut terminal = Terminal::new(TestBackend::new(100, height)).unwrap();
        for phase in [ProgressPhase::CheckingToolOutput, ProgressPhase::RunningTools] {
            session.handle_command(InlineCommand::SetInputStatus {
                left: Some("Checking tool output...".into()),
                right: None,
            });
            session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Phase { operation, phase }));
            terminal
                .draw(|frame| {
                    let layout = session.prepare_frame_layout(frame, 0).unwrap();
                    session.render_base_frame(frame, &layout, Rect::ZERO);
                    session.render_input(frame, layout.input_area);
                })
                .unwrap();
            let text = rendered_text(terminal.backend().buffer());
            assert_eq!(text.matches(phase.label()).count(), 1, "{text}");
            assert!(text.contains("branch: main"), "{text}");
            assert!(text.contains("model: fixture"), "{text}");
            if phase == ProgressPhase::RunningTools {
                assert!(!text.contains("Checking tool output..."));
            }
        }
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
        terminal
            .draw(|frame| {
                let layout = session.prepare_frame_layout(frame, 0).unwrap();
                session.render_base_frame(frame, &layout, Rect::ZERO);
                session.render_input(frame, layout.input_area);
            })
            .unwrap();
        assert!(!rendered_text(terminal.backend().buffer()).contains("Checking tool output..."));
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
fn progress_feedback_stays_visible_with_docked_local_agents() {
    use crate::tui::core_tui::app::{
        session::AppSession,
        types::{InlineCommand as AppCommand, LocalAgentsTransientRequest, TransientRequest},
    };

    // Local agents is now an inline bottom-dock, not a fullscreen cover:
    // the transcript progress row stays visible above the panel.
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
        assert!(rendered_text(terminal.backend().buffer()).contains("Waiting for model"));
        assert!(session.core.progress.active.unwrap().feedback_observed);
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
