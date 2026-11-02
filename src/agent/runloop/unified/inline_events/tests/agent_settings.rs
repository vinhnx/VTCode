use super::*;

#[tokio::test]
async fn previous_agent_event_cycles_primary_agent_backward() {
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
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::CyclePrimaryAgentPrevious, &mut queue)
        .await
        .expect("process previous primary agent");
    assert!(matches!(action, InlineLoopAction::CyclePrimaryAgentPrevious));
}
#[tokio::test]
async fn settings_editor_selection_submits_editor_config_command() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Settings {
        state: Box::new(SettingsPaletteState {
            workspace: std::path::PathBuf::from("."),
            source_path: std::path::PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("tools".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        }),
        esc_armed: false,
    });
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
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(ACTION_CONFIGURE_EDITOR.to_string()),
            ))),
            &mut queue,
        )
        .await
        .expect("process configure editor selection");

    assert!(matches!(
        action,
        InlineLoopAction::Submit(ref command) if command.text == "/config tools.editor"
    ));
    assert!(palette_state.is_none());
}
#[tokio::test]
async fn settings_string_selection_opens_value_editor() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Settings {
        state: Box::new(SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: temp.path().join("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("tools.editor".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        }),
        esc_armed: false,
    });
    let mut config = runtime_config();
    config.workspace = temp.path().to_path_buf();
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
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_EDIT}tools.editor.preferred_editor")),
            ))),
            &mut queue,
        )
        .await
        .expect("open string settings editor");
    assert!(matches!(action, InlineLoopAction::Continue));

    match commands.recv().await.expect("settings editor wizard command") {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::Wizard(request) => {
                assert_eq!(request.title, "Edit setting");
                assert_eq!(request.steps.len(), 1);
                assert_eq!(request.steps[0].freeform_default.as_deref(), Some(""));
            }
            other => panic!("expected settings editor wizard, got {other:?}"),
        },
        other => panic!("expected settings editor transient, got {}", other_name(&other)),
    }

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(vec![
                InlineListSelection::RequestUserInputAnswer {
                    question_id: "settings_value".to_string(),
                    selected: Vec::new(),
                    other: Some("code --wait".to_string()),
                },
            ]))),
            &mut queue,
        )
        .await
        .expect("apply string settings edit");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert!(
        std::iter::from_fn(|| commands.try_recv().ok())
            .any(|command| matches!(command, InlineCommand::ShowTransient { .. }))
    );
    assert!(matches!(
        palette_state,
        Some(ActivePalette::Settings { state, .. })
            if state.pending_edit_path.is_none() && state.draft.tools.editor.preferred_editor == "code --wait"
    ));
}
#[tokio::test]
async fn mode_palette_selection_selects_primary_agent() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Mode);
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
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(format!("{MODE_ACTION_PREFIX}auto")),
            ))),
            &mut queue,
        )
        .await
        .expect("process mode palette selection");

    assert!(matches!(
        action,
        InlineLoopAction::SelectPrimaryAgent { name: Some(ref name) } if name == "auto"
    ));
    assert!(palette_state.is_none());
}
