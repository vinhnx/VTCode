use super::*;

#[tokio::test]
async fn steering_events_are_passive_in_idle_loop() {
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

    {
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
        for event in [
            InlineEvent::Pause,
            InlineEvent::Resume,
            InlineEvent::Steer("keep going".into()),
        ] {
            let action = context.process_event(event, &mut queue).await.expect("process steering event");
            assert!(matches!(action, InlineLoopAction::Continue));
        }
    }

    // Undelivered steers fall through to the durable queue so the message is
    // processed once the agent is ready instead of vanishing.
    assert_eq!(queued_inputs.len(), 1);
    assert_eq!(queued_inputs.front().map(|q| q.input.text.as_str()), Some("keep going"));
}
#[tokio::test]
async fn steered_input_already_delivered_is_not_queued_twice() {
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

    {
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
        // Simulate the UI callback accepting this steer on the live channel.
        ctrl_c_state.mark_steer_delivered();
        let action = context
            .process_event(InlineEvent::Steer("already steered".into()), &mut queue)
            .await
            .expect("process delivered steer");
        assert!(matches!(action, InlineLoopAction::Continue));

        // A subsequent undelivered steer must still queue.
        let action = context
            .process_event(InlineEvent::Steer("needs queue".into()), &mut queue)
            .await
            .expect("process undelivered steer");
        assert!(matches!(action, InlineLoopAction::Continue));
    }
    // Only the undelivered steer landed in the queue.
    assert_eq!(queued_inputs.len(), 1);
    assert_eq!(queued_inputs.front().map(|q| q.input.text.as_str()), Some("needs queue"));
}
#[tokio::test]
async fn rapid_delivered_steers_are_not_double_queued() {
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

    {
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
        // Both steers are delivered to the live channel before the runloop
        // drains either event — the common rapid-influx ordering.
        ctrl_c_state.mark_steer_delivered();
        ctrl_c_state.mark_steer_delivered();
        for text in ["steer one", "steer two"] {
            let action = context
                .process_event(InlineEvent::Steer(text.into()), &mut queue)
                .await
                .expect("process delivered steer");
            assert!(matches!(action, InlineLoopAction::Continue));
        }
    }
    assert!(
        queued_inputs.is_empty(),
        "burst of delivered steers must not fall through to the queue, got {queued_inputs:?}"
    );
}
#[tokio::test]
async fn steer_with_attachments_restores_full_draft_and_continues() {
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

    let first_attachment = ContentPart::image("first-image", "image/png");
    let second_attachment = ContentPart::image("second-image", "image/png");
    let input = SubmittedInput::new("keep going", vec![first_attachment.clone(), second_attachment.clone()]);
    let action = context
        .process_event(InlineEvent::Steer(input.clone()), &mut queue)
        .await
        .expect("process steer with attachment");

    assert!(matches!(action, InlineLoopAction::Continue));
    let mut restored_draft = false;
    let mut warning_rendered = false;
    while let Ok(command) = commands.try_recv() {
        if let InlineCommand::Diagnostic(diagnostic) = &command
            && diagnostic
                .summary
                .contains("Live steering supports text only. Remove image attachments before steering.")
        {
            warning_rendered = true;
        }
        if let InlineCommand::RestoreInputDraft(restored_input) = command {
            assert_eq!(restored_input.text, "keep going");
            assert_eq!(restored_input.attachments, vec![first_attachment.clone(), second_attachment.clone()]);
            assert_eq!(restored_input, input);
            restored_draft = true;
        }
    }
    assert!(warning_rendered);
    assert!(restored_draft);
}
#[tokio::test]
async fn process_latest_queued_event_primes_newest_queue_priority() {
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
    let mut queued_inputs = VecDeque::from([
        QueuedInput::new("first".into(), Some("duck".to_string())),
        QueuedInput::new("latest".into(), Some("builder".to_string())),
    ]);
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::ProcessLatestQueued, &mut queue)
        .await
        .expect("process latest queued");

    assert!(matches!(action, InlineLoopAction::Continue));
    assert!(prefer_latest_once);
}
