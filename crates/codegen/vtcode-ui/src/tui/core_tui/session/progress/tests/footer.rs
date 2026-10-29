use super::*;

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
