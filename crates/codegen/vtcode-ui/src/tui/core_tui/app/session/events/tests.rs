use super::*;
use crate::tui::core_tui::app::types::{
    CompactActivityMetadata, InlineCommand, LocalAgentsTransientRequest, ModalOverlayRequest, TransientActivitySignal,
    TransientRequest,
};
use crate::tui::core_tui::session::action::BindingStore;
use crate::tui::core_tui::types::{
    InlineCommand as CoreInlineCommand, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
    SecurePromptConfig,
};
use hashbrown::HashMap;
use ratatui::Terminal;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn build_session() -> Session {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.core.set_fullscreen_active(true);
    session.core.apply_transcript_rows(8);
    session.core.apply_transcript_width(60);
    session
}

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

fn text_segment(text: impl Into<String>) -> InlineSegment {
    InlineSegment {
        text: text.into(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn rendered_buffer_text(terminal: &Terminal<ratatui::backend::TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer.cell((column, row)).expect("buffer cell").symbol())
                .collect::<Vec<_>>()
                .concat()
        })
        .collect::<Vec<_>>()
        .join("\n")
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

fn add_compact_activity(session: &mut Session, id: u64, command: &str) {
    session.handle_command(InlineCommand::RecordToolOutput {
        id,
        lines: vec![format!("• Ran {command}"), "  └ complete output".to_string()],
    });
    session.handle_command(InlineCommand::AppendCompactActivity(CompactActivityMetadata {
        group_id: id,
        command_count: 1,
        command: Some(command.to_string().into()),
        hidden_line_count: 1,
        suffix: None,
        review_anchor: Some(id),
        review_anchors: vec![id],
    }));
}

#[test]
fn ctrl_o_emits_copy_command() {
    let mut session = build_session();
    session
        .core
        .push_line(InlineMessageKind::Agent, vec![text_segment("agent reply")]);

    let event = session.process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert!(
        matches!(event, Some(InlineEvent::Submit(ref cmd)) if cmd == "/copy"),
        "Ctrl+O should emit Submit(\"/copy\"), got {event:?}"
    );
}

#[test]
fn ctrl_t_opens_and_closes_tool_output_viewer_in_fullscreen() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec!["• Ran echo hello".to_string(), "  └ hello".to_string()],
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
    assert!(session.process_key(key).is_none());
    assert!(session.tool_output_viewer_state().is_some());

    assert!(session.process_key(key).is_none());
    assert!(session.tool_output_viewer_state().is_none());
}

#[test]
fn ctrl_t_opens_tool_output_viewer_outside_fullscreen() {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.core.input_manager.set_content("abc".to_string());
    session.core.input_manager.set_cursor(1);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.core.input_manager.content(), "abc");
    assert!(session.tool_output_viewer_state().is_some());
}

#[test]
fn raw_ctrl_t_opens_tool_output_viewer() {
    let mut session = Session::new(InlineTheme::default(), None, 24);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{14}'), KeyModifiers::empty()));

    assert!(result.is_none());
    assert!(session.tool_output_viewer_state().is_some());
}

#[test]
fn unbound_ctrl_t_keeps_readline_transpose_outside_fullscreen() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), Vec::new());
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.input_manager.set_content("abc".to_string());
    session.core.input_manager.set_cursor(1);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert!(result.is_none());
    assert_eq!(session.core.input_manager.content(), "bac");
    assert!(session.tool_output_viewer_state().is_none());
}

#[test]
fn unbound_raw_ctrl_t_keeps_readline_transpose_outside_fullscreen() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), Vec::new());
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.input_manager.set_content("abc".to_string());
    session.core.input_manager.set_cursor(1);

    let result = session.process_key(KeyEvent::new(KeyCode::Char('\u{14}'), KeyModifiers::empty()));

    assert!(result.is_none());
    assert_eq!(session.core.input_manager.content(), "bac");
    assert!(session.tool_output_viewer_state().is_none());
}

#[test]
fn unbound_transcript_review_does_not_restore_ctrl_t_viewer_alias() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), Vec::new());
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.set_fullscreen_active(true);
    add_compact_activity(&mut session, 40, "printf unbound");

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert!(session.tool_output_viewer_state().is_none());

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render compact activity");
    assert!(session.compact_activity_hit_regions.is_empty());
}

#[test]
fn transcript_review_binding_can_open_outside_fullscreen() {
    let mut bindings = HashMap::new();
    bindings.insert("open_transcript_review".to_string(), vec!["ctrl+x".to_string()]);
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );

    assert!(!session.core.fullscreen.active);
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
            .is_none()
    );
    assert!(session.tool_output_viewer_state().is_some());
}

#[test]
fn configured_core_action_dispatches_through_app_session() {
    let mut bindings = HashMap::new();
    bindings.insert("open_model_picker".to_string(), vec!["ctrl+x".to_string()]);
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );

    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
        Some(InlineEvent::Submit(ref command)) if command == "/model"
    ));
}

#[test]
fn raw_control_exit_is_normalized_before_app_dispatch() {
    let mut session = build_session();

    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Char('\u{4}'), KeyModifiers::NONE)),
        Some(InlineEvent::Exit)
    ));
}

#[test]
fn repeated_tui_ctrl_c_exits_through_the_event_path() {
    let mut session = build_session();
    let (events, mut received) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), &events, None);
    session.handle_event(CrosstermEvent::Key(KeyEvent::new(KeyCode::Char('\u{3}'), KeyModifiers::NONE)), &events, None);

    assert!(matches!(received.try_recv(), Ok(InlineEvent::Interrupt)));
    assert!(matches!(received.try_recv(), Ok(InlineEvent::Exit)));
}

#[test]
fn double_idle_escape_submits_rewind_in_the_app_session() {
    let mut session = build_session();

    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(InlineEvent::Cancel)
    ));
    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(InlineEvent::Submit(value)) if value == "/rewind"
    ));
    // The double press consumed the armed timer, so the next press is a
    // fresh single-press cancel.
    assert!(matches!(
        session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Some(InlineEvent::Cancel)
    ));
}

#[test]
fn transcript_review_render_mode_is_rebindable_inside_viewer() {
    let mut bindings = HashMap::new();
    bindings.insert("toggle_transcript_render_mode".to_string(), vec!["alt+x".to_string()]);
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    session.core.set_fullscreen_active(true);

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
            .is_none()
    );
    let initial_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(initial_status.contains("rich"));

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT))
            .is_none()
    );
    let raw_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(raw_status.contains("raw"));
}

#[test]
fn transcript_render_binding_does_not_consume_normal_input() {
    let mut session = build_session();

    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE))
            .is_none()
    );
    assert_eq!(session.core.input_manager.content(), "r");
    assert!(session.tool_output_viewer_state().is_none());
}

#[test]
fn alt_o_compatibility_alias_opens_transcript_review() {
    let mut session = build_session();
    assert!(
        session
            .process_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT))
            .is_none()
    );
    assert!(session.tool_output_viewer_state().is_some());
}

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

#[test]
fn alt_g_toggles_task_panel() {
    let mut session = build_session();
    assert!(!session.show_task_panel);

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    assert!(session.show_task_panel, "Alt+G should reveal the task panel");

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    assert!(!session.show_task_panel, "Second Alt+G should hide the task panel");
}

#[test]
fn task_panel_visibility_requests_retain_body_and_metadata_progress() {
    use crate::tui::core_tui::app::types::{TaskPanelMetadata, TaskPanelTransientRequest, TransientRequest};
    use vtcode_commons::ui_protocol::TaskItemStatus;

    let mut session = build_session();
    let tree = vec!["  └ Implement".to_string(), "  └ Verify".to_string()];
    let statuses = vec![TaskItemStatus::Pending, TaskItemStatus::Completed];
    let metadata = TaskPanelMetadata {
        title: "Release".to_string(),
        completed: 1,
        total: 2,
    };

    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: tree.clone(),
        statuses: statuses.clone(),
        current: Some(0),
        visible: None,
        metadata: Some(metadata),
    }));
    assert_eq!(session.task_panel_lines, tree);
    assert_eq!(session.task_panel_statuses, statuses);
    assert_eq!(session.task_panel_current, Some(0));
    assert_eq!(session.task_panel_metadata.as_ref().map(|m| m.title.as_str()), Some("Release"));

    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: Vec::new(),
        statuses: Vec::new(),
        current: None,
        visible: Some(true),
        metadata: None,
    }));
    // Appearance may suppress auto-show; visibility must never wipe content.
    assert_eq!(session.task_panel_lines, tree, "show_task_panel must not wipe the panel body");
    assert_eq!(session.task_panel_statuses, statuses, "show_task_panel must not wipe row statuses");
    assert_eq!(session.task_panel_current, Some(0));
    assert_eq!(session.task_panel_metadata.as_ref().map(|m| m.completed), Some(1));

    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: Vec::new(),
        statuses: Vec::new(),
        current: None,
        visible: Some(false),
        metadata: None,
    }));
    assert!(!session.show_task_panel);
    assert_eq!(session.task_panel_lines, tree, "hide_task_panel must not wipe the panel body");

    // Session clear is a content update without metadata: it must drop
    // stale panel metadata so the terminal title cannot keep old N/M.
    session.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
        lines: Vec::new(),
        statuses: Vec::new(),
        current: None,
        visible: None,
        metadata: None,
    }));
    assert!(session.task_panel_lines.is_empty());
    assert!(session.task_panel_statuses.is_empty(), "clear must drop row statuses");
    assert_eq!(session.task_panel_current, None);
    assert!(session.task_panel_metadata.is_none(), "clear must drop panel metadata");
}

#[test]
fn ctrl_home_and_end_jump_transcript_in_fullscreen() {
    let mut session = build_session();
    for index in 0..40 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("line {index}"))]);
    }

    session.core.scroll_page_up();
    assert!(session.core.scroll_offset() > 0);

    let _ = session.process_key(KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL));
    assert_eq!(session.core.scroll_offset(), 0);

    let _ = session.process_key(KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL));
    assert_eq!(session.core.scroll_offset(), session.core.current_max_scroll_offset());
}

#[test]
fn tool_output_viewer_search_accept_and_cancel_work() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec![
            "• Ran alpha".to_string(),
            "  └ beta alpha".to_string(),
            "  └ gamma alpha".to_string(),
        ],
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in ['a', 'l', 'p', 'h', 'a'] {
        let _ = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let _ = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    let status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(status.contains("search 'alpha'"));
    assert!(status.contains("(2/3)"));

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    let _ = session.process_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    let status = session.tool_output_viewer_state().expect("viewer open").status_label();
    assert!(status.contains("search 'alpha'"));
}

#[test]
fn tool_output_viewer_exports_complete_tool_output() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec![
            "• Ran printf complete".to_string(),
            "  └ first complete line".to_string(),
            "    second complete line".to_string(),
        ],
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    match session.process_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)) {
        Some(InlineEvent::OpenToolOutputInEditor(text)) => {
            assert!(text.contains("first complete line"));
            assert!(text.contains("second complete line"));
        }
        other => panic!("unexpected viewer editor event: {other:?}"),
    }

    match session.process_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE)) {
        Some(InlineEvent::OpenToolOutputScrollback(text)) => {
            assert!(text.contains("first complete line"));
            assert!(text.contains("second complete line"));
        }
        other => panic!("unexpected viewer scrollback event: {other:?}"),
    }
}

#[test]
fn tool_output_viewer_scrolls_copies_selection_and_closes() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: (0..20).map(|index| format!("output line {index}")).collect(),
        ..Default::default()
    });
    session.tool_output_revision = 1;

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    let initial_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    let top_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    let next_status = session.tool_output_viewer_state().expect("viewer open").status_label();

    assert!(initial_status.contains("line 13/20"));
    assert!(top_status.contains("line 1/20"));
    assert!(next_status.contains("line 2/20"));

    session.core.mouse_selection.set_selection((1, 1), (8, 1));
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(session.core.mouse_selection.has_copy_request());

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(session.tool_output_viewer_state().is_none());
}

#[test]
fn tool_output_viewer_mouse_scroll_does_not_move_transcript() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: (0..20).map(|index| format!("output line {index}")).collect(),
        ..Default::default()
    });
    session.tool_output_revision = 1;
    for index in 0..20 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("transcript line {index}"))]);
    }

    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    let transcript_offset = session.core.scroll_offset();
    let viewer_status = session.tool_output_viewer_state().expect("viewer open").status_label();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &tx,
        None,
    );

    assert_eq!(session.core.scroll_offset(), transcript_offset);
    assert_ne!(session.tool_output_viewer_state().expect("viewer open").status_label(), viewer_status);
}

#[test]
fn mouse_events_are_ignored_when_fullscreen_mouse_capture_is_disabled() {
    let mut session = build_session();
    session.core.fullscreen.interaction.mouse_capture = false;
    for index in 0..20 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("line {index}"))]);
    }
    session.core.scroll_page_up();
    let initial_offset = session.core.scroll_offset();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &tx,
        None,
    );

    assert_eq!(session.core.scroll_offset(), initial_offset);
}

#[test]
fn transcript_wheel_scrolls_through_floating_plan_overlay() {
    let mut session = build_session();
    for index in 0..40 {
        session
            .core
            .push_line(InlineMessageKind::Agent, vec![text_segment(format!("plan line {index}"))]);
    }
    session.show_transient(TransientRequest::List(crate::tui::core_tui::app::types::ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines: vec!["A plan is ready to execute.".to_string()],
        footer_hint: None,
        items: vec![crate::tui::core_tui::types::InlineListItem {
            title: "Yes".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: None,
            search_value: None,
            ..Default::default()
        }],
        selected: None,
        search: None,
        hotkeys: Vec::new(),
        status: None,
    }));

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal.draw(|frame| session.render(frame)).expect("render overlay");
    let (events, _received) = tokio::sync::mpsc::unbounded_channel();

    session.handle_event(
        CrosstermEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }),
        &events,
        None,
    );

    assert!(session.core.scroll_offset() > 0, "transcript should remain scrollable behind the overlay");
}

#[test]
fn control_g_from_plan_approval_overlay_emits_launch_editor_hotkey() {
    use crate::tui::core_tui::app::types::{
        ListOverlayRequest, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey,
    };
    use crate::tui::core_tui::types::{InlineListItem, InlineListSelection};

    let mut session = build_session();
    session.show_transient(TransientRequest::List(ListOverlayRequest {
        title: "Ready to code?".to_string(),
        lines: vec!["A plan is ready to execute. Would you like to proceed?".to_string()],
        footer_hint: Some("ctrl-g to edit in VS Code · .vtcode/plans/test-plan.md".to_string()),
        items: vec![InlineListItem {
            title: "Yes, implement this plan".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalExecute),
            search_value: None,
            ..Default::default()
        }],
        selected: Some(InlineListSelection::PlanApprovalExecute),
        search: None,
        hotkeys: vec![TransientHotkey {
            key: TransientHotkeyKey::CtrlChar('g'),
            action: TransientHotkeyAction::LaunchEditor,
        }],
        status: None,
    }));
    assert!(session.has_active_overlay(), "plan approval overlay should be open");
    assert!(
        !session.core.input_enabled(),
        "modal focus must disable the composer so Ctrl+G hits the overlay hotkey, not the draft editor"
    );

    let event = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert!(
        matches!(
            &event,
            Some(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Hotkey(
                TransientHotkeyAction::LaunchEditor
            ))))
        ),
        "Ctrl+G must emit the plan-file hotkey, got {event:?}"
    );
    // The TUI closes the overlay on hotkey; the plan-approval runloop owns
    // re-showing it after the external editor closes (`show_overlay_and_wait`).
    assert!(!session.has_active_overlay(), "overlay closes locally; runloop re-shows after editing");
}

#[test]
fn control_g_without_overlay_still_opens_composer_draft_editor() {
    let mut session = build_session();
    assert!(!session.has_active_overlay());

    let event = session.process_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));

    assert!(
        matches!(&event, Some(InlineEvent::LaunchEditor { draft }) if draft.is_empty()),
        "composer Ctrl+G must keep the draft-editor path, got {event:?}"
    );
}

fn build_session_with_secure_prompt() -> Session {
    let mut session = build_session();
    session.show_transient(TransientRequest::Modal(ModalOverlayRequest {
        title: "Secure API key setup".to_string(),
        lines: vec!["Paste the key — it will be auto-detected and saved securely.".to_string()],
        secure_prompt: Some(SecurePromptConfig {
            label: "API key".to_string(),
            placeholder: None,
            mask_input: true,
        }),
        is_help_modal: false,
    }));
    assert!(session.has_active_overlay(), "secure prompt modal should be open");
    session
}

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
