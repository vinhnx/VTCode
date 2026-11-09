use super::*;

#[test]
fn runtime_progress_owns_feedback_through_the_production_event_path() {
    use crate::tui::core_tui::app::types::InlineHandle;
    use ratatui::backend::TestBackend;
    use vtcode_commons::ui_protocol::ProgressPhase;

    for fullscreen in [false, true] {
        let mut session = build_session();
        session.core.set_fullscreen_active(fullscreen);
        session.core.set_input("hello");
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), &event_tx, None);
        assert!(matches!(event_rx.try_recv().unwrap(), InlineEvent::Submit(input) if input.text == "hello"));
        assert!(!session.core.progress.is_active(), "local routing must not create an unowned operation");

        let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_tx);
        let guard = handle.begin_progress(ProgressPhase::PreparingContext);
        session.handle_command(command_rx.try_recv().unwrap());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| session.render(frame)).unwrap();
        let text: String = terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("Preparing context"));
        assert!(!session.core.is_running_activity());
        assert!(session.core.transcript_export_text().is_empty());

        drop(guard);
        session.handle_command(command_rx.try_recv().unwrap());
        assert!(!session.core.progress.is_active());
        terminal.draw(|frame| session.render(frame)).unwrap();
        let text: String = terminal.backend().buffer().content.iter().map(|cell| cell.symbol()).collect();
        assert!(!text.contains("Preparing context"));
    }
}
#[test]
#[serial_test::serial(theme_runtime)]
fn cancelling_a_list_modal_notifies_the_preview_owner() {
    use crate::tui::core_tui::app::types::ListOverlayRequest;
    use crate::tui::core_tui::types::{InlineListItem, InlineListSelection};

    let original_theme = crate::theme::active_theme_id();
    crate::theme::set_active_theme("ciapre").expect("built-in committed theme");
    crate::theme::set_preview_theme("mono").expect("built-in preview theme");
    let mut session = build_session();
    let cancellation_count = Arc::new(AtomicUsize::new(0));
    let callback_count = Arc::clone(&cancellation_count);
    session.preview_callback = Some(Arc::new(move |selection| {
        if selection.is_none() {
            callback_count.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }));
    session.show_transient(TransientRequest::List(ListOverlayRequest {
        title: "Theme".to_string(),
        lines: Vec::new(),
        footer_hint: None,
        items: vec![InlineListItem {
            title: "Ciapre".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Theme("ciapre".to_string())),
            search_value: None,
            ..Default::default()
        }],
        selected: Some(InlineListSelection::Theme("ciapre".to_string())),
        search: None,
        hotkeys: Vec::new(),
        status: None,
        plan_markdown: None,
    }));

    let event = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(matches!(event, Some(InlineEvent::Transient(TransientEvent::Cancelled))));
    assert_eq!(cancellation_count.load(Ordering::Relaxed), 1, "cancel should notify the preview owner once");
    assert!(!crate::theme::has_preview_theme(), "the runtime fallback must clear a stale preview");
    crate::theme::set_active_theme(&original_theme).expect("restore original theme after test");
}
#[test]
fn paste_routes_to_list_search_after_composer_is_reenabled() {
    use crate::tui::core_tui::app::types::ListOverlayRequest;
    use crate::tui::core_tui::types::{InlineListItem, InlineListSearchConfig, InlineListSelection};

    for searchable in [false, true] {
        let mut session = build_session();
        session.core.input_manager.set_content("draft".to_string());
        session.show_transient(TransientRequest::List(ListOverlayRequest {
            title: "Choose".to_string(),
            lines: Vec::new(),
            footer_hint: None,
            items: ["alpha", "beta"]
                .into_iter()
                .map(|title| InlineListItem {
                    title: title.to_string(),
                    subtitle: None,
                    badge: None,
                    indent: 0,
                    selection: Some(InlineListSelection::SlashCommand(title.to_string())),
                    search_value: Some(title.to_string()),
                    ..Default::default()
                })
                .collect(),
            selected: None,
            search: searchable.then(|| InlineListSearchConfig {
                label: "Filter".to_string(),
                placeholder: None,
                fuzzy: false,
            }),
            hotkeys: Vec::new(),
            status: None,
            plan_markdown: None,
        }));
        session.core.set_input_enabled(true);
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        session.handle_event(CrosstermEvent::Paste("beta".to_string()), &sender, None);

        assert_eq!(session.core.input_manager.content(), "draft");
        assert!(receiver.try_recv().is_err());
        let modal = session.modal_state_mut().expect("overlay remains active");
        if searchable {
            assert_eq!(modal.search.as_ref().expect("search").query, "beta");
            assert_eq!(modal.list.as_ref().expect("list").visible_indices, vec![1]);
        }
    }
}
#[test]
fn paste_routes_to_history_search_after_composer_is_reenabled() {
    let mut session = build_session();
    session.core.input_manager.set_content("draft".to_string());
    session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(session.history_picker_visible());
    session.core.set_input_enabled(true);
    let draft = session.core.input_manager.content().to_string();
    let query = session.history_picker_state.search_query.clone();

    assert!(handle_paste(&mut session, "beta").is_none());

    assert_eq!(session.core.input_manager.content(), draft);
    assert_eq!(session.history_picker_state.search_query, format!("{query}beta"));
}
#[test]
fn paste_respects_visible_surface_focus_policy() {
    for surface in [
        TransientSurface::DiffPreview,
        TransientSurface::ToolOutputViewer,
        TransientSurface::LocalAgents,
        TransientSurface::SlashPalette,
        TransientSurface::AgentPalette,
        TransientSurface::FilePalette,
        TransientSurface::TaskPanel,
    ] {
        let mut session = build_session();
        session.core.input_manager.set_content("draft".to_string());
        session.show_transient_surface(surface);
        session.core.set_input_enabled(true);

        assert!(handle_paste(&mut session, "beta").is_none());

        let expected = match surface.focus_policy() {
            TransientFocusPolicy::Modal | TransientFocusPolicy::CapturedInput => "draft",
            TransientFocusPolicy::SharedInput | TransientFocusPolicy::Passive => "draftbeta",
        };
        assert_eq!(session.core.input_manager.content(), expected, "{surface:?}");
    }
}
#[test]
fn paste_does_not_reach_suspended_history_search() {
    let mut session = build_session();
    session.process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert!(session.history_picker_visible());
    let query = session.history_picker_state.search_query.clone();
    session.show_transient(TransientRequest::Modal(ModalOverlayRequest {
        title: "Notice".to_string(),
        lines: Vec::new(),
        secure_prompt: None,
        is_help_modal: false,
    }));
    session.core.set_input_enabled(true);
    let draft = session.core.input_manager.content().to_string();

    assert!(handle_paste(&mut session, "beta").is_none());

    assert_eq!(session.history_picker_state.search_query, query);
    assert_eq!(session.core.input_manager.content(), draft);
    assert!(session.has_active_overlay());
}
#[test]
fn paste_routes_to_viewer_only_while_search_is_active() {
    let mut session = build_session();
    session.core.input_manager.set_content("draft".to_string());
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec!["beta output".to_string()],
        ..Default::default()
    });
    session.tool_output_revision = 1;
    session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert!(session.tool_output_viewer_state().is_some());
    session.core.set_input_enabled(true);

    assert!(handle_paste(&mut session, "ignored").is_none());
    assert_eq!(session.core.input_manager.content(), "draft");

    session.tool_output_viewer_state_mut().expect("viewer").start_search();
    assert!(handle_paste(&mut session, "beta").is_none());
    session.tool_output_viewer_state_mut().expect("viewer").commit_search(8);

    assert_eq!(session.core.input_manager.content(), "draft");
    assert!(
        session
            .tool_output_viewer_state()
            .expect("viewer")
            .status_label()
            .contains("search 'beta'")
    );
}
#[test]
fn capture_fifo_keeps_open_viewer_blocks() {
    let mut session = build_session();
    session.record_tool_output_block(1, vec!["first-capture".to_string()]);
    session.open_tool_output_viewer(40, 10, Some(1));
    assert!(session.tool_output_viewer_state().is_some());

    let overflow = ui::TUI_TOOL_OUTPUT_BLOCKS_MAX as u64 + 8;
    for id in 2..=overflow {
        session.record_tool_output_block(id, vec![format!("capture-{id}")]);
    }

    assert!(
        session.tool_output_blocks.iter().any(|block| block.id == 1),
        "open viewer's capture must stay pinned against FIFO eviction"
    );
    assert!(session.tool_output_blocks.len() <= ui::TUI_TOOL_OUTPUT_BLOCKS_MAX + 1);
}
#[test]
fn capture_fifo_keeps_newest_block_even_when_viewer_pins_cap() {
    let mut session = build_session();
    let cap = ui::TUI_TOOL_OUTPUT_BLOCKS_MAX as u64;
    for id in 1..=cap {
        session.record_tool_output_block(id, vec![format!("capture-{id}")]);
    }
    // Open a viewer that retains every existing capture.
    session.open_tool_output_viewer(40, 10, None);
    assert!(session.tool_output_viewer_state().is_some());

    let newest_id = cap + 1;
    session.record_tool_output_block(newest_id, vec!["newest".to_string()]);
    assert!(
        session.tool_output_blocks.iter().any(|block| block.id == newest_id),
        "the just-recorded capture must never be FIFO-dropped"
    );
}
#[test]
fn transient_activity_signal_tracks_ui_owned_surface_lifecycle() {
    let signal = Arc::new(TransientActivitySignal::default());
    let mut session = build_session();
    session.set_transient_activity_signal(signal.clone());

    session.show_transient(TransientRequest::LocalAgents(LocalAgentsTransientRequest { visible: Some(true) }));
    assert!(signal.is_active());

    assert!(session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).is_none());
    assert!(!signal.is_active());

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert!(signal.is_active());
    assert!(session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)).is_none());
    assert!(!signal.is_active());
}
#[test]
fn transient_activity_signal_keeps_lower_captured_surface_after_nested_close() {
    let signal = Arc::new(TransientActivitySignal::default());
    let mut session = build_session();
    session.set_transient_activity_signal(signal.clone());

    session.show_transient(TransientRequest::LocalAgents(LocalAgentsTransientRequest { visible: Some(true) }));
    session.show_transient(TransientRequest::Modal(ModalOverlayRequest {
        title: "nested".to_string(),
        lines: Vec::new(),
        secure_prompt: None,
        is_help_modal: false,
    }));
    assert!(signal.is_active());

    session.close_transient();
    assert!(signal.is_active(), "the lower captured surface still owns input");

    session.close_transient();
    assert!(!signal.is_active());
}
#[test]
fn locked_activity_blocks_explicit_mode_switch_commands() {
    for state in [
        vtcode_commons::ui_protocol::ActivityState::Building,
        vtcode_commons::ui_protocol::ActivityState::Recovery,
    ] {
        for input in ["/mode", "/mode build", "/plan", "/plan on"] {
            let mut session = build_session();
            session.core.handle_command(CoreInlineCommand::SetActivityState(state));

            assert!(
                handle_running_slash_command_block_for_input(&mut session, input),
                "{input} must stay locked in {state:?}"
            );
        }
    }
}
#[test]
fn navigation_commands_submit_while_busy_instead_of_blocking() {
    // Asymmetric: `/new` and `/exit` must fall through while busy so they
    // queue/submit instead of showing "disabled".
    // - In busy `PreparingFreshExecutionThread` (is_running true), a generic
    //   command like `/status` stays blocked, proving the exemption is
    //   specific.
    // - In stage `Building`/`Recovery`, mode switches stay locked.
    {
        let state = vtcode_commons::ui_protocol::ActivityState::PreparingFreshExecutionThread;
        for input in ["/new", "/exit"] {
            let mut session = build_session();
            session.core.handle_command(CoreInlineCommand::SetActivityState(state));

            assert!(
                !handle_running_slash_command_block_for_input(&mut session, input),
                "{input} must not block in {state:?}"
            );
        }
        let mut session = build_session();
        session.core.handle_command(CoreInlineCommand::SetActivityState(state));
        assert!(
            handle_running_slash_command_block_for_input(&mut session, "/status"),
            "/status must stay blocked in {state:?} to prove the exemption is specific"
        );
    }
    for state in [
        vtcode_commons::ui_protocol::ActivityState::Building,
        vtcode_commons::ui_protocol::ActivityState::Recovery,
    ] {
        for input in ["/new", "/new fresh topic", "/exit", "   /exit   "] {
            let mut session = build_session();
            session.core.handle_command(CoreInlineCommand::SetActivityState(state));

            assert!(
                !handle_running_slash_command_block_for_input(&mut session, input),
                "{input} must not block in {state:?}"
            );
        }
        let mut session = build_session();
        session.core.handle_command(CoreInlineCommand::SetActivityState(state));
        assert!(
            handle_running_slash_command_block_for_input(&mut session, "/mode"),
            "/mode must stay blocked in {state:?} to prove the exemption is specific"
        );
    }
}
#[test]
fn blocked_activity_allows_explicit_slash_commands() {
    for input in [
        "/mode",
        "/mode build",
        "/plan",
        "/status",
        "/effort high",
        "/clear",
        "/exit",
    ] {
        let mut session = build_session();
        session
            .core
            .handle_command(CoreInlineCommand::SetActivityState(vtcode_commons::ui_protocol::ActivityState::Blocked));

        assert!(
            !handle_running_slash_command_block_for_input(&mut session, input),
            "{input} must be accepted while the turn is blocked"
        );
    }
}
#[test]
fn progress_renders_in_both_surfaces_and_survives_overlay_clipping() {
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};
    for fullscreen in [false, true] {
        for width in [120, 48] {
            let mut session = build_session();
            session.core.set_fullscreen_active(fullscreen);
            let operation = ProgressOperation::start();
            session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
                operation,
                phase: ProgressPhase::WaitingForApproval,
            }));
            let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, 24)).unwrap();
            terminal.draw(|frame| session.render(frame)).unwrap();
            assert_eq!(rendered_buffer_text(&terminal).matches("Waiting for approval").count(), 1);
            let body = session.core.transcript_area().unwrap();
            assert!(body.bottom() < 24);
            assert!(session.core.transcript_export_text().is_empty());
            session.show_transient(TransientRequest::Modal(ModalOverlayRequest {
                title: "Approval details".into(),
                lines: vec!["Review this request".into()],
                secure_prompt: None,
                is_help_modal: false,
            }));
            terminal.draw(|frame| session.render(frame)).unwrap();
            assert!(rendered_buffer_text(&terminal).contains("Approval details"));
            assert_eq!(rendered_buffer_text(&terminal).matches("Waiting for approval").count(), 1);
            session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            terminal.draw(|frame| session.render(frame)).unwrap();
            assert_eq!(rendered_buffer_text(&terminal).matches("Waiting for approval").count(), 1);
            session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation }));
            terminal.draw(|frame| session.render(frame)).unwrap();
            assert!(!rendered_buffer_text(&terminal).contains("Waiting for approval"));
        }
    }
}
