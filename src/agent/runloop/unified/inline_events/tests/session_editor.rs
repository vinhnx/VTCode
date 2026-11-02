use super::*;

#[tokio::test]
async fn focused_exec_session_does_not_capture_slash_commands() {
    let temp_dir = tempfile::tempdir().expect("create workspace");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let response = registry
        .execute_harness_command_session(serde_json::json!({
            "action": "run",
            "command": "read line; printf received",
            "tty": false,
            "background": true,
            "stdin": true,
            "yield_time_ms": 250,
        }))
        .await
        .expect("start focused exec session");
    let session_id = response["session_id"].as_str().expect("background session id").to_string();
    let exec_sessions = registry.exec_session_manager();
    exec_sessions
        .focus_background_session(&session_id)
        .await
        .expect("focus background session");

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
    context.set_exec_session_manager(exec_sessions.clone());
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::Submit("/jobs".into()), &mut queue)
        .await
        .expect("process slash command");
    assert!(matches!(action, InlineLoopAction::Submit(input) if input.text == "/jobs"));

    context
        .process_event(
            InlineEvent::ExecSessionAction {
                id: session_id.clone(),
                action: vtcode_ui::tui::app::ExecSessionAction::Focus,
            },
            &mut queue,
        )
        .await
        .expect("toggle focused exec session");
    assert!(exec_sessions.focused_session_id().is_none());

    drop(context);
    registry
        .close_harness_exec_session(&session_id)
        .await
        .expect("close focused exec session");
}
#[tokio::test]
async fn force_cancel_with_registry_closes_live_foreground_session() {
    let temp_dir = tempfile::tempdir().expect("create workspace");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let response = registry
        .execute_harness_command_session(serde_json::json!({
            "action": "run",
            "command": "read line; printf received",
            "tty": false,
            "stdin": true,
            "yield_time_ms": 250,
        }))
        .await
        .expect("start foreground exec session");
    let session_id = response["session_id"].as_str().expect("foreground session id").to_string();
    let exec_sessions = registry.exec_session_manager();

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
    context.set_exec_session_manager(exec_sessions.clone());
    context.set_tool_registry(registry.clone());
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::ForceCancelPtySession, &mut queue)
        .await
        .expect("force-cancel live foreground session");
    assert!(matches!(action, InlineLoopAction::Continue));
    drop(context);
    // The force-cancelled session is closed and removed: closing it again
    // must fail. Verification reconciliation for terminated mutators is
    // covered at the registry level; this pins the wired registry-first path.
    assert!(registry.close_harness_exec_session(&session_id).await.is_err());
}
#[tokio::test]
async fn background_without_shortcut_result_requests_promotion() {
    // Desktop queues BackgroundOperation without the TUI send-time callback
    // that pre-requests the promotion: the handler must request it instead
    // of falling through to the panel-toggle submission.
    let temp_dir = tempfile::tempdir().expect("create workspace");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let response = registry
        .execute_harness_command_session(serde_json::json!({
            "action": "run",
            "command": "read line; printf received",
            "tty": false,
            "stdin": true,
            "yield_time_ms": 250,
        }))
        .await
        .expect("start foreground exec session");
    let session_id = response["session_id"].as_str().expect("foreground session id").to_string();
    let exec_sessions = registry.exec_session_manager();

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
    context.set_exec_session_manager(exec_sessions.clone());
    context.set_tool_registry(registry.clone());
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::BackgroundOperation, &mut queue)
        .await
        .expect("background live foreground session");
    assert!(matches!(action, InlineLoopAction::Continue));
    drop(context);
    registry
        .close_harness_exec_session(&session_id)
        .await
        .expect("close backgrounded session");
}
#[tokio::test]
async fn launch_editor_event_opens_editor_directly() {
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
        .process_event(InlineEvent::LaunchEditor { draft: "".to_string() }, &mut queue)
        .await
        .expect("process launch editor");
    assert!(matches!(
        action,
        InlineLoopAction::LaunchEditorWithDraft { ref draft } if draft.is_empty()
    ));
}
#[tokio::test]
async fn launch_editor_event_with_draft_returns_editor_with_draft_action() {
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
        .process_event(InlineEvent::LaunchEditor { draft: "hello world".to_string() }, &mut queue)
        .await
        .expect("process launch editor with draft");
    assert!(matches!(
        action,
        InlineLoopAction::LaunchEditorWithDraft { ref draft } if draft == "hello world"
    ));
}
