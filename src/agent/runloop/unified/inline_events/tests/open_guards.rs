use super::*;

#[tokio::test]
async fn open_file_in_editor_event_emits_out_of_band_request_with_path() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let (editor_sender, mut editor_requests) = bounded_editor_open_requests();
    context.set_editor_open_sink(editor_sender, Arc::new(EditorOpenDispatcher::new(true)));
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
    let path = "/tmp/demo.rs".to_string();

    let action = context
        .process_event(InlineEvent::OpenFileInEditor(path.clone()), &mut queue)
        .await
        .expect("process open file in editor");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert_eq!(
        editor_requests.recv().await,
        Some(EditorOpenRequest::from_raw_target(&path, &config.workspace).expect("valid editor target"))
    );
    assert!(queued_inputs.is_empty());
}
#[tokio::test]
async fn open_file_drain_skips_event_instance_forwarded_by_callback() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let workspace = config.workspace.clone();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let (editor_sender, mut editor_requests) = bounded_editor_open_requests();
    let dispatcher = Arc::new(EditorOpenDispatcher::new(true));
    dispatcher.set_sender(editor_sender.clone());
    context.set_editor_open_sink(editor_sender, dispatcher.clone());
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
    let path = "/tmp/demo.rs".to_string();

    // Simulate the TUI event callback firing first (immediate mid-turn
    // open), then the same event instance arriving via the main channel
    // drain after the turn.
    assert!(dispatcher.try_forward_immediate(&path, &workspace));
    let action = context
        .process_event(InlineEvent::OpenFileInEditor(path.clone()), &mut queue)
        .await
        .expect("process open file in editor");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert_eq!(
        editor_requests.recv().await,
        Some(EditorOpenRequest::from_raw_target(&path, &workspace).expect("valid editor target"))
    );
    assert!(editor_requests.try_recv().is_err());
}
#[tokio::test]
async fn open_url_event_shows_guard_modal_with_deny_selected_by_default() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let url = "https://example.com/docs".to_string();
    {
        let mut history = Vec::<uni::Message>::new();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();
        let mut context = InlineEventContext::new(
            &mut renderer,
            &handle,
            interrupts,
            &mut ctrl_c_notice_displayed,
            &mut header_context,
            &mut model_picker_state,
            &mut palette_state,
            &mut config,
            &mut vt_cfg,
            &mut provider_client,
            &ctrl_c_state,
            &ctrl_c_notify,
            &session_bootstrap,
            false,
            &mut history,
            &mut session_stats,
            &mut context_manager,
            "test-session",
            "test-thread",
            None,
            None,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(InlineEvent::OpenUrl(url.clone()), &mut queue)
            .await
            .expect("process open url");
        assert!(matches!(action, InlineLoopAction::Continue));
    }

    let prompt = UrlGuardPrompt::parse(url.clone()).expect("parse url guard prompt");
    let command = commands.recv().await.expect("guard overlay command");
    match command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => {
                assert_eq!(request.title, "Open External Link");
                assert_eq!(request.selected, Some(prompt.default_selection()));
                assert_eq!(request.lines, prompt.lines());
                let titles: Vec<_> = request.items.iter().map(|item| item.title.as_str()).collect();
                assert_eq!(titles, vec!["Cancel", "Open in browser"]);
            }
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }

    assert!(matches!(palette_state, Some(ActivePalette::UrlGuard { .. })));
}
#[tokio::test]
async fn open_http_url_guard_modal_includes_insecure_transport_warning() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    {
        let mut history = Vec::<uni::Message>::new();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();
        let mut context = InlineEventContext::new(
            &mut renderer,
            &handle,
            interrupts,
            &mut ctrl_c_notice_displayed,
            &mut header_context,
            &mut model_picker_state,
            &mut palette_state,
            &mut config,
            &mut vt_cfg,
            &mut provider_client,
            &ctrl_c_state,
            &ctrl_c_notify,
            &session_bootstrap,
            false,
            &mut history,
            &mut session_stats,
            &mut context_manager,
            "test-session",
            "test-thread",
            None,
            None,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(InlineEvent::OpenUrl("http://example.com/docs".to_string()), &mut queue)
            .await
            .expect("process insecure open url");
        assert!(matches!(action, InlineLoopAction::Continue));
    }

    let command = commands.recv().await.expect("guard overlay command");
    match command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => {
                assert!(request.lines.iter().any(|line| line.contains("Plain HTTP")));
            }
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }
}
#[tokio::test]
async fn cancelling_url_guard_restores_previous_palette() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Theme {
        mode: ThemePaletteMode::Select,
        original_theme_id: theme::DEFAULT_THEME_ID.to_string(),
    });
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    {
        let mut history = Vec::<uni::Message>::new();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();
        let mut context = InlineEventContext::new(
            &mut renderer,
            &handle,
            interrupts,
            &mut ctrl_c_notice_displayed,
            &mut header_context,
            &mut model_picker_state,
            &mut palette_state,
            &mut config,
            &mut vt_cfg,
            &mut provider_client,
            &ctrl_c_state,
            &ctrl_c_notify,
            &session_bootstrap,
            false,
            &mut history,
            &mut session_stats,
            &mut context_manager,
            "test-session",
            "test-thread",
            None,
            None,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(InlineEvent::OpenUrl("https://example.com/docs".to_string()), &mut queue)
            .await
            .expect("process open url");
        assert!(matches!(action, InlineLoopAction::Continue));

        let cancel = context
            .process_event(InlineEvent::Transient(TransientEvent::Cancelled), &mut queue)
            .await
            .expect("process cancel");
        assert!(matches!(cancel, InlineLoopAction::Continue));
    }

    let initial_command = commands.recv().await.expect("url guard command");
    match initial_command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => assert_eq!(request.title, "Open External Link"),
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }

    let restored_command = commands.recv().await.expect("restored palette command");
    match restored_command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => assert_eq!(request.title, "Theme"),
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }

    assert!(matches!(palette_state, Some(ActivePalette::Theme { mode: ThemePaletteMode::Select, .. })));
}
